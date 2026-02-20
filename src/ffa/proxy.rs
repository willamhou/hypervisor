//! FF-A Proxy — main dispatch for FF-A SMC calls.
//!
//! Routes FF-A function IDs to local handlers or stub SPMC.
//! Validates page ownership via Stage-2 PTE SW bits before allowing
//! memory sharing operations (pKVM-compatible).

use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "linux_guest")]
use crate::arch::aarch64::defs::*;
use crate::arch::aarch64::regs::VcpuContext;
use crate::ffa::*;

/// Whether a real SPMC was detected at EL3 during init.
static SPMC_PRESENT: AtomicBool = AtomicBool::new(false);

/// Initialize FF-A proxy. Probes EL3 for a real SPMC.
///
/// Called once at boot before guest entry.
pub fn init() {
    if smc_forward::probe_spmc() {
        SPMC_PRESENT.store(true, Ordering::Relaxed);
        crate::uart_puts(b"[FFA] Real SPMC detected at EL3\n");
    }
}

/// Handle an FF-A SMC call from guest.
///
/// Called from handle_smc() when function_id is in FF-A range.
/// Returns true to continue guest, false to exit.
pub fn handle_ffa_call(context: &mut VcpuContext) -> bool {
    let function_id = context.gp_regs.x0;

    match function_id {
        // Always handled locally (proxy policy, same as pKVM)
        FFA_VERSION => handle_version(context),
        FFA_ID_GET => handle_id_get(context),
        FFA_FEATURES => handle_features(context),
        FFA_RXTX_MAP => handle_rxtx_map(context),
        FFA_RXTX_UNMAP => handle_rxtx_unmap(context),
        FFA_RX_RELEASE => handle_rx_release(context),
        FFA_PARTITION_INFO_GET => handle_partition_info_get(context),

        // Direct messaging: forward to SPMC if present, else stub
        FFA_MSG_SEND_DIRECT_REQ_32 | FFA_MSG_SEND_DIRECT_REQ_64 => {
            handle_msg_send_direct_req(context)
        }

        // Memory operations: validate ownership, then stub SPMC or forward
        FFA_MEM_SHARE_32 | FFA_MEM_SHARE_64 => handle_mem_share(context),
        FFA_MEM_LEND_32 | FFA_MEM_LEND_64 => handle_mem_lend(context),
        FFA_MEM_RECLAIM => handle_mem_reclaim(context),
        FFA_MEM_RETRIEVE_REQ_32 | FFA_MEM_RETRIEVE_REQ_64 => handle_mem_retrieve_req(context),
        FFA_MEM_RELINQUISH => handle_mem_relinquish(context),

        // Blocked: FFA_MEM_DONATE (pKVM policy)
        FFA_MEM_DONATE_32 | FFA_MEM_DONATE_64 => {
            ffa_error(context, FFA_NOT_SUPPORTED);
            true
        }

        // Unknown FF-A: forward to SPMC if present, else NOT_SUPPORTED
        _ => {
            if SPMC_PRESENT.load(Ordering::Relaxed) {
                forward_ffa_to_spmc(context)
            } else {
                ffa_error(context, FFA_NOT_SUPPORTED);
                true
            }
        }
    }
}

