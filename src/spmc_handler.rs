//! SPMC Event Loop — FF-A request dispatch for S-EL2 SPMC role.
//!
//! When booted as BL32 at S-EL2, the hypervisor acts as the SPMC (Secure
//! Partition Manager Core). After initialization, it sends FFA_MSG_WAIT to
//! SPMD (EL3), which returns the first Normal World FF-A request. The SPMC
//! then enters an event loop: dispatch the request, send the response via
//! SMC, and receive the next request.
//!
//! TF-A v2.12 SPMD forwards FFA_RXTX_MAP, RXTX_UNMAP, RX_RELEASE, and
//! PARTITION_INFO_GET from NWd directly to the SPMC. The SPMC manages NWd
//! RXTX state and writes PARTITION_INFO descriptors to the NWd RX buffer.

use crate::ffa;
use crate::ffa::smc_forward::SmcResult8;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, AtomicU64};
#[cfg(feature = "sel2")]
use core::sync::atomic::Ordering;

/// Flag set by the IRQ handler when a physical IRQ preempts SP execution.
/// The SPMC event loop checks this after enter_guest() returns to decide
/// whether to return FFA_INTERRUPT (preempted) or DIRECT_RESP (completed).
pub static SP_IRQ_PREEMPTED: AtomicBool = AtomicBool::new(false);

/// Debug: count FIQ preemptions to detect infinite loops.
#[cfg(feature = "sel2")]
pub static FIQ_PREEMPT_COUNT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Saved host ELR_EL2/SPSR_EL2 across enter_guest() calls.
/// Using globals instead of stack locals because debug-mode stack slots can get
/// corrupted by the enter_guest/exception_handler/guest_exit stack sequence.
#[cfg(feature = "sel2")]
static SAVED_HOST_ELR: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "sel2")]
static SAVED_HOST_SPSR: AtomicU64 = AtomicU64::new(0);
/// SP context pointer saved to global before enter_guest().
#[cfg(feature = "sel2")]
static SAVED_SP_PTR: AtomicU64 = AtomicU64::new(0);

/// Tracks which SP is currently executing at S-EL1. Set before enter_guest(),
/// cleared after return. Used by the IRQ handler to inject virtual interrupts
/// directly via LR without going through SPMD.
#[cfg(feature = "sel2")]
static CURRENT_RUNNING_SP: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);

/// Get the currently running SP ID (0 if none).
#[cfg(feature = "sel2")]
pub fn current_running_sp() -> u16 {
    CURRENT_RUNNING_SP.load(Ordering::Acquire)
}

// ── NWd RXTX state (SPMD forwards RXTX_MAP from NWd to SPMC) ──

/// Tracks the Normal World endpoint's RXTX buffer registration.
/// SPMD at EL3 forwards FFA_RXTX_MAP from NWd to SPMC (not handled by SPMD).
struct NwdRxtxState {
    tx_pa: u64,
    rx_pa: u64,
    page_count: u32,
    mapped: bool,
}

static mut NWD_RXTX: NwdRxtxState = NwdRxtxState {
    tx_pa: 0,
    rx_pa: 0,
    page_count: 0,
    mapped: false,
};

// ── SPMC-side memory share records ──────────────────────────────────

const MAX_SHARE_RANGES: usize = 4;
const MAX_SPMC_SHARES: usize = 16;

/// SPMC memory share record (mirrors stub_spmc::MemShareRecord).
struct SpmcShareRecord {
    handle: u64,
    sender_id: u16,
    receiver_id: u16,
    ranges: [(u64, u32); MAX_SHARE_RANGES],
    range_count: usize,
    active: bool,
    is_lend: bool,
    retrieved: bool,
}

struct SpmcShareArray(UnsafeCell<[SpmcShareRecord; MAX_SPMC_SHARES]>);
unsafe impl Sync for SpmcShareArray {}

static SPMC_SHARES: SpmcShareArray = SpmcShareArray(UnsafeCell::new({
    const EMPTY: SpmcShareRecord = SpmcShareRecord {
        handle: 0,
        sender_id: 0,
        receiver_id: 0,
        ranges: [(0, 0); MAX_SHARE_RANGES],
        range_count: 0,
        active: false,
        is_lend: false,
        retrieved: false,
    };
    [
        EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
        EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
    ]
}));

