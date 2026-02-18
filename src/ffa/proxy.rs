//! FF-A Proxy — main dispatch for FF-A SMC calls.
//!
//! Routes FF-A function IDs to local handlers or stub SPMC.

use crate::arch::aarch64::regs::VcpuContext;
use crate::ffa::*;

/// Handle an FF-A SMC call from guest.
///
/// Called from handle_smc() when function_id is in FF-A range.
/// Returns true to continue guest, false to exit.
pub fn handle_ffa_call(context: &mut VcpuContext) -> bool {
    let function_id = context.gp_regs.x0;

    match function_id {
        FFA_VERSION => handle_version(context),
        FFA_ID_GET => handle_id_get(context),
        FFA_FEATURES => handle_features(context),
        FFA_RXTX_MAP => handle_rxtx_map(context),
        FFA_RXTX_UNMAP => handle_rxtx_unmap(context),
        FFA_RX_RELEASE => handle_rx_release(context),
        FFA_PARTITION_INFO_GET => handle_partition_info_get(context),
        FFA_MSG_SEND_DIRECT_REQ_32 | FFA_MSG_SEND_DIRECT_REQ_64 => {
            handle_msg_send_direct_req(context)
        }
        FFA_MEM_SHARE_32 | FFA_MEM_SHARE_64 => handle_mem_share(context),
        FFA_MEM_LEND_32 | FFA_MEM_LEND_64 => handle_mem_lend(context),
        FFA_MEM_RECLAIM => handle_mem_reclaim(context),

        // Blocked: FFA_MEM_DONATE
        FFA_MEM_DONATE_32 | FFA_MEM_DONATE_64 => {
            ffa_error(context, FFA_NOT_SUPPORTED);
            true
        }

        // Not yet implemented — return NOT_SUPPORTED
        _ => {
            ffa_error(context, FFA_NOT_SUPPORTED);
            true
        }
    }
}

// ── Locally Handled ──────────────────────────────────────────────────

/// FFA_VERSION: Return supported FF-A version.
///
/// Input:  x1 = caller's version (ignored for now)
/// Output: x0 = FFA_VERSION_1_1 (0x00010001)
fn handle_version(context: &mut VcpuContext) -> bool {
    context.gp_regs.x0 = FFA_VERSION_1_1 as u64;
    true
}

/// FFA_ID_GET: Return the calling VM's FF-A partition ID.
///
/// Output: x0 = FFA_SUCCESS_32, x2 = partition ID
fn handle_id_get(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let part_id = vm_id_to_partition_id(vm_id);
    context.gp_regs.x0 = FFA_SUCCESS_32;
    context.gp_regs.x2 = part_id as u64;
    true
}

/// FFA_FEATURES: Query if a specific FF-A function is supported.
///
/// Input:  x1 = function ID to query
/// Output: x0 = FFA_SUCCESS_32 if supported, FFA_ERROR + NOT_SUPPORTED if not
fn handle_features(context: &mut VcpuContext) -> bool {
    let queried_fid = context.gp_regs.x1;
    let supported = matches!(queried_fid,
        FFA_VERSION | FFA_ID_GET | FFA_FEATURES |
        FFA_RXTX_MAP | FFA_RXTX_UNMAP | FFA_RX_RELEASE |
        FFA_PARTITION_INFO_GET |
        FFA_MSG_SEND_DIRECT_REQ_32 | FFA_MSG_SEND_DIRECT_REQ_64 |
        FFA_MEM_SHARE_32 | FFA_MEM_SHARE_64 |
        FFA_MEM_LEND_32 | FFA_MEM_LEND_64 |
        FFA_MEM_RECLAIM
    );

    if supported {
        context.gp_regs.x0 = FFA_SUCCESS_32;
        context.gp_regs.x2 = 0; // No additional feature properties
    } else {
        ffa_error(context, FFA_NOT_SUPPORTED);
    }
    true
}

// ── RXTX Mailbox ─────────────────────────────────────────────────────

