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

// ── Proxy RXTX buffers (registered with SPMD for PARTITION_INFO relay) ──

/// 4KB-aligned page for proxy TX/RX buffers (separate from per-VM guest mailboxes).
/// Used when SPMC is present (tfa_boot feature) for PARTITION_INFO relay.
#[cfg(feature = "tfa_boot")]
#[repr(C, align(4096))]
struct AlignedPage([u8; 4096]);

#[cfg(feature = "tfa_boot")]
#[allow(dead_code)] // Reserved for future MEM_SHARE descriptor forwarding to SPMC
static mut PROXY_TX_BUF: AlignedPage = AlignedPage([0u8; 4096]);
#[cfg(feature = "tfa_boot")]
static mut PROXY_RX_BUF: AlignedPage = AlignedPage([0u8; 4096]);

/// Whether the proxy's RXTX buffers have been successfully registered with SPMD.
#[cfg(feature = "tfa_boot")]
static PROXY_RXTX_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Initialize FF-A proxy. Probes EL3 for a real SPMC.
///
/// Called once at boot before guest entry.
pub fn init() {
    // When booted through TF-A (tfa_boot feature), SPMD+SPMC are present
    // by construction — no runtime probing needed.
    #[cfg(feature = "tfa_boot")]
    {
        SPMC_PRESENT.store(true, Ordering::Relaxed);
        crate::uart_puts(b"[FFA] TF-A boot: SPMC present (build-time)\n");

        // Register proxy RXTX buffers with SPMD for PARTITION_INFO relay
        let tx_pa = &raw const PROXY_TX_BUF as u64;
        let rx_pa = &raw const PROXY_RX_BUF as u64;
        let result = smc_forward::forward_smc8(
            FFA_RXTX_MAP,
            tx_pa,
            rx_pa,
            1, // 1 page
            0,
            0,
            0,
            0,
        );
        if result.x0 == FFA_SUCCESS_32 {
            PROXY_RXTX_REGISTERED.store(true, Ordering::Relaxed);
            crate::uart_puts(b"[FFA] Proxy RXTX registered with SPMD\n");
        } else {
            crate::uart_puts(b"[FFA] WARNING: Proxy RXTX registration failed\n");
        }

        return;
    }

    #[cfg(not(feature = "tfa_boot"))]
    {
        if smc_forward::probe_spmc() {
            SPMC_PRESENT.store(true, Ordering::Relaxed);
            crate::uart_puts(b"[FFA] Real SPMC detected at EL3\n");
        }
    }
}