static SPMC_NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn spmc_alloc_handle() -> u64 {
    SPMC_NEXT_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// Record a memory share. Returns the handle on success.
fn record_spmc_share(
    sender_id: u16,
    receiver_id: u16,
    ranges: &[(u64, u32)],
    is_lend: bool,
) -> Option<u64> {
    let handle = spmc_alloc_handle();
    let records = unsafe { &mut *SPMC_SHARES.0.get() };
    for record in records.iter_mut() {
        if !record.active {
            let mut stored = [(0u64, 0u32); MAX_SHARE_RANGES];
            let count = ranges.len().min(MAX_SHARE_RANGES);
            for (i, &r) in ranges.iter().take(count).enumerate() {
                stored[i] = r;
            }
            *record = SpmcShareRecord {
                handle,
                sender_id,
                receiver_id,
                ranges: stored,
                range_count: count,
                active: true,
                is_lend,
                retrieved: false,
            };
            return Some(handle);
        }
    }
    None
}

/// Look up a share record by handle (immutable).
fn lookup_spmc_share(handle: u64) -> Option<(u16, u16, [(u64, u32); MAX_SHARE_RANGES], usize, bool, bool)> {
    let records = unsafe { &*SPMC_SHARES.0.get() };
    for record in records.iter() {
        if record.active && record.handle == handle {
            return Some((
                record.sender_id,
                record.receiver_id,
                record.ranges,
                record.range_count,
                record.is_lend,
                record.retrieved,
            ));
        }
    }
    None
}

/// Mark a share as retrieved. Returns true if successful.
fn mark_spmc_retrieved(handle: u64) -> bool {
    let records = unsafe { &mut *SPMC_SHARES.0.get() };
    for record in records.iter_mut() {
        if record.active && record.handle == handle && !record.retrieved {
            record.retrieved = true;
            return true;
        }
    }
    false
}

/// Mark a share as relinquished. Returns true if successful.
fn mark_spmc_relinquished(handle: u64) -> bool {
    let records = unsafe { &mut *SPMC_SHARES.0.get() };
    for record in records.iter_mut() {
        if record.active && record.handle == handle && record.retrieved {
            record.retrieved = false;
            return true;
        }
    }
    false
}

/// Reclaim (delete) a share record. Fails if still retrieved.
fn reclaim_spmc_share(handle: u64) -> Result<(), i32> {
    let records = unsafe { &mut *SPMC_SHARES.0.get() };
    for record in records.iter_mut() {
        if record.active && record.handle == handle {
            if record.retrieved {
                return Err(ffa::FFA_DENIED);
            }
            record.active = false;
            return Ok(());
        }
    }
    Err(ffa::FFA_INVALID_PARAMETERS)
}

/// Save S-EL2 host context (ELR_EL2, SPSR_EL2) before entering an SP.
#[cfg(feature = "sel2")]
#[inline(always)]
fn save_host_el2_state() -> (u64, u64) {
    let elr: u64;
    let spsr: u64;
    unsafe {
        core::arch::asm!("mrs {}, elr_el2", out(reg) elr, options(nostack, nomem));
        core::arch::asm!("mrs {}, spsr_el2", out(reg) spsr, options(nostack, nomem));
    }
    (elr, spsr)
}

/// Restore S-EL2 host context (ELR_EL2, SPSR_EL2) after SP exit.
#[cfg(feature = "sel2")]
#[inline(always)]
fn restore_host_el2_state(elr: u64, spsr: u64) {
    unsafe {
        core::arch::asm!("msr elr_el2, {}", in(reg) elr, options(nostack, nomem));
        core::arch::asm!("msr spsr_el2, {}", in(reg) spsr, options(nostack, nomem));
        core::arch::asm!("isb", options(nostack, nomem));
    }
}

/// Common preamble before enter_guest: clear FMO, mask PSTATE.F, save host state.
#[cfg(feature = "sel2")]
#[inline(always)]
fn pre_enter_guest(sp: &mut crate::sp_context::SpContext) {
    // Clear HCR_EL2.FMO so NS FIQ doesn't trap to S-EL2 during SP execution.
    // Mask FIQ in PSTATE.F at S-EL2 to prevent FIQ during enter_guest asm.
    unsafe {
        core::arch::asm!(
            "mrs {tmp}, hcr_el2",
            "bic {tmp}, {tmp}, #(1 << 3)",
            "msr hcr_el2, {tmp}",
            "msr DAIFSet, #1",
            "isb",
            tmp = out(reg) _,
            options(nostack, nomem),
        );
    }
    let (saved_elr, saved_spsr) = save_host_el2_state();
    SAVED_HOST_ELR.store(saved_elr, Ordering::Relaxed);
    SAVED_HOST_SPSR.store(saved_spsr, Ordering::Relaxed);
    SAVED_SP_PTR.store(sp as *mut _ as u64, Ordering::Relaxed);
}

/// Common postamble after enter_guest: restore host state, re-enable FMO.
/// Returns the reloaded SP reference (safe from stack corruption).
#[cfg(feature = "sel2")]
#[inline(always)]
fn post_enter_guest() -> &'static mut crate::sp_context::SpContext {
    let sp = unsafe {
        &mut *(SAVED_SP_PTR.load(Ordering::Relaxed)
            as *mut crate::sp_context::SpContext)
    };
    restore_host_el2_state(
        SAVED_HOST_ELR.load(Ordering::Relaxed),
        SAVED_HOST_SPSR.load(Ordering::Relaxed),
    );
    unsafe {
        core::arch::asm!("msr tpidr_el2, xzr", options(nostack, nomem));
    }
    // Restore HCR_EL2.FMO and unmask PSTATE.F
    unsafe {
        core::arch::asm!(
            "mrs {tmp}, hcr_el2",
            "orr {tmp}, {tmp}, #(1 << 3)",
            "msr hcr_el2, {tmp}",
            "msr DAIFClr, #1",
            "isb",
            tmp = out(reg) _,
            options(nostack, nomem),
        );
    }
    sp
}