/// FFA_RXTX_MAP (SMC64): Register TX/RX buffers.
///
/// Input:  x1 = TX buffer IPA, x2 = RX buffer IPA, x3 = page count
/// Output: x0 = FFA_SUCCESS_32 or FFA_ERROR
fn handle_rxtx_map(context: &mut VcpuContext) -> bool {
    let tx_ipa = context.gp_regs.x1;
    let rx_ipa = context.gp_regs.x2;
    let page_count = context.gp_regs.x3 as u32;

    // Validate: page-aligned, non-zero, reasonable size
    if tx_ipa & 0xFFF != 0 || rx_ipa & 0xFFF != 0 || page_count == 0 || page_count > 1 {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    let vm_id = crate::global::current_vm_id();
    let mbox = mailbox::get_mailbox(vm_id);

    if mbox.mapped {
        ffa_error(context, FFA_DENIED); // Already mapped
        return true;
    }

    mbox.tx_ipa = tx_ipa;
    mbox.rx_ipa = rx_ipa;
    mbox.page_count = page_count;
    mbox.mapped = true;
    mbox.rx_held_by_proxy = true;

    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

/// FFA_RXTX_UNMAP: Unregister TX/RX buffers.
///
/// Input:  x1 = partition ID (must match caller)
/// Output: x0 = FFA_SUCCESS_32 or FFA_ERROR
fn handle_rxtx_unmap(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let mbox = mailbox::get_mailbox(vm_id);

    if !mbox.mapped {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    *mbox = mailbox::FfaMailbox::new();
    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

/// FFA_RX_RELEASE: VM releases ownership of RX buffer back to proxy.
///
/// Output: x0 = FFA_SUCCESS_32 or FFA_ERROR
fn handle_rx_release(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let mbox = mailbox::get_mailbox(vm_id);

    if !mbox.mapped {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    mbox.rx_held_by_proxy = true;
    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

// ── Stub SPMC: Partition Discovery ───────────────────────────────────

/// FFA_PARTITION_INFO_GET: Return partition info in RX buffer.
///
/// Input:  x1-x4 = UUID (or all zero for all partitions)
/// Output: x0 = FFA_SUCCESS_32, x2 = partition count
///         Partition descriptors written to VM's RX buffer.
fn handle_partition_info_get(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let mbox = mailbox::get_mailbox(vm_id);

    if !mbox.mapped {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    if !mbox.rx_held_by_proxy {
        ffa_error(context, FFA_BUSY); // Proxy doesn't own RX buffer
        return true;
    }

    // Write partition info structs to RX buffer (identity mapped: IPA == PA)
    let rx_ptr = mbox.rx_ipa as *mut u8;
    let count = stub_spmc::partition_count();

    // FF-A v1.1 partition info descriptor: 24 bytes each (DEN0077A Table 5.37)
    // We use a minimal 8-byte subset for the stub (ID + ctx count + properties).
    // TODO: Expand to full 24-byte descriptor when integrating real SPMC.
    for (i, sp) in stub_spmc::STUB_PARTITIONS.iter().enumerate() {
        let offset = i * 8;
        unsafe {
            let ptr = rx_ptr.add(offset);
            // Partition ID (16-bit LE)
            core::ptr::write_volatile(ptr as *mut u16, sp.id);
            // Execution context count (16-bit LE)
            core::ptr::write_volatile(ptr.add(2) as *mut u16, sp.exec_ctx_count);
            // Properties (32-bit LE)
            core::ptr::write_volatile(ptr.add(4) as *mut u32, sp.properties);
        }
    }

    // Transfer RX ownership to VM
    mbox.rx_held_by_proxy = false;

    context.gp_regs.x0 = FFA_SUCCESS_32;
    context.gp_regs.x2 = count as u64;
    true
}

// ── Stub SPMC: Direct Messaging ──────────────────────────────────────

/// FFA_MSG_SEND_DIRECT_REQ: Send direct message to SP.
///
/// Input:  x1 = [31:16] sender, [15:0] receiver
///         x3-x7 = message data
/// Output: FFA_MSG_SEND_DIRECT_RESP with echoed x4-x7
fn handle_msg_send_direct_req(context: &mut VcpuContext) -> bool {
    let sender = ((context.gp_regs.x1 >> 16) & 0xFFFF) as u16;
    let receiver = (context.gp_regs.x1 & 0xFFFF) as u16;

    // Validate sender is the calling VM
    let vm_id = crate::global::current_vm_id();
    let expected_sender = vm_id_to_partition_id(vm_id);
    if sender != expected_sender {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // Validate receiver is a known SP
    if !stub_spmc::is_valid_sp(receiver) {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // Stub SPMC: echo back x3-x7 as direct response
    let x3 = context.gp_regs.x3;
    let x4 = context.gp_regs.x4;
    let x5 = context.gp_regs.x5;
    let x6 = context.gp_regs.x6;
    let x7 = context.gp_regs.x7;

    // Return FFA_MSG_SEND_DIRECT_RESP
    let is_64bit = context.gp_regs.x0 == FFA_MSG_SEND_DIRECT_REQ_64;
    context.gp_regs.x0 = if is_64bit {
        FFA_MSG_SEND_DIRECT_RESP_64
    } else {
        FFA_MSG_SEND_DIRECT_RESP_32
    };
    // x1 = [31:16] responder (SP), [15:0] receiver (VM)
    context.gp_regs.x1 = ((receiver as u64) << 16) | (sender as u64);
    context.gp_regs.x3 = x3;
    context.gp_regs.x4 = x4;
    context.gp_regs.x5 = x5;
    context.gp_regs.x6 = x6;
    context.gp_regs.x7 = x7;
    true
}

// ── Memory Sharing ───────────────────────────────────────────────────

/// FFA_MEM_SHARE: Share memory pages with a secure partition.
///
/// Simplified interface (no RXTX descriptor parsing):
///   x3 = IPA of first page to share
///   x4 = page count
///   x5 = receiver partition ID
///
/// NOTE: Real FF-A v1.1 MEM_SHARE uses composite memory region descriptors
/// in the TX buffer (DEN0077A §5.12). This stub uses registers for testability.
fn handle_mem_share(context: &mut VcpuContext) -> bool {
    let _ipa = context.gp_regs.x3;
    let page_count = context.gp_regs.x4 as u32;
    let receiver_id = context.gp_regs.x5 as u16;

    if page_count == 0 {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // Validate receiver is a known SP
    if !stub_spmc::is_valid_sp(receiver_id) {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    let vm_id = crate::global::current_vm_id();
    let sender_id = vm_id_to_partition_id(vm_id);

    // Record the share in stub SPMC
    let handle = match stub_spmc::record_share(sender_id, receiver_id, page_count) {
        Some(h) => h,
        None => {
            ffa_error(context, FFA_NO_MEMORY);
            return true;
        }
    };

    // Return success with handle
    context.gp_regs.x0 = FFA_SUCCESS_32;
    // Handle is 64-bit, returned in x2 (low) and x3 (high)
    context.gp_regs.x2 = handle & 0xFFFF_FFFF;
    context.gp_regs.x3 = handle >> 32;
    true
}

/// FFA_MEM_LEND: Lend memory pages to a secure partition.
/// Same as share for the stub implementation.
fn handle_mem_lend(context: &mut VcpuContext) -> bool {
    handle_mem_share(context)
}

/// FFA_MEM_RECLAIM: Reclaim previously shared/lent memory.
///
/// Input: x1 = handle (low 32), x2 = handle (high 32), x3 = flags
/// Output: x0 = FFA_SUCCESS_32 or FFA_ERROR
fn handle_mem_reclaim(context: &mut VcpuContext) -> bool {
    let handle = (context.gp_regs.x1 & 0xFFFF_FFFF)
        | ((context.gp_regs.x2 & 0xFFFF_FFFF) << 32);

    if stub_spmc::reclaim_share(handle) {
        context.gp_regs.x0 = FFA_SUCCESS_32;
    } else {
        ffa_error(context, FFA_INVALID_PARAMETERS);
    }
    true
}

// ── Helper ───────────────────────────────────────────────────────────

/// Set FFA_ERROR return with error code.
/// FF-A error codes are 32-bit signed values in w2 (not sign-extended to 64-bit x2).
pub(crate) fn ffa_error(context: &mut VcpuContext, error_code: i32) {
    context.gp_regs.x0 = FFA_ERROR;
    context.gp_regs.x2 = (error_code as u32) as u64; // Mask to 32 bits, no sign extension
}