/// Check if a real SPMC is present (for testing/debugging).
pub fn spmc_present() -> bool {
    SPMC_PRESENT.load(Ordering::Relaxed)
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

        // Supplemental calls
        FFA_SPM_ID_GET => handle_spm_id_get(context),
        FFA_RUN => handle_run(context),

        // Notifications
        FFA_NOTIFICATION_BITMAP_CREATE => handle_notification_bitmap_create(context),
        FFA_NOTIFICATION_BITMAP_DESTROY => handle_notification_bitmap_destroy(context),
        FFA_NOTIFICATION_BIND => handle_notification_bind(context),
        FFA_NOTIFICATION_UNBIND => handle_notification_unbind(context),
        FFA_NOTIFICATION_SET => handle_notification_set(context),
        FFA_NOTIFICATION_GET => handle_notification_get(context),
        FFA_NOTIFICATION_INFO_GET_32 | FFA_NOTIFICATION_INFO_GET_64 => {
            handle_notification_info_get(context)
        }

        // Indirect messaging
        FFA_MSG_SEND2 => handle_msg_send2(context),
        FFA_MSG_WAIT => handle_msg_wait(context),

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

/// Forward an FF-A call transparently to the Secure World (8-register).
///
/// Uses forward_smc8() to preserve x4-x7 (needed for DIRECT_REQ/RESP payload).
fn forward_ffa_to_spmc(context: &mut VcpuContext) -> bool {
    let result = smc_forward::forward_smc8(
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
    context.gp_regs.x4 = result.x4;
    context.gp_regs.x5 = result.x5;
    context.gp_regs.x6 = result.x6;
    context.gp_regs.x7 = result.x7;
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
            | FFA_SPM_ID_GET
            | FFA_RUN
            | FFA_NOTIFICATION_BITMAP_CREATE
            | FFA_NOTIFICATION_BITMAP_DESTROY
            | FFA_NOTIFICATION_BIND
            | FFA_NOTIFICATION_UNBIND
            | FFA_NOTIFICATION_SET
            | FFA_NOTIFICATION_GET
            | FFA_NOTIFICATION_INFO_GET_32
            | FFA_NOTIFICATION_INFO_GET_64
            | FFA_MSG_SEND2
            | FFA_MSG_WAIT
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

    // Validate IPAs are within guest RAM (prevents guest from targeting hypervisor memory).
    // Identity mapping means IPA == PA at EL2, so a malicious guest could otherwise
    // direct the proxy to write descriptors into hypervisor code/heap.
    #[cfg(feature = "linux_guest")]
    {
        let buf_size = page_count as u64 * 4096;
        if !is_guest_ram(tx_ipa, buf_size) || !is_guest_ram(rx_ipa, buf_size) {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
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
    mbox.msg_pending = false;
    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

// ── Stub SPMC: Partition Discovery ───────────────────────────────────

/// FFA_PARTITION_INFO_GET: Return partition info in RX buffer.
///
/// Input:  x1-x4 = UUID (or all zero for all partitions)
/// Output: x0 = FFA_SUCCESS_32, x2 = partition count
///         Partition descriptors written to VM's RX buffer.
///
/// When SPMC_PRESENT: forwards to SPMD, copies 24-byte descriptors from
/// proxy RX to guest RX. Otherwise falls back to stub SPMC (8-byte descs).
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

    #[cfg(feature = "tfa_boot")]
    if SPMC_PRESENT.load(Ordering::Relaxed) {
        // Ensure proxy RXTX buffers are registered before forwarding
        if !PROXY_RXTX_REGISTERED.load(Ordering::Relaxed) {
            ffa_error(context, FFA_DENIED);
            return true;
        }

        // Forward to real SPMC via SPMD
        let result = smc_forward::forward_smc8(
            FFA_PARTITION_INFO_GET,
            context.gp_regs.x1,
            context.gp_regs.x2,
            context.gp_regs.x3,
            context.gp_regs.x4,
            0,
            0,
            0,
        );
        if result.x0 != FFA_SUCCESS_32 {
            // Forward error to guest
            context.gp_regs.x0 = result.x0;
            context.gp_regs.x2 = result.x2;
            return true;
        }
        let count = result.x2 as usize;
        let bytes = count * 24; // 24 bytes per FF-A v1.1 descriptor
        let max_bytes = core::cmp::min(4096, mbox.page_count as usize * 4096);
        if bytes > max_bytes {
            ffa_error(context, FFA_NO_MEMORY);
            return true;
        }

        // Copy descriptors from proxy RX buffer to guest RX buffer.
        // rx_ipa was validated in handle_rxtx_map() to be within guest RAM.
        // Both are identity-mapped: VA == PA at EL2, IPA == PA for guest.
        unsafe {
            let src = &raw const PROXY_RX_BUF as *const u8;
            let dst = mbox.rx_ipa as *mut u8;
            core::ptr::copy_nonoverlapping(src, dst, bytes);
        }

        // Release proxy RX back to SPMD
        let release_result = smc_forward::forward_smc8(FFA_RX_RELEASE, 0, 0, 0, 0, 0, 0, 0);
        if release_result.x0 != FFA_SUCCESS_32 {
            crate::uart_puts(b"[FFA] WARNING: Proxy RX_RELEASE failed\n");
        }

        // Transfer guest RX ownership to VM
        mbox.rx_held_by_proxy = false;

        context.gp_regs.x0 = FFA_SUCCESS_32;
        context.gp_regs.x2 = count as u64;
        return true;
    }

    // Stub path: write 8-byte descriptors from stub partition data
    let rx_ptr = mbox.rx_ipa as *mut u8;
    let count = stub_spmc::partition_count();

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

    // If real SPMC present and receiver is an SP (ID >= 0x8000), forward
    if SPMC_PRESENT.load(Ordering::Relaxed) && receiver >= FFA_SPMC_ID {
        return forward_ffa_to_spmc(context);
    }

    // Stub path: validate receiver is a known SP, echo x3-x7
    if !stub_spmc::is_valid_sp(receiver) {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

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

// ── Supplemental Calls ──────────────────────────────────────────────

/// FFA_SPM_ID_GET: Return the SPMC partition ID.
///
/// Output: x0 = FFA_SUCCESS_32, x2 = 0x8000 (SPMC ID)
fn handle_spm_id_get(context: &mut VcpuContext) -> bool {
    context.gp_regs.x0 = FFA_SUCCESS_32;
    context.gp_regs.x2 = FFA_SPMC_ID as u64;
    true
}

/// FFA_RUN: Resume execution of a Secure Partition.
///
/// Input: x1[31:16] = target SP ID, x1[15:0] = vCPU ID
/// In NS-EL2 stub mode: forward to SPMC if present, else NOT_SUPPORTED.
fn handle_run(context: &mut VcpuContext) -> bool {
    if SPMC_PRESENT.load(Ordering::Relaxed) {
        forward_ffa_to_spmc(context)
    } else {
        ffa_error(context, FFA_NOT_SUPPORTED);
        true
    }
}

// ── Notifications ───────────────────────────────────────────────────

/// FFA_NOTIFICATION_BITMAP_CREATE: Create notification bitmap for a partition.
///
/// Input: x1 = partition ID, x2 = vCPU count
fn handle_notification_bitmap_create(context: &mut VcpuContext) -> bool {
    let part_id = context.gp_regs.x1 as u16;
    match notifications::bitmap_create(part_id) {
        Ok(()) => context.gp_regs.x0 = FFA_SUCCESS_32,
        Err(code) => ffa_error(context, code),
    }
    true
}

/// FFA_NOTIFICATION_BITMAP_DESTROY: Destroy notification bitmap for a partition.
///
/// Input: x1 = partition ID
fn handle_notification_bitmap_destroy(context: &mut VcpuContext) -> bool {
    let part_id = context.gp_regs.x1 as u16;
    match notifications::bitmap_destroy(part_id) {
        Ok(()) => context.gp_regs.x0 = FFA_SUCCESS_32,
        Err(code) => ffa_error(context, code),
    }
    true
}

/// FFA_NOTIFICATION_BIND: Bind sender to notification IDs on receiver.
///
/// Input: x1[31:16] = sender, x1[15:0] = receiver, x2 = flags, x3/x4 = bitmap
fn handle_notification_bind(context: &mut VcpuContext) -> bool {
    let sender = ((context.gp_regs.x1 >> 16) & 0xFFFF) as u16;
    let receiver = (context.gp_regs.x1 & 0xFFFF) as u16;
    let flags = context.gp_regs.x2 as u32;
    let bitmap = (context.gp_regs.x3 as u64) | ((context.gp_regs.x4 as u64) << 32);
    match notifications::bind(sender, receiver, flags, bitmap) {
        Ok(()) => context.gp_regs.x0 = FFA_SUCCESS_32,
        Err(code) => ffa_error(context, code),
    }
    true
}

/// FFA_NOTIFICATION_UNBIND: Unbind sender from notification IDs on receiver.
///
/// Input: x1[31:16] = sender, x1[15:0] = receiver, x3/x4 = bitmap
fn handle_notification_unbind(context: &mut VcpuContext) -> bool {
    let sender = ((context.gp_regs.x1 >> 16) & 0xFFFF) as u16;
    let receiver = (context.gp_regs.x1 & 0xFFFF) as u16;
    let bitmap = (context.gp_regs.x3 as u64) | ((context.gp_regs.x4 as u64) << 32);
    match notifications::unbind(sender, receiver, bitmap) {
        Ok(()) => context.gp_regs.x0 = FFA_SUCCESS_32,
        Err(code) => ffa_error(context, code),
    }
    true
}

/// FFA_NOTIFICATION_SET: Set pending notification bits on receiver.
///
/// Input: x1[31:16] = sender, x1[15:0] = receiver, x2 = flags, x3/x4 = bitmap
fn handle_notification_set(context: &mut VcpuContext) -> bool {
    let sender = ((context.gp_regs.x1 >> 16) & 0xFFFF) as u16;
    let receiver = (context.gp_regs.x1 & 0xFFFF) as u16;
    let _flags = context.gp_regs.x2 as u32;
    let bitmap = (context.gp_regs.x3 as u64) | ((context.gp_regs.x4 as u64) << 32);
    match notifications::set(sender, receiver, bitmap) {
        Ok(()) => context.gp_regs.x0 = FFA_SUCCESS_32,
        Err(code) => ffa_error(context, code),
    }
    true
}

/// FFA_NOTIFICATION_GET: Get and clear pending notification bits.
///
/// Input: x1 = receiver partition ID, x2 = flags (bit0=SP, bit1=VM)
/// Output: x2/x3 = SP notification bitmap, x4/x5 = VM notification bitmap
fn handle_notification_get(context: &mut VcpuContext) -> bool {
    let receiver = context.gp_regs.x1 as u16;
    match notifications::get(receiver) {
        Ok(pending) => {
            context.gp_regs.x0 = FFA_SUCCESS_32;
            // SP pending in x2/x3, VM pending in x4/x5
            // For simplicity, return all pending in x2/x3 (SP position)
            context.gp_regs.x2 = pending as u64;
            context.gp_regs.x3 = 0;
            context.gp_regs.x4 = 0;
            context.gp_regs.x5 = 0;
        }
        Err(code) => ffa_error(context, code),
    }
    true
}

/// FFA_NOTIFICATION_INFO_GET: Get list of partitions with pending notifications.
///
/// Output: x2 = flags (count in bits [6:0]), x3-x7 = partition IDs (16-bit packed)
fn handle_notification_info_get(context: &mut VcpuContext) -> bool {
    let (count, ids) = notifications::info_get();
    if count == 0 {
        ffa_error(context, FFA_NO_DATA);
    } else {
        context.gp_regs.x0 = FFA_SUCCESS_32;
        // Pack: count in x2[6:0], partition IDs in x3 (16-bit each, up to 4)
        context.gp_regs.x2 = count as u64;
        let mut packed: u64 = 0;
        for i in 0..core::cmp::min(count, 4) {
            packed |= (ids[i] as u64) << (i * 16);
        }
        context.gp_regs.x3 = packed;
    }
    true
}

// ── Indirect Messaging ──────────────────────────────────────────────

/// FFA_MSG_SEND2: Send indirect message via TX/RX buffers.
///
/// Input: x1 = flags
/// TX buffer contains: sender_id(u16) + receiver_id(u16) + size(u32) + payload
fn handle_msg_send2(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let sender_mbox = mailbox::get_mailbox(vm_id);

    if !sender_mbox.mapped {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    // Read message header from TX buffer (identity-mapped IPA)
    let tx_ipa = sender_mbox.tx_ipa;
    let (msg_sender_id, msg_receiver_id, msg_size) = unsafe {
        let tx_ptr = tx_ipa as *const u8;
        let s = core::ptr::read_unaligned(tx_ptr as *const u16);
        let r = core::ptr::read_unaligned(tx_ptr.add(2) as *const u16);
        let sz = core::ptr::read_unaligned(tx_ptr.add(4) as *const u32);
        (s, r, sz)
    };

    // Validate sender matches caller
    let expected_sender = vm_id_to_partition_id(vm_id);
    if msg_sender_id != expected_sender {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }

    // Validate receiver is a valid VM
    let recv_vm_id = match partition_id_to_vm_id(msg_receiver_id) {
        Some(id) => id,
        None => {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
    };

    // Copy message to receiver's RX buffer
    // Need to drop sender_mbox reference before getting recv_mbox
    let tx_ipa_copy = tx_ipa;

    let recv_mbox = mailbox::get_mailbox(recv_vm_id);
    if !recv_mbox.mapped {
        ffa_error(context, FFA_DENIED);
        return true;
    }
    if !recv_mbox.rx_held_by_proxy {
        ffa_error(context, FFA_BUSY);
        return true;
    }
    if recv_mbox.msg_pending {
        ffa_error(context, FFA_BUSY);
        return true;
    }

    // Copy header + payload from sender TX to receiver RX
    let copy_len = core::cmp::min((8 + msg_size) as usize, 4096);
    unsafe {
        core::ptr::copy_nonoverlapping(
            tx_ipa_copy as *const u8,
            recv_mbox.rx_ipa as *mut u8,
            copy_len,
        );
    }

    recv_mbox.msg_pending = true;
    recv_mbox.msg_sender_id = msg_sender_id;
    recv_mbox.rx_held_by_proxy = false;

    context.gp_regs.x0 = FFA_SUCCESS_32;
    true
}

/// FFA_MSG_WAIT: Wait for an indirect message.
///
/// Non-blocking stub: returns pending message or NO_DATA.
/// Output: x0 = FFA_SUCCESS_32 + x1 = sender_id, or FFA_ERROR + NO_DATA
fn handle_msg_wait(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let mbox = mailbox::get_mailbox(vm_id);

    if !mbox.mapped {
        ffa_error(context, FFA_DENIED);
        return true;
    }

    if mbox.msg_pending {
        context.gp_regs.x0 = FFA_SUCCESS_32;
        context.gp_regs.x1 = mbox.msg_sender_id as u64;
    } else {
        ffa_error(context, FFA_NO_DATA);
    }
    true
}

// ── Helper ───────────────────────────────────────────────────────────

/// Check if a guest IPA range falls within the guest RAM region.
///
/// Prevents a malicious guest from directing the proxy to write into
/// hypervisor memory (code, heap, page tables) via RXTX_MAP.
#[cfg(feature = "linux_guest")]
fn is_guest_ram(ipa: u64, len: u64) -> bool {
    let ram_start = crate::platform::GUEST_LOAD_ADDR;
    let ram_size = crate::platform::LINUX_MEM_SIZE;
    ipa >= ram_start && len <= ram_size && ipa <= ram_start + ram_size - len
}

/// Set FFA_ERROR return with error code.
/// FF-A error codes are 32-bit signed values in w2 (not sign-extended to 64-bit x2).
pub(crate) fn ffa_error(context: &mut VcpuContext, error_code: i32) {
    context.gp_regs.x0 = FFA_ERROR;
    context.gp_regs.x2 = (error_code as u32) as u64; // Mask to 32 bits, no sign extension
}