/// SPMC event loop — dispatches FF-A requests from SPMD (EL3) forever.
///
/// `first_request` is the SmcResult8 returned by the initial FFA_MSG_WAIT
/// SMC (sent during SPMC boot). Each iteration dispatches the request,
/// sends the response back to SPMD via forward_smc8(), and receives the
/// next request in the return value.
#[cfg(feature = "sel2")]
pub fn run_event_loop(first_request: SmcResult8) -> ! {
    let mut request = first_request;
    loop {
        crate::log_debug!("[SPMC] req x0={:#x} x1={:#x}\n", request.x0, request.x1);
        let response = dispatch_request(&request);
        crate::log_debug!("[SPMC] rsp x0={:#x} x1={:#x}\n", response.x0, response.x1);

        // Disable Secure Group 1 interrupt delivery before SMC to SPMD.
        unsafe {
            core::arch::asm!(
                "msr ICC_IGRPEN1_EL1, xzr",
                "isb",
                options(nostack, nomem),
            );
        }

        request = crate::ffa::smc_forward::forward_smc8(
            response.x0,
            response.x1,
            response.x2,
            response.x3,
            response.x4,
            response.x5,
            response.x6,
            response.x7,
        );

        // Re-enable Secure Group 1 for IRQ handling during SP execution
        unsafe {
            core::arch::asm!(
                "mov x0, #1",
                "msr ICC_IGRPEN1_EL1, x0",
                "isb",
                out("x0") _,
                options(nostack, nomem),
            );
        }
    }
}

/// Dispatch an FF-A request. Routes to SP or local SPMC handling.
#[cfg(feature = "sel2")]
fn dispatch_request(req: &SmcResult8) -> SmcResult8 {
    if req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_32
        || req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_64
    {
        let dest = (req.x1 & 0xFFFF) as u16;
        if crate::sp_context::is_registered_sp(dest) {
            return dispatch_to_sp(req, dest);
        }
    }
    // FFA_RUN: resume a preempted SP
    if req.x0 == ffa::FFA_RUN {
        let sp_id = ((req.x1 >> 16) & 0xFFFF) as u16;
        return resume_preempted_sp(sp_id);
    }
    dispatch_ffa(req)
}

/// Route DIRECT_REQ to an SP: ERET, wait for response, return it.
///
/// Arms the CNTHP preemption timer before SP entry. After enter_guest()
/// returns, checks SP_IRQ_PREEMPTED to determine if the SP was preempted
/// by a physical IRQ (returns FFA_INTERRUPT) or completed normally
/// (returns DIRECT_RESP).
///
/// Before each ERET, injects any pending virtual interrupt via GIC List Register.
#[cfg(feature = "sel2")]
fn dispatch_to_sp(req: &SmcResult8, sp_id: u16) -> SmcResult8 {
    let sp = match crate::sp_context::get_sp_mut(sp_id) {
        Some(sp) => sp,
        None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if sp.state() != crate::sp_context::SpState::Idle {
        return make_error(ffa::FFA_BUSY as u64);
    }

    // Set up SP registers with the DIRECT_REQ args
    sp.set_args(req.x0, req.x1, req.x2, req.x3, req.x4, req.x5, req.x6, req.x7);
    sp.transition_to(crate::sp_context::SpState::Running)
        .expect("SP Running transition failed");

    // Clear preemption flag before SP entry
    SP_IRQ_PREEMPTED.store(false, Ordering::Release);

    // Inject any pending virtual interrupt before entry
    inject_pending_virq(sp);

    // Install SP's Secure Stage-2 (VSTTBR_EL2/VSTCR_EL2)
    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();

    // Track which SP is running so exception/IRQ handler can route correctly
    CURRENT_RUNNING_SP.store(sp_id, Ordering::Release);

    // Restore SP's EL1 sysregs (SCTLR_EL1, VBAR_EL1, etc.)
    sp.restore_el1_state();

    // Arm CNTHP poll timer so owned INTIDs get injected during slow-path
    crate::arch::aarch64::peripherals::timer::arm_preemption_timer();

    pre_enter_guest(sp);

    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(
            sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
        )
    };

    let sp = post_enter_guest();

    sp.save_el1_state();

    CURRENT_RUNNING_SP.store(0, Ordering::Release);
    crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();

    // Handle SP exit — may loop if SP calls FF-A memory operations
    let result = handle_sp_exit(sp, sp_id);

    // Clear Secure Stage-2 and HCR_EL2 bits before returning to SPMD.
    clear_secure_stage2();

    result
}