/// Forward an FF-A call transparently to the Secure World.
fn forward_ffa_to_spmc(context: &mut VcpuContext) -> bool {
    let result = smc_forward::forward_smc(
        context.gp_regs.x0,
        context.gp_regs.x1,
        context.gp_regs.x2,
        context.gp_regs.x3,
        context.gp_regs.x4,
        context.gp_regs.x5,
        context.gp_regs.x6,
        context.gp_regs.x7,
    );
    context.gp_regs.x0 = result.x0;
    context.gp_regs.x1 = result.x1;
    context.gp_regs.x2 = result.x2;
    context.gp_regs.x3 = result.x3;
    true
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
    let supported = matches!(
        queried_fid,
        FFA_VERSION
            | FFA_ID_GET
            | FFA_FEATURES
            | FFA_RXTX_MAP
            | FFA_RXTX_UNMAP
            | FFA_RX_RELEASE
            | FFA_PARTITION_INFO_GET
            | FFA_MSG_SEND_DIRECT_REQ_32
            | FFA_MSG_SEND_DIRECT_REQ_64
            | FFA_MEM_SHARE_32
            | FFA_MEM_SHARE_64
            | FFA_MEM_LEND_32
            | FFA_MEM_LEND_64
            | FFA_MEM_RECLAIM
            | FFA_MEM_RETRIEVE_REQ_32
            | FFA_MEM_RETRIEVE_REQ_64
            | FFA_MEM_RELINQUISH
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
/// Two interfaces supported:
/// 1. **Descriptor-based** (FF-A v1.1 compliant): If RXTX mailbox is mapped,
///    reads composite memory region descriptor from TX buffer.
///    x1 = total_length, x2 = fragment_length.
/// 2. **Register-based** (fallback for testing): If no mailbox,
///    x3 = IPA, x4 = page_count, x5 = receiver_id.
///
/// Validates page ownership via Stage-2 PTE SW bits and transitions
/// pages from Owned → SharedOwned. Sets S2AP to RO for shared pages.
fn handle_mem_share(context: &mut VcpuContext) -> bool {
    handle_mem_share_or_lend(context, false)
}

/// FFA_MEM_LEND: Lend memory pages to a secure partition.
///
/// Same as share but sets S2AP to NONE (no access) instead of RO.
fn handle_mem_lend(context: &mut VcpuContext) -> bool {
    handle_mem_share_or_lend(context, true)
}

/// Unified handler for MEM_SHARE and MEM_LEND.
///
/// - is_lend=false (SHARE): pages become S2AP_RO (guest retains read)
/// - is_lend=true  (LEND):  pages become S2AP_NONE (guest loses access)
fn handle_mem_share_or_lend(context: &mut VcpuContext, is_lend: bool) -> bool {
    let vm_id = crate::global::current_vm_id();
    let mbox = mailbox::get_mailbox(vm_id);

    // Choose interface: descriptor-based (mailbox mapped) or register-based (fallback)
    let (sender_id_from_desc, receiver_id, ranges, range_count, total_page_count) = if mbox.mapped {
        // FF-A v1.1 descriptor path: parse TX buffer
        match parse_share_descriptor(context, mbox) {
            Ok(info) => info,
            Err(code) => {
                ffa_error(context, code);
                return true;
            }
        }
    } else {
        // Register-based fallback (for unit tests and simple use)
        let base_ipa = context.gp_regs.x3;
        let page_count = context.gp_regs.x4 as u32;
        let receiver_id = context.gp_regs.x5 as u16;
        if page_count == 0 {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
        let mut ranges = [(0u64, 0u32); descriptors::MAX_ADDR_RANGES];
        ranges[0] = (base_ipa, page_count);
        (0u16, receiver_id, ranges, 1usize, page_count)
    };

    // Validate receiver is a known partition (VM or SP)
    if !is_valid_receiver(receiver_id) {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // Validate sender matches caller (only for descriptor path where sender is explicit)
    let expected_sender = vm_id_to_partition_id(vm_id);
    if sender_id_from_desc != 0 && sender_id_from_desc != expected_sender {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // Validate and transition page ownership via Stage-2 PTE SW bits.
    // Only when running actual VMs (linux_guest feature), not unit tests.
    // In unit test mode, VTTBR may contain stale values from earlier page table tests.
    #[cfg(feature = "linux_guest")]
    {
        let walker = stage2_walker::Stage2Walker::from_vttbr();
        if walker.has_stage2() {
            // Validate: all pages must be in Owned state
            for i in 0..range_count {
                let (base_ipa, page_count) = ranges[i];
                for p in 0..page_count as u64 {
                    let ipa = base_ipa + p * PAGE_SIZE_4KB;
                    match walker.read_sw_bits(ipa) {
                        Some(sw) => {
                            if let Err(code) = memory::validate_page_for_share(sw) {
                                ffa_error(context, code);
                                return true;
                            }
                        }
                        None => {
                            ffa_error(context, FFA_DENIED);
                            return true;
                        }
                    }
                }
            }

            // Transition pages: Owned -> SharedOwned, restrict S2AP
            let new_sw = memory::PageOwnership::SharedOwned as u8;
            let new_s2ap = if is_lend {
                (S2AP_NONE >> S2AP_SHIFT) as u8
            } else {
                (S2AP_RO >> S2AP_SHIFT) as u8
            };
            for i in 0..range_count {
                let (base_ipa, page_count) = ranges[i];
                for p in 0..page_count as u64 {
                    let ipa = base_ipa + p * PAGE_SIZE_4KB;
                    let _ = walker.write_sw_bits(ipa, new_sw);
                    let _ = walker.set_s2ap(ipa, new_s2ap);
                }
            }
        }
    }

    let sender_id = expected_sender;

    // Record the share in stub SPMC
    let handle = match stub_spmc::record_share(
        sender_id,
        receiver_id,
        &ranges[..range_count],
        total_page_count,
        is_lend,
    ) {
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

/// Parse a FF-A v1.1 composite memory region descriptor from the TX buffer.
///
/// Returns (sender_id, receiver_id, ranges, range_count, total_page_count).
fn parse_share_descriptor(
    context: &VcpuContext,
    mbox: &mailbox::FfaMailbox,
) -> Result<
    (
        u16,
        u16,
        [(u64, u32); descriptors::MAX_ADDR_RANGES],
        usize,
        u32,
    ),
    i32,
> {
    let total_length = context.gp_regs.x1 as u32;
    let fragment_length = context.gp_regs.x2 as u32;

    // No fragmentation support: entire descriptor must fit in one TX buffer
    if total_length != fragment_length || total_length == 0 {
        return Err(FFA_INVALID_PARAMETERS);
    }

    // Identity mapping: IPA == PA, safe to read TX buffer directly at EL2
    let tx_ptr = mbox.tx_ipa as *const u8;

    let parsed = unsafe { descriptors::parse_mem_region(tx_ptr, total_length)? };

    Ok((
        parsed.sender_id,
        parsed.receiver_id,
        parsed.ranges,
        parsed.range_count,
        parsed.total_page_count,
    ))
}

/// FFA_MEM_RECLAIM: Reclaim previously shared/lent memory.
///
/// Input: x1 = handle (low 32), x2 = handle (high 32), x3 = flags
/// Output: x0 = FFA_SUCCESS_32 or FFA_ERROR
///
/// Restores page ownership to Owned and S2AP to RW.
fn handle_mem_reclaim(context: &mut VcpuContext) -> bool {
    let handle = (context.gp_regs.x1 & 0xFFFF_FFFF) | ((context.gp_regs.x2 & 0xFFFF_FFFF) << 32);

    // Look up share record (need IPA info for restoration + retrieved status)
    let info = match stub_spmc::lookup_share_full(handle) {
        Some(info) => info,
        None => {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
    };

    // Block reclaim while share is still retrieved by receiver
    if info.retrieved {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    // Restore pages to Owned + S2AP_RW.
    // Only when running actual VMs (linux_guest feature), not unit tests.
    #[cfg(feature = "linux_guest")]
    {
        let walker = stage2_walker::Stage2Walker::from_vttbr();
        if walker.has_stage2() {
            let owned_sw = memory::PageOwnership::Owned as u8;
            let rw_s2ap = (S2AP_RW >> S2AP_SHIFT) as u8;
            for i in 0..info.range_count {
                let (base_ipa, page_count) = info.ranges[i];
                for p in 0..page_count as u64 {
                    let ipa = base_ipa + p * PAGE_SIZE_4KB;
                    let _ = walker.write_sw_bits(ipa, owned_sw);
                    let _ = walker.set_s2ap(ipa, rw_s2ap);
                }
            }
        }
    }

    // Now remove the record
    stub_spmc::reclaim_share(handle);
    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

/// FFA_MEM_RETRIEVE_REQ: Receiver retrieves previously shared memory.
///
/// Input: x1 = handle (low 32), x2 = handle (high 32)
/// Output: x0 = FFA_MEM_RETRIEVE_RESP or FFA_ERROR
///
/// For VM receivers: maps shared pages into receiver's Stage-2 via map_page().
/// For SP receivers: returns NOT_SUPPORTED (stub SPMC has no Stage-2).
fn handle_mem_retrieve_req(context: &mut VcpuContext) -> bool {
    let handle = (context.gp_regs.x1 & 0xFFFF_FFFF) | ((context.gp_regs.x2 & 0xFFFF_FFFF) << 32);

    // Look up the share record
    let info = match stub_spmc::lookup_share_full(handle) {
        Some(info) => info,
        None => {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
    };

    // Verify caller is the designated receiver
    let vm_id = crate::global::current_vm_id();
    let caller_id = vm_id_to_partition_id(vm_id);
    if caller_id != info.receiver_id {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    // Check not already retrieved
    if info.retrieved {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    // Only VM receivers get Stage-2 mapping; SP receivers are stub-only
    if is_vm_partition(info.receiver_id) {
        #[cfg(feature = "linux_guest")]
        {
            let recv_vm_id = partition_id_to_vm_id(info.receiver_id).unwrap();
            let l0_pa =
                crate::global::PER_VM_VTTBR[recv_vm_id].load(core::sync::atomic::Ordering::Acquire);
            if l0_pa != 0 {
                let walker = stage2_walker::Stage2Walker::new(l0_pa);
                let s2ap = (S2AP_RW >> S2AP_SHIFT) as u8;
                let sw = memory::PageOwnership::SharedBorrowed as u8;
                for i in 0..info.range_count {
                    let (base_ipa, page_count) = info.ranges[i];
                    for p in 0..page_count as u64 {
                        let ipa = base_ipa + p * PAGE_SIZE_4KB;
                        if let Err(_) = walker.map_page(ipa, s2ap, sw) {
                            // Rollback: unmap pages we already mapped
                            // (best effort -- ignore errors on rollback)
                            for j in 0..=i {
                                let (rb_ipa, rb_count) = info.ranges[j];
                                let end = if j == i { p } else { rb_count as u64 };
                                for k in 0..end {
                                    let _ = walker.unmap_page(rb_ipa + k * PAGE_SIZE_4KB);
                                }
                            }
                            ffa_error(context, FFA_DENIED);
                            return true;
                        }
                    }
                }
            }
        }
    }

    // Mark as retrieved
    stub_spmc::mark_retrieved(handle);

    // Return FFA_MEM_RETRIEVE_RESP
    context.gp_regs.x0 = FFA_MEM_RETRIEVE_RESP;
    // x1 = total_length (0 for register-based), x2/x3 = handle
    context.gp_regs.x1 = 0;
    context.gp_regs.x2 = handle & 0xFFFF_FFFF;
    context.gp_regs.x3 = handle >> 32;
    true
}

/// FFA_MEM_RELINQUISH: Receiver gives up access to previously retrieved memory.
///
/// Input: x1 = handle (low 32), x2 = handle (high 32)
/// Output: x0 = FFA_SUCCESS_32 or FFA_ERROR
///
/// For VM receivers: unmaps shared pages from receiver's Stage-2 via unmap_page().
fn handle_mem_relinquish(context: &mut VcpuContext) -> bool {
    let handle = (context.gp_regs.x1 & 0xFFFF_FFFF) | ((context.gp_regs.x2 & 0xFFFF_FFFF) << 32);

    // Look up the share record
    let info = match stub_spmc::lookup_share_full(handle) {
        Some(info) => info,
        None => {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
    };

    // Verify caller is the designated receiver
    let vm_id = crate::global::current_vm_id();
    let caller_id = vm_id_to_partition_id(vm_id);
    if caller_id != info.receiver_id {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    // Must be currently retrieved
    if !info.retrieved {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    // Unmap pages from receiver's Stage-2
    if is_vm_partition(info.receiver_id) {
        #[cfg(feature = "linux_guest")]
        {
            let recv_vm_id = partition_id_to_vm_id(info.receiver_id).unwrap();
            let l0_pa =
                crate::global::PER_VM_VTTBR[recv_vm_id].load(core::sync::atomic::Ordering::Acquire);
            if l0_pa != 0 {
                let walker = stage2_walker::Stage2Walker::new(l0_pa);
                for i in 0..info.range_count {
                    let (base_ipa, page_count) = info.ranges[i];
                    for p in 0..page_count as u64 {
                        let ipa = base_ipa + p * PAGE_SIZE_4KB;
                        let _ = walker.unmap_page(ipa);
                    }
                }
            }
        }
    }

    // Mark as relinquished
    stub_spmc::mark_relinquished(handle);

    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

// ── Helper ───────────────────────────────────────────────────────────

/// Set FFA_ERROR return with error code.
/// FF-A error codes are 32-bit signed values in w2 (not sign-extended to 64-bit x2).
pub(crate) fn ffa_error(context: &mut VcpuContext, error_code: i32) {
    context.gp_regs.x0 = FFA_ERROR;
    context.gp_regs.x2 = (error_code as u32) as u64; // Mask to 32 bits, no sign extension
}