/// Clear Secure Stage-2 state after SP execution.
///
/// Zeroes VSTTBR_EL2 and clears HCR_EL2.{VM,VI,VF} so SPMC runs without
/// Clear Secure Stage-2 state after SP execution.
///
/// Zeroes VSTTBR_EL2/VSTCR_EL2 and clears HCR_EL2.{VM,VI,VF}.
/// Preserves TSC, FMO, IMO, AMO, RW and other trap bits that must remain
/// set for the SPMC event loop (TSC is critical: without it SP's SMC
/// bypasses S-EL2 and goes directly to EL3).
#[cfg(feature = "sel2")]
fn clear_secure_stage2() {
    unsafe {
        // Clear Secure Stage-2 translation registers
        core::arch::asm!(
            "msr s3_4_c2_c6_0, xzr", // VSTTBR_EL2 = 0
            "msr s3_4_c2_c6_2, xzr", // VSTCR_EL2 = 0
            "isb",
            options(nostack, nomem),
        );

        // Clear only VM, VI, VF bits from HCR_EL2 — leave TSC, FMO, IMO,
        // AMO, RW, and other trap bits intact. SPMD saves HCR_EL2 when we
        // call forward_smc8; without TSC, SP's next SMC bypasses S-EL2.
        core::arch::asm!(
            "mrs {tmp}, hcr_el2",
            "bic {tmp}, {tmp}, #(1 << 0)",  // clear VM  (bit 0)
            "bic {tmp}, {tmp}, #(1 << 6)",  // clear VF  (bit 6)
            "bic {tmp}, {tmp}, #(1 << 7)",  // clear VI  (bit 7)
            "msr hcr_el2, {tmp}",
            "isb",
            tmp = out(reg) _,
            options(nostack, nomem),
        );
    }
}

/// Handle SP exit after enter_guest() returns.
///
/// If the SP called an FF-A memory operation (MEM_RETRIEVE_REQ, MEM_RELINQUISH),
/// handle it locally and re-enter the SP. Otherwise, return the SP's response
/// (DIRECT_RESP, FFA_MSG_WAIT, etc.) to the caller.
///
/// Used by both `dispatch_to_sp()` and `resume_preempted_sp()` to avoid duplication.
#[cfg(feature = "sel2")]
fn handle_sp_exit(
    sp: &mut crate::sp_context::SpContext,
    sp_id: u16,
) -> SmcResult8 {
    loop {
        // Check if SP was preempted by a physical IRQ (NS FIQ at S-EL2)
        if SP_IRQ_PREEMPTED.swap(false, Ordering::Acquire) {
            crate::log_debug!("[SPMC] preempted sp={:#06x} fiq_count={}\n",
                sp_id, FIQ_PREEMPT_COUNT.load(Ordering::Relaxed));
            sp.transition_to(crate::sp_context::SpState::Preempted)
                .expect("SP Preempted transition failed");

            // Check if another SP has a pending interrupt (cross-SP preemption)
            if let Some(target_id) = crate::sp_context::find_sp_with_pending_irq() {
                if target_id != sp_id {
                    dispatch_interrupt_to_sp(target_id);
                }
            }

            return SmcResult8 {
                x0: ffa::FFA_INTERRUPT,
                x1: (sp_id as u64) << 16, // target SP ID for FFA_RUN resume
                x2: 0,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            };
        }

        // SP exited normally — check what it called
        let (x0, x1, x2, x3, x4, x5, x6, x7) = sp.get_args();

        match x0 {
            ffa::FFA_MEM_RETRIEVE_REQ_32 | ffa::FFA_MEM_RETRIEVE_REQ_64 => {
                // SP-initiated MEM_RETRIEVE: build request, call handler, re-enter SP
                let sp_req = SmcResult8 { x0, x1, x2, x3, x4, x5, x6, x7 };
                let result = handle_spmc_mem_retrieve(&sp_req);
                sp.set_args(
                    result.x0, result.x1, result.x2, result.x3,
                    result.x4, result.x5, result.x6, result.x7,
                );
            }
            ffa::FFA_MEM_RELINQUISH => {
                // SP-initiated MEM_RELINQUISH: build request, call handler, re-enter SP
                let sp_req = SmcResult8 { x0, x1, x2, x3, x4, x5, x6, x7 };
                let result = handle_spmc_mem_relinquish(&sp_req);
                sp.set_args(
                    result.x0, result.x1, result.x2, result.x3,
                    result.x4, result.x5, result.x6, result.x7,
                );
            }
            _ => {
                // Normal exit (FFA_DIRECT_RESP, FFA_MSG_WAIT, etc.)
                sp.transition_to(crate::sp_context::SpState::Idle)
                    .expect("SP Idle transition failed");
                return SmcResult8 { x0, x1, x2, x3, x4, x5, x6, x7 };
            }
        }

        // Re-enter SP with the handler's result (Running→Idle→Running)
        sp.transition_to(crate::sp_context::SpState::Idle)
            .expect("SP Idle transition failed (re-enter)");
        sp.transition_to(crate::sp_context::SpState::Running)
            .expect("SP Running transition failed (re-enter)");

        SP_IRQ_PREEMPTED.store(false, Ordering::Release);
        inject_pending_virq(sp);

        let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
        s2.install();
        CURRENT_RUNNING_SP.store(sp_id, Ordering::Release);

        sp.restore_el1_state();
        crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
        pre_enter_guest(sp);

        let _exit = unsafe {
            crate::arch::aarch64::enter_guest(
                sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
            )
        };

        let sp = post_enter_guest();
        sp.save_el1_state();

        CURRENT_RUNNING_SP.store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
    }
}

/// Resume a preempted SP via FFA_RUN. Returns FFA_INTERRUPT if preempted
/// again, or the SP's DIRECT_RESP when it completes.
///
/// Injects any pending virtual interrupt via GIC LR before resuming.
#[cfg(feature = "sel2")]
fn resume_preempted_sp(sp_id: u16) -> SmcResult8 {
    crate::log_debug!("[SPMC] resume sp={:#06x}\n", sp_id);
    let sp = match crate::sp_context::get_sp_mut(sp_id) {
        Some(sp) => sp,
        None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if sp.state() != crate::sp_context::SpState::Preempted {
        return make_error(ffa::FFA_DENIED as u64);
    }

    sp.transition_to(crate::sp_context::SpState::Running)
        .expect("SP Running transition failed");

    SP_IRQ_PREEMPTED.store(false, Ordering::Release);

    inject_pending_virq(sp);

    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();
    CURRENT_RUNNING_SP.store(sp_id, Ordering::Release);

    sp.restore_el1_state();
    crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
    pre_enter_guest(sp);

    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(
            sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
        )
    };

    let sp = post_enter_guest();
    sp.save_el1_state();

    CURRENT_RUNNING_SP.store(0, Ordering::Release);
    crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();

    let result = handle_sp_exit(sp, sp_id);
    clear_secure_stage2();
    result
}

/// Set HCR_EL2.VI if the SP has a pending virtual interrupt.
///
/// Called before enter_guest() during FFA_RUN resume or cross-SP dispatch.
/// Uses the Hafnium-compatible HCR_EL2.VI mechanism: setting VI causes
/// hardware to auto-vector to VBAR_EL1+0x280 on ERET. The SP then calls
/// HVC (HF_INTERRUPT_GET) to retrieve the INTID.
///
/// Note: Does NOT consume the pending_irq — that happens when the SP
/// calls HF_INTERRUPT_GET via HVC (handled in exception.rs).
#[cfg(feature = "sel2")]
fn inject_pending_virq(sp: &mut crate::sp_context::SpContext) {
    if sp.has_pending_irq() {
        // Set HCR_EL2.VI → hardware auto-vectors to IRQ handler on ERET
        unsafe {
            let mut hcr: u64;
            core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr, options(nostack, nomem));
            hcr |= 1 << 7; // HCR_EL2.VI
            core::arch::asm!("msr hcr_el2, {}", "isb", in(reg) hcr, options(nostack, nomem));
        }
    }
}

/// Dispatch a pending interrupt to an SP that is currently Idle.
///
/// Transitions the SP: Idle → Running, injects vIRQ via LR, enters guest.
/// The SP's IRQ handler fires, processes the interrupt, and traps back
/// (e.g., via FFA_MSG_WAIT). SPMC transitions SP back to Idle.
///
/// Used for cross-SP preemption: SP1 running + SP2's interrupt fires →
/// preempt SP1 → dispatch interrupt to SP2 → SP2 returns → resume SP1.
#[cfg(feature = "sel2")]
fn dispatch_interrupt_to_sp(sp_id: u16) {
    let sp = match crate::sp_context::get_sp_mut(sp_id) {
        Some(sp) => sp,
        None => return,
    };

    // Only dispatch to Idle SPs
    if sp.state() != crate::sp_context::SpState::Idle {
        return;
    }

    sp.transition_to(crate::sp_context::SpState::Running)
        .expect("SP Running transition failed");

    inject_pending_virq(sp);

    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();

    sp.restore_el1_state();
    crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
    pre_enter_guest(sp);

    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(
            sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
        )
    };

    let sp = post_enter_guest();
    sp.save_el1_state();

    crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
    sp.transition_to(crate::sp_context::SpState::Idle)
        .expect("SP Idle transition failed");
}

/// Dispatch an FF-A request and return the appropriate response.
///
/// Pure function: matches on the FF-A function ID in req.x0 and builds
/// a response SmcResult8. Not gated by feature flags so it can be unit
/// tested on the host.
pub fn dispatch_ffa(req: &SmcResult8) -> SmcResult8 {
    match req.x0 {
        ffa::FFA_VERSION => {
            // Return FF-A v1.1
            SmcResult8 {
                x0: ffa::FFA_VERSION_1_1 as u64,
                x1: 0,
                x2: 0,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_ID_GET => {
            // SPMC partition ID = 0x8000
            SmcResult8 {
                x0: ffa::FFA_SUCCESS_32,
                x1: 0,
                x2: ffa::FFA_SPMC_ID as u64,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_SPM_ID_GET => {
            // SPMC partition ID = 0x8000
            SmcResult8 {
                x0: ffa::FFA_SUCCESS_32,
                x1: 0,
                x2: ffa::FFA_SPMC_ID as u64,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_FEATURES => {
            // Check if the queried function ID (in x1) is supported
            let queried_fid = req.x1;
            // RXTX_MAP is listed because SPMD forwards it from NWd to SPMC.
            let supported = matches!(
                queried_fid,
                ffa::FFA_VERSION
                    | ffa::FFA_ID_GET
                    | ffa::FFA_FEATURES
                    | ffa::FFA_SPM_ID_GET
                    | ffa::FFA_PARTITION_INFO_GET
                    | ffa::FFA_MSG_SEND_DIRECT_REQ_32
                    | ffa::FFA_MSG_SEND_DIRECT_REQ_64
                    | ffa::FFA_RXTX_MAP
                    | ffa::FFA_RX_RELEASE
                    | ffa::FFA_RUN
                    | ffa::FFA_MEM_SHARE_32
                    | ffa::FFA_MEM_LEND_32
                    | ffa::FFA_MEM_RETRIEVE_REQ_32
                    | ffa::FFA_MEM_RELINQUISH
                    | ffa::FFA_MEM_RECLAIM
            );
            if supported {
                SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: 0,
                    x3: 0,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                }
            } else {
                make_error(ffa::FFA_NOT_SUPPORTED as u64)
            }
        }

        ffa::FFA_RUN => {
            // FFA_RUN: x1[31:16] = target SP ID
            let sp_id = ((req.x1 >> 16) & 0xFFFF) as u16;
            if !crate::sp_context::is_registered_sp(sp_id) {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }
            let sp = crate::sp_context::get_sp_mut(sp_id).unwrap();
            if sp.state() != crate::sp_context::SpState::Preempted {
                return make_error(ffa::FFA_DENIED as u64);
            }
            // In sel2 mode, dispatch_request() handles this before we get here.
            // In unit tests (no sel2), just validate the state.
            make_error(ffa::FFA_NOT_SUPPORTED as u64)
        }

        ffa::FFA_RXTX_MAP => handle_rxtx_map(req),
        ffa::FFA_RXTX_UNMAP => handle_rxtx_unmap(),
        ffa::FFA_RX_RELEASE => handle_rx_release(),

        ffa::FFA_PARTITION_INFO_GET => {
            handle_partition_info_get()
        }

        ffa::FFA_MSG_SEND_DIRECT_REQ_32 => {
            handle_direct_req_32(req)
        }

        ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
            // Echo x3-x7 back, swap source/dest in x1
            let source = (req.x1 >> 16) & 0xFFFF;
            let dest = req.x1 & 0xFFFF;
            SmcResult8 {
                x0: ffa::FFA_MSG_SEND_DIRECT_RESP_64,
                x1: (dest << 16) | source,
                x2: 0,
                x3: req.x3,
                x4: req.x4,
                x5: req.x5,
                x6: req.x6,
                x7: req.x7,
            }
        }

        ffa::FFA_MEM_SHARE_32 | ffa::FFA_MEM_SHARE_64 => handle_spmc_mem_share(req, false),
        ffa::FFA_MEM_LEND_32 | ffa::FFA_MEM_LEND_64 => handle_spmc_mem_share(req, true),
        ffa::FFA_MEM_RETRIEVE_REQ_32 | ffa::FFA_MEM_RETRIEVE_REQ_64 => handle_spmc_mem_retrieve(req),
        ffa::FFA_MEM_RELINQUISH => handle_spmc_mem_relinquish(req),
        ffa::FFA_MEM_RECLAIM => handle_spmc_mem_reclaim(req),
        ffa::FFA_MEM_DONATE_32 | ffa::FFA_MEM_DONATE_64 => make_error(ffa::FFA_NOT_SUPPORTED as u64),

        _ => make_error(ffa::FFA_NOT_SUPPORTED as u64),
    }
}

/// Handle FFA_RXTX_MAP — store NWd's TX/RX buffer PAs.
///
/// SPMD at EL3 forwards this from NWd to SPMC. We store the PAs for later
/// use by PARTITION_INFO_GET (which writes descriptors directly to NWd's RX).
fn handle_rxtx_map(req: &SmcResult8) -> SmcResult8 {
    let tx_pa = req.x1;
    let rx_pa = req.x2;
    let page_count = req.x3 as u32;

    // Validate alignment
    if tx_pa & 0xFFF != 0 || rx_pa & 0xFFF != 0 || page_count == 0 {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    unsafe {
        if NWD_RXTX.mapped {
            return make_error(ffa::FFA_DENIED as u64);
        }
        NWD_RXTX.tx_pa = tx_pa;
        NWD_RXTX.rx_pa = rx_pa;
        NWD_RXTX.page_count = page_count;
        NWD_RXTX.mapped = true;
    }

    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: 0,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle FFA_RXTX_UNMAP — clear NWd's RXTX registration.
fn handle_rxtx_unmap() -> SmcResult8 {
    unsafe {
        if !NWD_RXTX.mapped {
            return make_error(ffa::FFA_DENIED as u64);
        }
        NWD_RXTX.tx_pa = 0;
        NWD_RXTX.rx_pa = 0;
        NWD_RXTX.page_count = 0;
        NWD_RXTX.mapped = false;
    }

    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: 0,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle FFA_RX_RELEASE — acknowledge NWd has consumed the RX buffer.
fn handle_rx_release() -> SmcResult8 {
    unsafe {
        if !NWD_RXTX.mapped {
            return make_error(ffa::FFA_DENIED as u64);
        }
    }
    // No-op: we write descriptors synchronously in PARTITION_INFO_GET.
    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: 0,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle PARTITION_INFO_GET — writes 24-byte descriptors to NWd's RX buffer.
///
/// FF-A v1.1 partition info descriptor (DEN0077A Table 5.37):
///   Offset 0:  partition_id    (u16 LE)
///   Offset 2:  exec_ctx_count  (u16 LE)
///   Offset 4:  properties      (u32 LE)
///   Offset 8:  uuid[16]        (128-bit UUID)
///
/// If NWd has registered RXTX, writes descriptors to NWd's RX PA.
/// If no RXTX registered, returns count only (FF-A "count query" mode).
fn handle_partition_info_get() -> SmcResult8 {
    let mut count = 0u64;

    // Write descriptors to NWd's RX buffer (sel2 mode) or just count (unit tests).
    #[cfg(feature = "sel2")]
    {
        let mapped = unsafe { NWD_RXTX.mapped };
        let rx_pa = unsafe { NWD_RXTX.rx_pa };
        let max_bytes = if mapped {
            unsafe { NWD_RXTX.page_count as usize * 4096 }
        } else {
            0
        };

        crate::sp_context::for_each_sp(|sp| {
            let offset = count as usize * 24;
            if mapped && offset + 24 <= max_bytes {
                unsafe {
                    let ptr = (rx_pa as *mut u8).add(offset);
                    // partition_id (u16 LE)
                    core::ptr::write_unaligned(ptr as *mut u16, sp.sp_id());
                    // exec_ctx_count (u16 LE)
                    core::ptr::write_unaligned(ptr.add(2) as *mut u16, 1);
                    // properties (u32 LE) — bit 0: supports DIRECT_REQ
                    core::ptr::write_unaligned(ptr.add(4) as *mut u32, 0x1);
                    // UUID (16 bytes) — read from SpContext
                    core::ptr::copy_nonoverlapping(
                        sp.uuid().as_ptr() as *const u8,
                        ptr.add(8),
                        16,
                    );
                }
            }
            count += 1;
        });
    }

    // In non-sel2 mode (unit tests), just count registered SPs
    #[cfg(not(feature = "sel2"))]
    {
        crate::sp_context::for_each_sp(|_| {
            count += 1;
        });
    }

    // FF-A v1.1: x3 = size of each partition info descriptor (24 bytes).
    // pKVM reads this to calculate copy_sz = partition_sz * count.
    // Without this, pKVM copies 0 bytes and the guest sees all zeros.
    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: count,
        x3: 24, // sizeof(ffa_partition_info) = 24 bytes per FF-A v1.1 Table 5.37
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle DIRECT_REQ_32 — checks for SPMD framework messages first.
///
/// SPMD wraps certain FF-A calls (e.g. FFA_VERSION) as framework messages
/// inside DIRECT_REQ with FFA_FWK_MSG_BIT set in x2. We must detect and
/// respond to these before falling through to the normal echo handler.
fn handle_direct_req_32(req: &SmcResult8) -> SmcResult8 {
    // Check for SPMD framework message (FFA_FWK_MSG_BIT set in x2)
    if (req.x2 & ffa::FFA_FWK_MSG_BIT) != 0 {
        let fwk_func = req.x2 & !ffa::FFA_FWK_MSG_BIT;
        // Swap source/dest from the request so SPMD recognizes us.
        // SPMD sends x1 = (SPMD_EP_ID << 16) | SPMC_ID.
        // We must respond with x1 = (SPMC_ID << 16) | SPMD_EP_ID.
        let source = (req.x1 >> 16) & 0xFFFF;
        let dest = req.x1 & 0xFFFF;
        let swapped_x1 = (dest << 16) | source;
        if fwk_func == ffa::SPMD_FWK_MSG_FFA_VERSION_REQ {
            // SPMD forwarding NWd's FFA_VERSION. x3 = requested version.
            return SmcResult8 {
                x0: ffa::FFA_MSG_SEND_DIRECT_RESP_32,
                x1: swapped_x1,
                x2: ffa::FFA_FWK_MSG_BIT | ffa::SPMD_FWK_MSG_FFA_VERSION_RESP,
                x3: ffa::FFA_VERSION_1_1 as u64,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            };
        }
        // Unknown framework message
        return make_error(ffa::FFA_NOT_SUPPORTED as u64);
    }

    // Normal direct request: echo x3-x7, swap source/dest in x1
    let source = (req.x1 >> 16) & 0xFFFF;
    let dest = req.x1 & 0xFFFF;
    SmcResult8 {
        x0: ffa::FFA_MSG_SEND_DIRECT_RESP_32,
        x1: (dest << 16) | source,
        x2: 0,
        x3: req.x3,
        x4: req.x4,
        x5: req.x5,
        x6: req.x6,
        x7: req.x7,
    }
}

/// Handle FFA_MEM_SHARE / FFA_MEM_LEND from NWd.
///
/// In sel2 mode: reads FF-A v1.1 composite memory region descriptor from NWd TX buffer.
/// In unit tests (no sel2): uses register-based protocol (x3=IPA, x4=count, x5=receiver).
fn handle_spmc_mem_share(req: &SmcResult8, is_lend: bool) -> SmcResult8 {
    let sender_id: u16;
    let receiver_id: u16;
    let ranges: [(u64, u32); 1];

    #[cfg(feature = "sel2")]
    {
        let mapped = unsafe { NWD_RXTX.mapped };
        if mapped {
            let tx_pa = unsafe { NWD_RXTX.tx_pa };
            let total_length = req.x1 as usize;
            let parsed = unsafe {
                crate::ffa::descriptors::parse_mem_region(tx_pa as *const u8, total_length as u32)
            };
            match parsed {
                Ok(desc) => {
                    sender_id = desc.sender_id;
                    receiver_id = desc.receiver_id;
                    if desc.range_count == 0 {
                        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
                    }
                    ranges = [(desc.ranges[0].0, desc.ranges[0].1)];
                }
                Err(_) => {
                    return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
                }
            }
        } else {
            // Fallback: register-based protocol
            sender_id = ((req.x1 >> 16) & 0xFFFF) as u16;
            receiver_id = (req.x5 & 0xFFFF) as u16;
            let ipa = req.x3;
            let count = req.x4 as u32;
            ranges = [(ipa, count)];
        }
    }

    #[cfg(not(feature = "sel2"))]
    {
        // Register-based protocol for unit tests
        sender_id = ((req.x1 >> 16) & 0xFFFF) as u16;
        receiver_id = (req.x5 & 0xFFFF) as u16;
        let ipa = req.x3;
        let count = req.x4 as u32;
        ranges = [(ipa, count)];
    }

    // Validate receiver is a registered SP
    if !crate::sp_context::is_registered_sp(receiver_id) {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    match record_spmc_share(sender_id, receiver_id, &ranges, is_lend) {
        Some(handle) => SmcResult8 {
            x0: ffa::FFA_SUCCESS_32,
            x1: 0,
            x2: handle & 0xFFFF_FFFF,
            x3: handle >> 32,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        },
        None => make_error(ffa::FFA_NO_MEMORY as u64),
    }
}

/// Handle FFA_MEM_RETRIEVE_REQ — maps pages into receiver SP's Secure Stage-2.
fn handle_spmc_mem_retrieve(req: &SmcResult8) -> SmcResult8 {
    let handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);

    let (_sender, receiver_id, ranges, range_count, _, retrieved) = match lookup_spmc_share(handle) {
        Some(info) => info,
        None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if retrieved {
        return make_error(ffa::FFA_DENIED as u64);
    }

    // In sel2 mode, map pages into the receiver SP's Secure Stage-2
    #[cfg(feature = "sel2")]
    {
        if let Some(sp) = crate::sp_context::get_sp_mut(receiver_id) {
            let vsttbr = sp.vsttbr();
            let l0_addr = vsttbr & 0x0000_FFFF_FFFF_F000;
            let walker = crate::ffa::stage2_walker::Stage2Walker::new(l0_addr);
            for i in 0..range_count {
                let (base_ipa, page_count) = ranges[i];
                for p in 0..page_count as u64 {
                    let ipa = base_ipa + p * 4096;
                    walker.map_page(ipa, 0b11, 0b10); // S2AP_RW, SW=SHARED_BORROWED
                }
            }
        }
    }
    let _ = (receiver_id, &ranges, range_count);

    mark_spmc_retrieved(handle);

    SmcResult8 {
        x0: ffa::FFA_MEM_RETRIEVE_RESP,
        x1: 0,
        x2: handle & 0xFFFF_FFFF,
        x3: handle >> 32,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle FFA_MEM_RELINQUISH — unmaps pages from receiver SP's Secure Stage-2.
fn handle_spmc_mem_relinquish(req: &SmcResult8) -> SmcResult8 {
    let handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);

    let (_sender, receiver_id, ranges, range_count, _, retrieved) = match lookup_spmc_share(handle) {
        Some(info) => info,
        None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if !retrieved {
        return make_error(ffa::FFA_DENIED as u64);
    }

    // In sel2 mode, unmap pages from the receiver SP's Secure Stage-2
    #[cfg(feature = "sel2")]
    {
        if let Some(sp) = crate::sp_context::get_sp_mut(receiver_id) {
            let vsttbr = sp.vsttbr();
            let l0_addr = vsttbr & 0x0000_FFFF_FFFF_F000;
            let walker = crate::ffa::stage2_walker::Stage2Walker::new(l0_addr);
            for i in 0..range_count {
                let (base_ipa, page_count) = ranges[i];
                for p in 0..page_count as u64 {
                    let ipa = base_ipa + p * 4096;
                    walker.unmap_page(ipa);
                }
            }
        }
    }
    let _ = (receiver_id, &ranges, range_count);

    mark_spmc_relinquished(handle);

    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: 0,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle FFA_MEM_RECLAIM — delete share record (must not be retrieved).
fn handle_spmc_mem_reclaim(req: &SmcResult8) -> SmcResult8 {
    let handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);

    match reclaim_spmc_share(handle) {
        Ok(()) => SmcResult8 {
            x0: ffa::FFA_SUCCESS_32,
            x1: 0,
            x2: 0,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        },
        Err(code) => make_error(code as u64),
    }
}

/// Build an FFA_ERROR response with the given error code in x2.
fn make_error(error_code: u64) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_ERROR,
        x1: 0,
        x2: error_code,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}
