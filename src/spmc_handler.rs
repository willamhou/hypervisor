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

use crate::arch::aarch64::defs::PAGE_SIZE_4KB;
use crate::ffa;
use crate::ffa::smc_forward::SmcResult8;
use crate::sync::SpinLock;
#[cfg(feature = "sel2")]
use core::cell::UnsafeCell;
#[cfg(feature = "sel2")]
use core::sync::atomic::Ordering;
use core::sync::atomic::{AtomicBool, AtomicU64};

// ── Per-CPU SPMC state ──────────────────────────────────────────────────
//
// With pKVM at NS-EL2, SPMD is per-CPU: each physical CPU can independently
// enter S-EL2 via SMC. These globals MUST be per-CPU arrays indexed by
// MPIDR_EL1.Aff0 to prevent concurrent CPUs from corrupting each other's
// saved state.

use crate::platform::MAX_SMP_CPUS;

// ── SP-to-SP Call Stack ────────────────────────────────────────────────

/// A frame in the SP-to-SP call stack, tracking who called whom.
pub struct CallFrame {
    pub caller_id: u16,
    pub callee_id: u16,
}

/// Maximum SP-to-SP call chain depth (MAX_SPS - 1; one SP must be innermost callee).
const MAX_CALL_DEPTH: usize = 3;

/// Global call stack for tracking SP-to-SP DIRECT_REQ nesting.
/// Maximum depth is MAX_CALL_DEPTH (= MAX_SPS - 1).
pub struct CallStack {
    frames: [Option<CallFrame>; MAX_CALL_DEPTH],
    depth: usize,
}

impl CallStack {
    pub const fn new() -> Self {
        Self {
            frames: [None, None, None], // MAX_SPS - 1 = 3
            depth: 0,
        }
    }

    /// Push a new call frame. Returns Err if stack is full.
    pub fn push(&mut self, caller: u16, callee: u16) -> Result<(), ()> {
        if self.depth >= self.frames.len() {
            return Err(());
        }
        self.frames[self.depth] = Some(CallFrame {
            caller_id: caller,
            callee_id: callee,
        });
        self.depth += 1;
        Ok(())
    }

    /// Pop the top frame. Returns None if stack is empty.
    pub fn pop(&mut self) -> Option<CallFrame> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        self.frames[self.depth].take()
    }

    /// Check if sp_id appears as caller or callee anywhere in the stack.
    /// Used for cycle detection.
    pub fn contains(&self, sp_id: u16) -> bool {
        self.frames[..self.depth].iter().any(|f| {
            if let Some(frame) = f {
                frame.caller_id == sp_id || frame.callee_id == sp_id
            } else {
                false
            }
        })
    }

    /// Current nesting depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Find the caller that is waiting for a given callee.
    /// Used by resume_preempted_sp() for chain-resume.
    pub fn find_caller(&self, callee_id: u16) -> Option<u16> {
        self.frames[..self.depth].iter().find_map(|f| {
            if let Some(frame) = f {
                if frame.callee_id == callee_id {
                    return Some(frame.caller_id);
                }
            }
            None
        })
    }
}

/// Global SP-to-SP call stack, protected by SpinLock.
/// Lock ordering: CALL_STACK → SP_STORE_LOCK (never reverse).
pub static CALL_STACK: SpinLock<CallStack> = SpinLock::new(CallStack::new());

/// Internal sentinel: SP issued DIRECT_REQ targeting another SP.
/// dispatch_to_sp() must handle the recursive dispatch.
const SP_TO_SP_PENDING: u64 = 0xFFFF_FFFF_FFFF_FFFE;

/// Helper: read current CPU index (MPIDR_EL1.Aff0). Works at both NS-EL2 and S-EL2.
#[cfg(feature = "sel2")]
#[inline(always)]
fn sel2_cpu_id() -> usize {
    let mpidr: u64;
    // SAFETY: reading MPIDR_EL1 is side-effect free and valid at EL2.
    unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr, options(nostack, nomem)) };
    (mpidr & 0xFF) as usize
}

/// Per-CPU flag set by the IRQ handler when a physical IRQ preempts SP execution.
/// The SPMC event loop checks this after enter_guest() returns to decide
/// whether to return FFA_INTERRUPT (preempted) or DIRECT_RESP (completed).
static SP_IRQ_PREEMPTED: [AtomicBool; MAX_SMP_CPUS] = {
    const INIT: AtomicBool = AtomicBool::new(false);
    [INIT; MAX_SMP_CPUS]
};

/// Per-CPU debug counter: count FIQ preemptions to detect infinite loops.
#[cfg(feature = "sel2")]
static FIQ_PREEMPT_COUNT: [core::sync::atomic::AtomicU32; MAX_SMP_CPUS] = {
    const INIT: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    [INIT; MAX_SMP_CPUS]
};

/// Per-CPU saved host ELR_EL2/SPSR_EL2 across enter_guest() calls.
/// Using per-CPU globals because these hardware registers get clobbered
/// by the exception handler and must be restored after enter_guest returns.
#[cfg(feature = "sel2")]
static SAVED_HOST_ELR: [AtomicU64; MAX_SMP_CPUS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
    [INIT; MAX_SMP_CPUS]
};
#[cfg(feature = "sel2")]
static SAVED_HOST_SPSR: [AtomicU64; MAX_SMP_CPUS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
    [INIT; MAX_SMP_CPUS]
};

/// Per-CPU tracking of which SP is executing at S-EL1. Set before enter_guest(),
/// cleared after return. Used by the IRQ handler to inject virtual interrupts
/// directly via LR without going through SPMD.
#[cfg(feature = "sel2")]
static CURRENT_RUNNING_SP: [core::sync::atomic::AtomicU16; MAX_SMP_CPUS] = {
    const INIT: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(0);
    [INIT; MAX_SMP_CPUS]
};

/// Per-CPU saved SPMC host EL1 state. When we enter an SP, we overwrite
/// EL1 sysregs with the SP's state. We must save the host's EL1 state
/// before and restore it after, because SPMD doesn't save/restore EL1
/// for the SPMC (only EL2). Without this, the NWd's EL1 state gets
/// corrupted on return to pKVM.
#[cfg(feature = "sel2")]
struct PerCpuEl1State([UnsafeCell<crate::sp_context::SpEl1State>; MAX_SMP_CPUS]);

// Safety: each CPU only accesses its own index, no concurrent access.
#[cfg(feature = "sel2")]
unsafe impl Sync for PerCpuEl1State {}

#[cfg(feature = "sel2")]
static HOST_EL1_STATE: PerCpuEl1State = {
    const INIT: UnsafeCell<crate::sp_context::SpEl1State> =
        UnsafeCell::new(crate::sp_context::SpEl1State::new());
    PerCpuEl1State([INIT; MAX_SMP_CPUS])
};

/// Get the currently running SP ID on this CPU (0 if none).
#[cfg(feature = "sel2")]
pub fn current_running_sp() -> u16 {
    CURRENT_RUNNING_SP[sel2_cpu_id()].load(Ordering::Acquire)
}

/// Set the SP_IRQ_PREEMPTED flag for the current CPU.
pub fn set_sp_irq_preempted(val: bool) {
    let cpu = {
        let mpidr: u64;
        // SAFETY: reading MPIDR_EL1 is side-effect free and valid at EL2.
        unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr, options(nostack, nomem)) };
        (mpidr & 0xFF) as usize
    };
    SP_IRQ_PREEMPTED[cpu].store(val, core::sync::atomic::Ordering::Release);
}

/// Increment the FIQ preempt count for the current CPU.
pub fn inc_fiq_preempt_count() {
    let cpu = {
        let mpidr: u64;
        // SAFETY: reading MPIDR_EL1 is side-effect free and valid at EL2.
        unsafe { core::arch::asm!("mrs {}, MPIDR_EL1", out(reg) mpidr, options(nostack, nomem)) };
        (mpidr & 0xFF) as usize
    };
    #[cfg(feature = "sel2")]
    FIQ_PREEMPT_COUNT[cpu].fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    #[cfg(not(feature = "sel2"))]
    let _ = cpu;
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

static NWD_RXTX: SpinLock<NwdRxtxState> = SpinLock::new(NwdRxtxState {
    tx_pa: 0,
    rx_pa: 0,
    page_count: 0,
    mapped: false,
});

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
    is_donate: bool,
    retrieved: bool,
}

static SPMC_SHARES: SpinLock<[SpmcShareRecord; MAX_SPMC_SHARES]> = SpinLock::new({
    const EMPTY: SpmcShareRecord = SpmcShareRecord {
        handle: 0,
        sender_id: 0,
        receiver_id: 0,
        ranges: [(0, 0); MAX_SHARE_RANGES],
        range_count: 0,
        active: false,
        is_lend: false,
        is_donate: false,
        retrieved: false,
    };
    [
        EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY,
        EMPTY, EMPTY, EMPTY,
    ]
});

static SPMC_NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

/// Global lock for Stage-2 page table modifications (map_page/unmap_page).
/// Prevents TOCTOU races when two CPUs concurrently walk/modify the same
/// SP's page tables (e.g., concurrent MEM_RETRIEVE_REQ via per-CPU SPMD).
#[cfg(feature = "sel2")]
static STAGE2_LOCK: SpinLock<()> = SpinLock::new(());

// ── NWd fragment reassembly state ───────────────────────────────────

/// State for reassembling fragmented NWd memory descriptors.
pub struct NwdFragmentState {
    pub active: bool,
    accum_buf: [u8; 4096],
    pub total_length: u32,
    pub received: u32,
    pub handle: u64,
    is_lend: bool,
    is_donate: bool,
    sender_id: u16, // Track sender to prevent mid-fragment sender switching
}

pub static NWD_FRAG: SpinLock<NwdFragmentState> = SpinLock::new(NwdFragmentState {
    active: false,
    accum_buf: [0u8; 4096],
    total_length: 0,
    received: 0,
    handle: 0,
    is_lend: false,
    is_donate: false,
    sender_id: 0,
});

/// Reset stale fragmentation state. Called when fragment assembly needs cleanup.
pub fn reset_nwd_frag_state() {
    let mut frag = NWD_FRAG.lock();
    frag.active = false;
    frag.total_length = 0;
    frag.received = 0;
    frag.handle = 0;
    frag.sender_id = 0;
}

// ── NWd retrieve response fragmentation state ────────────────────────

/// State for delivering fragmented MEM_RETRIEVE_RESP descriptors to NWd.
struct NwdFragRxState {
    active: bool,
    resp_buf: [u8; 4096],
    total_length: u32,
    delivered: u32,
    handle: u64,
}

static NWD_FRAG_RX: SpinLock<NwdFragRxState> = SpinLock::new(NwdFragRxState {
    active: false,
    resp_buf: [0u8; 4096],
    total_length: 0,
    delivered: 0,
    handle: 0,
});

fn spmc_alloc_handle() -> u64 {
    SPMC_NEXT_HANDLE.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

// ── Per-SP indirect messaging mailboxes ──────────────────────────────

const MAX_SP_MAILBOXES: usize = 4;

/// Per-SP mailbox for indirect messaging (MSG_SEND2 / MSG_WAIT).
/// Pre-allocated — SPs don't need to call RXTX_MAP.
struct SpMailbox {
    rx_buf: [u8; 4096],
    msg_pending: bool,
    msg_sender_id: u16,
}

static SP_MAILBOXES: SpinLock<[SpMailbox; MAX_SP_MAILBOXES]> = SpinLock::new({
    const EMPTY: SpMailbox = SpMailbox {
        rx_buf: [0u8; 4096],
        msg_pending: false,
        msg_sender_id: 0,
    };
    [EMPTY, EMPTY, EMPTY, EMPTY]
});

/// Map SP partition ID to mailbox index. Returns None for unknown SPs.
fn sp_mailbox_index(sp_id: u16) -> Option<usize> {
    if sp_id >= ffa::FFA_SPMC_ID + 1 && sp_id < ffa::FFA_SPMC_ID + 1 + MAX_SP_MAILBOXES as u16 {
        Some((sp_id - ffa::FFA_SPMC_ID - 1) as usize)
    } else {
        None
    }
}

/// Record a memory share. Returns the handle on success.
pub fn record_spmc_share(
    sender_id: u16,
    receiver_id: u16,
    ranges: &[(u64, u32)],
    is_lend: bool,
    is_donate: bool,
) -> Option<u64> {
    if ranges.len() > MAX_SHARE_RANGES {
        return None;
    }
    let handle = spmc_alloc_handle();
    let mut records = SPMC_SHARES.lock();
    for record in records.iter_mut() {
        if !record.active {
            let mut stored = [(0u64, 0u32); MAX_SHARE_RANGES];
            let count = ranges.len();
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
                is_donate,
                retrieved: false,
            };
            return Some(handle);
        }
    }
    None
}

/// Look up a share record by handle (immutable).
fn lookup_spmc_share(
    handle: u64,
) -> Option<(
    u16,
    u16,
    [(u64, u32); MAX_SHARE_RANGES],
    usize,
    bool,
    bool,
    bool,
)> {
    let records = SPMC_SHARES.lock();
    for record in records.iter() {
        if record.active && record.handle == handle {
            return Some((
                record.sender_id,
                record.receiver_id,
                record.ranges,
                record.range_count,
                record.is_lend,
                record.is_donate,
                record.retrieved,
            ));
        }
    }
    None
}

/// Mark a share as retrieved. Returns true if successful.
fn mark_spmc_retrieved(handle: u64) -> bool {
    let mut records = SPMC_SHARES.lock();
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
    let mut records = SPMC_SHARES.lock();
    for record in records.iter_mut() {
        if record.active && record.handle == handle && record.retrieved {
            record.retrieved = false;
            return true;
        }
    }
    false
}

/// Reclaim (delete) a share record. Fails if still retrieved or if donated.
fn reclaim_spmc_share(handle: u64) -> Result<(), i32> {
    let mut records = SPMC_SHARES.lock();
    for record in records.iter_mut() {
        if record.active && record.handle == handle {
            if record.is_donate {
                return Err(ffa::FFA_DENIED);
            }
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
    // SAFETY: EL2 sysregs are readable in this context and accesses are local CPU state.
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
    // SAFETY: restores previously saved EL2 host state for the current CPU.
    unsafe {
        core::arch::asm!("msr elr_el2, {}", in(reg) elr, options(nostack, nomem));
        core::arch::asm!("msr spsr_el2, {}", in(reg) spsr, options(nostack, nomem));
        core::arch::asm!("isb", options(nostack, nomem));
    }
}

/// Common preamble before enter_guest: clear FMO, mask PSTATE.F, save host state.
/// Returns the CPU index used for per-CPU save slots.
///
/// The SP reference (`&mut SpContext`) is preserved across `enter_guest()` by the
/// C calling convention — `enter_guest` saves/restores callee-saved registers
/// (x19-x28), so the compiler can safely keep the SP pointer in one of these regs.
/// This eliminates the need for a global `SAVED_SP_PTR` array which was vulnerable
/// to corruption during multi-CPU dispatch.
#[cfg(feature = "sel2")]
#[inline(always)]
fn pre_enter_guest(_sp: &mut crate::sp_context::SpContext) -> usize {
    // Clear HCR_EL2.FMO so NS FIQ doesn't trap to S-EL2 during SP execution.
    // Mask FIQ in PSTATE.F at S-EL2 to prevent FIQ during enter_guest asm.
    // SAFETY: modifies only local CPU trap/mask bits before guest entry.
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
    let cpu = sel2_cpu_id();
    let (saved_elr, saved_spsr) = save_host_el2_state();
    SAVED_HOST_ELR[cpu].store(saved_elr, Ordering::Relaxed);
    SAVED_HOST_SPSR[cpu].store(saved_spsr, Ordering::Relaxed);
    cpu
}

/// Common postamble after enter_guest: restore host state, re-enable FMO.
///
/// The SP reference is passed in by the caller (preserved in callee-saved register
/// across `enter_guest()`), not reloaded from a global. This eliminates a class of
/// multi-CPU corruption where SAVED_SP_PTR could be overwritten by another CPU.
#[cfg(feature = "sel2")]
#[inline(always)]
fn post_enter_guest(cpu: usize) {
    restore_host_el2_state(
        SAVED_HOST_ELR[cpu].load(Ordering::Relaxed),
        SAVED_HOST_SPSR[cpu].load(Ordering::Relaxed),
    );
    // SAFETY: TPIDR_EL2 is scratch for this CPU and cleared on return path.
    unsafe {
        core::arch::asm!("msr tpidr_el2, xzr", options(nostack, nomem));
    }
    // Restore HCR_EL2.FMO and unmask PSTATE.F
    // SAFETY: restores local CPU trap/mask bits after guest exit.
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
}

/// SPMC event loop — dispatches FF-A requests from SPMD (EL3) forever.
///
/// `first_request` is the SmcResult8 returned by the initial FFA_MSG_WAIT
/// SMC (sent during SPMC boot). Each iteration dispatches the request,
/// sends the response back to SPMD via forward_smc8(), and receives the
/// next request in the return value.
///
/// HOST_EL1_STATE save/restore happens HERE at the event loop level:
/// - Save host EL1 immediately after forward_smc8() returns (captures the
///   NWd EL1 state that SPMD left in hardware when entering S-EL2)
/// - Restore host EL1 immediately before forward_smc8() call (restores
///   NWd EL1 state so SPMD returns to NWd with correct EL1 regs)
///
/// This placement is critical: restoring pKVM's EL1 state (SCTLR_EL1.M=1
/// with pKVM's page tables) during SPMC Rust code execution would crash
/// because our code runs with the SPMC's own S-EL2 Stage-1 MMU. The
/// restore must happen right before we SMC back to SPMD.
#[cfg(feature = "sel2")]
pub fn run_event_loop(first_request: SmcResult8) -> ! {
    let cpu = sel2_cpu_id();
    let mut request = first_request;

    // Save NWd EL1 state from the initial entry (FFA_MSG_WAIT return).
    // SPMD does NOT save/restore EL1 when SPMD_SPM_AT_SEL2=1 (only EL2),
    // so hardware EL1 regs contain whatever NWd (pKVM) left. We must save
    // them before dispatch_to_sp() overwrites them with SP's EL1 state.
    // SAFETY: this CPU owns its HOST_EL1_STATE slot.
    unsafe {
        (*HOST_EL1_STATE.0[cpu].get()).save();
    }

    loop {
        let response = dispatch_request(&request);

        // Disable Secure Group 1 interrupt delivery before SMC to SPMD.
        // SAFETY: local CPU interrupt-group enable programming.
        unsafe {
            core::arch::asm!("msr ICC_IGRPEN1_EL1, xzr", "isb", options(nostack, nomem),);
        }

        // Restore NWd EL1 state before SMC to SPMD. SPMD_SPM_AT_SEL2=1 means
        // SPMD only saves/restores EL2 registers across world switches — EL1
        // regs pass through unchanged. Without this, SP's EL1 state (SCTLR=0,
        // VBAR=0) leaks to pKVM, crashing the host kernel.
        //
        // Uses restore_except_sp_el0() because at S-EL2 (SPSel=0), SP_EL0 is
        // our current stack pointer — overwriting it would corrupt the stack.
        // SAFETY: this CPU owns its HOST_EL1_STATE slot.
        unsafe {
            (*HOST_EL1_STATE.0[cpu].get()).restore_except_sp_el0();
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

        // Save NWd EL1 state immediately after SPMD returns to S-EL2.
        // Hardware EL1 regs now contain NWd's state (passed through by SPMD).
        // SAFETY: this CPU owns its HOST_EL1_STATE slot.
        unsafe {
            (*HOST_EL1_STATE.0[cpu].get()).save();
        }

        // Re-enable Secure Group 1 for IRQ handling during SP execution
        // SAFETY: local CPU interrupt-group enable programming.
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
    if req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_32 || req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_64 {
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
    // Acquire per-SP dispatch lock to prevent two CPUs from simultaneously
    // entering the same SP context (data race on VcpuContext fields).
    let mut sp_guard = match crate::sp_context::try_lock_sp(sp_id) {
        Ok(g) => g,
        Err(crate::sp_context::SpLockError::NotFound) => {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        Err(crate::sp_context::SpLockError::Busy) => return make_error(ffa::FFA_BUSY as u64),
    };

    let cpu = sel2_cpu_id();
    let mut sp = sp_guard.sp_mut();

    // Claim ownership for initial dispatch (Idle -> Running).
    if !sp.try_claim_owner_cpu(cpu) {
        return make_error(ffa::FFA_BUSY as u64);
    }

    // Atomically claim SP: Idle→Running.
    if sp
        .try_transition(
            crate::sp_context::SpState::Idle,
            crate::sp_context::SpState::Running,
        )
        .is_err()
    {
        sp.clear_owner_cpu();
        return make_error(ffa::FFA_BUSY as u64);
    }
    sp.clear_preempted_cpu();

    // Set up SP registers with the DIRECT_REQ args
    sp.set_args(
        req.x0, req.x1, req.x2, req.x3, req.x4, req.x5, req.x6, req.x7,
    );

    // Clear preemption flag before SP entry
    SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);

    // Inject any pending virtual interrupt before entry
    inject_pending_virq(sp);

    // Install SP's Secure Stage-2 (VSTTBR_EL2/VSTCR_EL2)
    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();

    // Track which SP is running so exception/IRQ handler can route correctly
    CURRENT_RUNNING_SP[cpu].store(sp_id, Ordering::Release);

    // Restore SP's EL1 sysregs (SCTLR_EL1, VBAR_EL1, etc.)
    sp.restore_el1_state();

    // Arm CNTHP poll timer so owned INTIDs get injected during slow-path
    crate::arch::aarch64::peripherals::timer::arm_preemption_timer();

    pre_enter_guest(sp);

    let ctx_ptr = sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext;

    // SAFETY: `ctx_ptr` points to the current SP's owned VcpuContext and the
    // per-SP lock ensures exclusive mutable access across CPUs.
    let _exit = unsafe { crate::arch::aarch64::enter_guest(ctx_ptr) };

    // sp reference is preserved across enter_guest() via callee-saved register
    post_enter_guest(cpu);

    sp.save_el1_state();

    CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
    crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();

    // Handle SP exit — may loop if SP calls FF-A operations or SP→SP DIRECT_REQ
    let result;
    loop {
        let exit_result = handle_sp_exit(sp, sp_id);

        if exit_result.x0 != SP_TO_SP_PENDING {
            result = exit_result;
            break;
        }

        // SP→SP DIRECT_REQ: caller is now Blocked, callee dispatch needed
        let dest_id = exit_result.x1 as u16;

        // Save caller state before entering callee
        sp.save_el1_state();
        let caller_cpu = sel2_cpu_id();
        CURRENT_RUNNING_SP[caller_cpu].store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
        clear_secure_stage2();

        // Build callee request from caller's saved registers (the DIRECT_REQ args)
        let (cx0, cx1, cx2, cx3, cx4, cx5, cx6, cx7) = sp.get_args();
        let callee_req = SmcResult8 {
            x0: cx0,
            x1: cx1,
            x2: cx2,
            x3: cx3,
            x4: cx4,
            x5: cx5,
            x6: cx6,
            x7: cx7,
        };

        // Drop caller's dispatch lock before acquiring callee's
        drop(sp_guard);

        // Recursive dispatch to callee SP
        let callee_result = dispatch_to_sp(&callee_req, dest_id);

        // Re-acquire caller's dispatch lock
        sp_guard = match crate::sp_context::try_lock_sp(sp_id) {
            Ok(g) => g,
            Err(_) => {
                CALL_STACK.lock().pop();
                return make_error(ffa::FFA_DENIED as u64);
            }
        };
        sp = sp_guard.sp_mut();

        if callee_result.x0 == ffa::FFA_INTERRUPT {
            // Chain preemption: Blocked → Preempted, do NOT pop stack
            if sp
                .transition_to(crate::sp_context::SpState::Preempted)
                .is_err()
            {
                CALL_STACK.lock().pop();
                return make_error(ffa::FFA_DENIED as u64);
            }
            sp.set_preempted_cpu(sel2_cpu_id());
            result = callee_result;
            break;
        }

        // Normal completion: pop stack frame, Blocked → Running
        CALL_STACK.lock().pop();
        if sp
            .transition_to(crate::sp_context::SpState::Running)
            .is_err()
        {
            return make_error(ffa::FFA_DENIED as u64);
        }

        // Write callee's response to caller's registers
        sp.set_args(
            callee_result.x0,
            callee_result.x1,
            callee_result.x2,
            callee_result.x3,
            callee_result.x4,
            callee_result.x5,
            callee_result.x6,
            callee_result.x7,
        );

        // Re-enter caller: restore S2, EL1, re-enter guest loop
        if sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
            return make_error(ffa::FFA_DENIED as u64);
        }
        if sp
            .transition_to(crate::sp_context::SpState::Running)
            .is_err()
        {
            return make_error(ffa::FFA_DENIED as u64);
        }

        let re_cpu = sel2_cpu_id();
        SP_IRQ_PREEMPTED[re_cpu].store(false, Ordering::Release);
        inject_pending_virq(sp);
        let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
        s2.install();
        CURRENT_RUNNING_SP[re_cpu].store(sp_id, Ordering::Release);
        sp.restore_el1_state();
        crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
        pre_enter_guest(sp);

        let _exit = unsafe {
            crate::arch::aarch64::enter_guest(
                sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext
            )
        };
        post_enter_guest(re_cpu);
        sp.save_el1_state();
        CURRENT_RUNNING_SP[re_cpu].store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
        // Loop back to handle_sp_exit() for the re-entered caller
    }

    // Clear Secure Stage-2 and HCR_EL2 bits before returning to SPMD.
    clear_secure_stage2();

    result
}

/// Clear Secure Stage-2 state after SP execution.
///
/// Zeroes VSTTBR_EL2/VSTCR_EL2 and clears HCR_EL2.{VI,VF}.
/// Preserves HCR_EL2.VM — SPMD saves/restores HCR_EL2 across world
/// switches and expects VM to remain set. Clearing VM causes
/// forward_smc8() to hang because SPMD never returns to S-EL2.
/// With VSTTBR_EL2=0, VM=1 is harmless (no Stage-2 translation occurs).
/// Preserves TSC, FMO, IMO, AMO, RW and other trap bits.
#[cfg(feature = "sel2")]
fn clear_secure_stage2() {
    // SAFETY: programs local CPU secure stage-2 and HCR_EL2 control bits.
    unsafe {
        // Clear Secure Stage-2 translation registers
        core::arch::asm!(
            "msr s3_4_c2_c6_0, xzr", // VSTTBR_EL2 = 0
            "msr s3_4_c2_c6_2, xzr", // VSTCR_EL2 = 0
            "isb",
            options(nostack, nomem),
        );

        // Clear only VI, VF bits from HCR_EL2.
        // Do NOT clear VM (bit 0) — SPMD expects it to remain set.
        core::arch::asm!(
            "mrs {tmp}, hcr_el2",
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
fn handle_sp_exit(sp: &mut crate::sp_context::SpContext, sp_id: u16) -> SmcResult8 {
    loop {
        // Check if SP was preempted by a physical IRQ (NS FIQ at S-EL2)
        let cpu = sel2_cpu_id();
        if SP_IRQ_PREEMPTED[cpu].swap(false, Ordering::Acquire) {
            crate::log_debug!(
                "[SPMC] preempted sp={:#06x} fiq_count={}\n",
                sp_id,
                FIQ_PREEMPT_COUNT[cpu].load(Ordering::Relaxed)
            );
            if sp
                .transition_to(crate::sp_context::SpState::Preempted)
                .is_err()
            {
                return make_error(ffa::FFA_DENIED as u64);
            }
            sp.set_preempted_cpu(cpu);

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
        if x0 != ffa::FFA_MSG_SEND_DIRECT_RESP_32
            && x0 != ffa::FFA_MSG_SEND_DIRECT_RESP_64
            && x0 != ffa::FFA_MSG_WAIT
            && x0 != ffa::FFA_RX_RELEASE
            && x0 != ffa::FFA_MEM_RETRIEVE_REQ_32
            && x0 != ffa::FFA_MEM_RETRIEVE_REQ_64
            && x0 != ffa::FFA_MEM_RELINQUISH
            && x0 != ffa::FFA_MEM_SHARE_32
            && x0 != ffa::FFA_MEM_SHARE_64
            && x0 != ffa::FFA_MEM_LEND_32
            && x0 != ffa::FFA_MEM_LEND_64
            && x0 != ffa::FFA_MEM_DONATE_32
            && x0 != ffa::FFA_MEM_DONATE_64
            && x0 != ffa::FFA_MEM_RECLAIM
            && x0 != ffa::FFA_MEM_FRAG_RX
            && x0 != ffa::FFA_CONSOLE_LOG_32
            && x0 != ffa::FFA_CONSOLE_LOG_64
            && x0 != ffa::FFA_MSG_SEND_DIRECT_REQ_32
            && x0 != ffa::FFA_MSG_SEND_DIRECT_REQ_64
        {
            let ctx = sp.vcpu_ctx();
            let esr = ctx.sys_regs.esr_el2;
            let ec = (esr >> 26) & 0x3f;
            crate::log_warn!(
                "[SPMC] unexpected SP exit sp={:#06x} x0={:#018x} x1={:#018x} x3={:#018x} x4={:#018x}\n",
                sp_id, x0, x1, x3, x4
            );
            crate::log_warn!(
                "[SPMC] exit detail ec={:#x} esr={:#018x} pc={:#018x} elr_el1={:#018x}\n",
                ec,
                esr,
                ctx.pc,
                ctx.sys_regs.elr_el1
            );

            // Unexpected SP exits are fatal for the current transaction.
            // Do not forward raw x0/x1 to NWd (can surface as -EINVAL).
            let _ = sp.transition_to(crate::sp_context::SpState::Idle);
            sp.clear_preempted_cpu();
            sp.clear_owner_cpu();
            return make_error(ffa::FFA_DENIED as u64);
        }

        match x0 {
            ffa::FFA_MEM_RETRIEVE_REQ_32 | ffa::FFA_MEM_RETRIEVE_REQ_64 => {
                // SP-initiated MEM_RETRIEVE: build request, call handler, re-enter SP
                let sp_req = SmcResult8 {
                    x0,
                    x1,
                    x2,
                    x3,
                    x4,
                    x5,
                    x6,
                    x7,
                };
                let result = handle_spmc_mem_retrieve(&sp_req, Some((sp.sp_id(), sp.vsttbr())));
                sp.set_args(
                    result.x0, result.x1, result.x2, result.x3, result.x4, result.x5, result.x6,
                    result.x7,
                );
            }
            ffa::FFA_MEM_RELINQUISH => {
                // SP-initiated MEM_RELINQUISH: build request, call handler, re-enter SP
                let sp_req = SmcResult8 {
                    x0,
                    x1,
                    x2,
                    x3,
                    x4,
                    x5,
                    x6,
                    x7,
                };
                let result = handle_spmc_mem_relinquish(&sp_req, Some((sp.sp_id(), sp.vsttbr())));
                sp.set_args(
                    result.x0, result.x1, result.x2, result.x3, result.x4, result.x5, result.x6,
                    result.x7,
                );
            }
            ffa::FFA_MEM_SHARE_32
            | ffa::FFA_MEM_SHARE_64
            | ffa::FFA_MEM_LEND_32
            | ffa::FFA_MEM_LEND_64
            | ffa::FFA_MEM_DONATE_32
            | ffa::FFA_MEM_DONATE_64 => {
                let is_lend = x0 == ffa::FFA_MEM_LEND_32 || x0 == ffa::FFA_MEM_LEND_64;
                let is_donate = x0 == ffa::FFA_MEM_DONATE_32 || x0 == ffa::FFA_MEM_DONATE_64;
                match validate_sp_share(sp_id, x1, x3, x4, x5, is_lend, is_donate) {
                    Ok(handle) => sp.set_args(
                        ffa::FFA_SUCCESS_32,
                        0,
                        handle & 0xFFFF_FFFF,
                        handle >> 32,
                        0,
                        0,
                        0,
                        0,
                    ),
                    Err(code) => sp.set_args(ffa::FFA_ERROR, 0, code, 0, 0, 0, 0, 0),
                }
            }
            ffa::FFA_MEM_RECLAIM => match validate_sp_reclaim(sp_id, x1, x2) {
                Ok(()) => sp.set_args(ffa::FFA_SUCCESS_32, 0, 0, 0, 0, 0, 0, 0),
                Err(code) => sp.set_args(ffa::FFA_ERROR, 0, code, 0, 0, 0, 0, 0),
            },
            ffa::FFA_MEM_FRAG_RX => {
                // SP-initiated FRAG_RX: deliver next chunk of RETRIEVE_RESP
                let sp_req = SmcResult8 {
                    x0,
                    x1,
                    x2,
                    x3,
                    x4,
                    x5,
                    x6,
                    x7,
                };
                let result = handle_spmc_mem_frag_rx(&sp_req);
                sp.set_args(
                    result.x0, result.x1, result.x2, result.x3, result.x4, result.x5, result.x6,
                    result.x7,
                );
            }
            ffa::FFA_MSG_WAIT => {
                // SP-initiated MSG_WAIT: check if indirect message is pending
                if let Some(idx) = sp_mailbox_index(sp_id) {
                    let mailboxes = SP_MAILBOXES.lock();
                    if mailboxes[idx].msg_pending {
                        // Message pending — return sender ID and re-enter SP
                        let sender = mailboxes[idx].msg_sender_id;
                        drop(mailboxes);
                        sp.set_args(ffa::FFA_SUCCESS_32, sender as u64, 0, 0, 0, 0, 0, 0);
                        continue; // re-enter SP via enter_guest()
                    }
                }
                // No message pending — SP yields, transition to Idle
                if sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
                    return make_error(ffa::FFA_DENIED as u64);
                }
                sp.clear_preempted_cpu();
                sp.clear_owner_cpu();
                return SmcResult8 {
                    x0,
                    x1,
                    x2,
                    x3,
                    x4,
                    x5,
                    x6,
                    x7,
                };
            }
            ffa::FFA_RX_RELEASE => {
                // SP-initiated RX_RELEASE: clear pending message
                if let Some(idx) = sp_mailbox_index(sp_id) {
                    let mut mailboxes = SP_MAILBOXES.lock();
                    mailboxes[idx].msg_pending = false;
                }
                sp.set_args(ffa::FFA_SUCCESS_32, 0, 0, 0, 0, 0, 0, 0);
                continue; // re-enter SP
            }
            ffa::FFA_CONSOLE_LOG_32 | ffa::FFA_CONSOLE_LOG_64 => {
                // SP-initiated CONSOLE_LOG: extract chars and write to UART
                let sp_req = SmcResult8 {
                    x0,
                    x1,
                    x2,
                    x3,
                    x4,
                    x5,
                    x6,
                    x7,
                };
                let result = handle_console_log(&sp_req);
                sp.set_args(
                    result.x0, result.x1, result.x2, result.x3, result.x4, result.x5, result.x6,
                    result.x7,
                );
                continue; // re-enter SP
            }
            ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
                // SP→SP DIRECT_REQ: validate, push call stack, return sentinel
                let source_id = (x1 >> 16) as u16;
                let dest_id = (x1 & 0xFFFF) as u16;

                let mut valid = true;

                // Validation 1: source must match current SP
                if source_id != sp_id {
                    sp.set_args(
                        ffa::FFA_ERROR,
                        0,
                        ffa::FFA_INVALID_PARAMETERS as u64,
                        0,
                        0,
                        0,
                        0,
                        0,
                    );
                    valid = false;
                }
                // Validation 2: no self-calls
                if valid && dest_id == sp_id {
                    sp.set_args(
                        ffa::FFA_ERROR,
                        0,
                        ffa::FFA_INVALID_PARAMETERS as u64,
                        0,
                        0,
                        0,
                        0,
                        0,
                    );
                    valid = false;
                }
                // Validation 3: destination must exist
                if valid && !crate::sp_context::is_registered_sp(dest_id) {
                    sp.set_args(
                        ffa::FFA_ERROR,
                        0,
                        ffa::FFA_INVALID_PARAMETERS as u64,
                        0,
                        0,
                        0,
                        0,
                        0,
                    );
                    valid = false;
                }
                // Validation 4+5: cycle detection + push (atomic under CALL_STACK lock)
                if valid {
                    let mut stack = CALL_STACK.lock();
                    if stack.contains(dest_id) {
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_BUSY as u64, 0, 0, 0, 0, 0);
                        valid = false;
                    } else if stack.push(sp_id, dest_id).is_err() {
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_BUSY as u64, 0, 0, 0, 0, 0);
                        valid = false;
                    }
                }
                if valid {
                    // Transition caller: Running → Blocked
                    if sp
                        .transition_to(crate::sp_context::SpState::Blocked)
                        .is_err()
                    {
                        CALL_STACK.lock().pop();
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_DENIED as u64, 0, 0, 0, 0, 0);
                        // Fall through to re-enter SP with error
                    } else {
                        // Return sentinel — dispatch_to_sp() handles the recursive call
                        return SmcResult8 {
                            x0: SP_TO_SP_PENDING,
                            x1: dest_id as u64,
                            x2: 0,
                            x3: 0,
                            x4: 0,
                            x5: 0,
                            x6: 0,
                            x7: 0,
                        };
                    }
                }
                // Validation failed: fall through to re-enter SP with FFA_ERROR in args
            }
            _ => {
                // Normal exit (FFA_DIRECT_RESP, etc.)
                if sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
                    return make_error(ffa::FFA_DENIED as u64);
                }
                sp.clear_preempted_cpu();
                sp.clear_owner_cpu();
                return SmcResult8 {
                    x0,
                    x1,
                    x2,
                    x3,
                    x4,
                    x5,
                    x6,
                    x7,
                };
            }
        }

        // Re-enter SP with the handler's result (Running→Idle→Running)
        if sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
            return make_error(ffa::FFA_DENIED as u64);
        }
        if sp
            .transition_to(crate::sp_context::SpState::Running)
            .is_err()
        {
            return make_error(ffa::FFA_DENIED as u64);
        }

        let cpu = sel2_cpu_id();
        SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);
        inject_pending_virq(sp);

        let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
        s2.install();
        CURRENT_RUNNING_SP[cpu].store(sp_id, Ordering::Release);

        sp.restore_el1_state();
        crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
        pre_enter_guest(sp);

        // SAFETY: pointer comes from the locked SP context and is exclusive.
        let _exit = unsafe {
            crate::arch::aarch64::enter_guest(
                sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext
            )
        };

        // sp reference preserved across enter_guest() via callee-saved register
        post_enter_guest(cpu);
        sp.save_el1_state();

        CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
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
    let cpu = sel2_cpu_id();

    let mut sp_guard = match crate::sp_context::try_lock_sp(sp_id) {
        Ok(g) => g,
        Err(crate::sp_context::SpLockError::NotFound) => {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        Err(crate::sp_context::SpLockError::Busy) => return make_error(ffa::FFA_DENIED as u64),
    };

    let sp = sp_guard.sp_mut();

    // Acquire ownership for this CPU. If another CPU owns the preempted SP,
    // perform explicit owner migration under dispatch lock.
    match sp.owner_cpu() {
        Some(owner) if owner == cpu => {}
        Some(owner) => {
            if !sp.try_migrate_owner_cpu(owner, cpu) {
                return make_error(ffa::FFA_BUSY as u64);
            }
        }
        None => {
            if !sp.try_claim_owner_cpu(cpu) {
                return make_error(ffa::FFA_BUSY as u64);
            }
        }
    }

    // Atomically claim: Preempted→Running
    if sp
        .try_transition(
            crate::sp_context::SpState::Preempted,
            crate::sp_context::SpState::Running,
        )
        .is_err()
    {
        return make_error(ffa::FFA_DENIED as u64);
    }

    sp.clear_preempted_cpu();
    SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);

    inject_pending_virq(sp);

    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();
    CURRENT_RUNNING_SP[cpu].store(sp_id, Ordering::Release);

    sp.restore_el1_state();
    crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
    pre_enter_guest(sp);

    // SAFETY: pointer comes from the locked SP context and is exclusive.
    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(
            sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext
        )
    };

    // sp reference preserved across enter_guest() via callee-saved register
    post_enter_guest(cpu);
    sp.save_el1_state();

    CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
    crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();

    let result = handle_sp_exit(sp, sp_id);

    // Check if this SP was a callee in an SP→SP chain.
    // If so, chain-resume the caller instead of returning to NWd.
    let caller_id = CALL_STACK.lock().find_caller(sp_id);
    if let Some(caller) = caller_id {
        // Pop the frame {caller, sp_id}
        CALL_STACK.lock().pop();

        // Drop callee's lock before acquiring caller's
        drop(sp_guard);

        // Transition caller: Preempted → Running
        let mut caller_guard = match crate::sp_context::try_lock_sp(caller) {
            Ok(g) => g,
            Err(_) => {
                clear_secure_stage2();
                return make_error(ffa::FFA_DENIED as u64);
            }
        };
        let caller_sp = caller_guard.sp_mut();

        match caller_sp.owner_cpu() {
            Some(owner) if owner == cpu => {}
            Some(owner) => {
                if !caller_sp.try_migrate_owner_cpu(owner, cpu) {
                    clear_secure_stage2();
                    return make_error(ffa::FFA_BUSY as u64);
                }
            }
            None => {
                if !caller_sp.try_claim_owner_cpu(cpu) {
                    clear_secure_stage2();
                    return make_error(ffa::FFA_BUSY as u64);
                }
            }
        }

        if caller_sp
            .try_transition(
                crate::sp_context::SpState::Preempted,
                crate::sp_context::SpState::Running,
            )
            .is_err()
        {
            clear_secure_stage2();
            return make_error(ffa::FFA_DENIED as u64);
        }
        caller_sp.clear_preempted_cpu();

        // Write callee's response to caller's registers
        caller_sp.set_args(
            result.x0, result.x1, result.x2, result.x3, result.x4, result.x5, result.x6, result.x7,
        );

        // Chain-resume: re-enter caller SP
        if caller_sp
            .transition_to(crate::sp_context::SpState::Idle)
            .is_err()
        {
            clear_secure_stage2();
            return make_error(ffa::FFA_DENIED as u64);
        }
        if caller_sp
            .transition_to(crate::sp_context::SpState::Running)
            .is_err()
        {
            clear_secure_stage2();
            return make_error(ffa::FFA_DENIED as u64);
        }

        SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);
        inject_pending_virq(caller_sp);
        let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(caller_sp.vsttbr());
        s2.install();
        CURRENT_RUNNING_SP[cpu].store(caller, Ordering::Release);
        caller_sp.restore_el1_state();
        crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
        pre_enter_guest(caller_sp);

        let _exit = unsafe {
            crate::arch::aarch64::enter_guest(
                caller_sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext
            )
        };
        post_enter_guest(cpu);
        caller_sp.save_el1_state();
        CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();

        let caller_result = handle_sp_exit(caller_sp, caller);
        clear_secure_stage2();
        return caller_result;
    }

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
        // SAFETY: local CPU HCR_EL2 update before guest entry.
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
fn dispatch_interrupt_to_sp(sp_id: u16) -> bool {
    let mut sp_guard = match crate::sp_context::try_lock_sp(sp_id) {
        Ok(g) => g,
        Err(_) => return false,
    };

    let cpu = sel2_cpu_id();
    let sp = sp_guard.sp_mut();

    // Atomically claim: Idle→Running
    if sp
        .try_transition(
            crate::sp_context::SpState::Idle,
            crate::sp_context::SpState::Running,
        )
        .is_err()
    {
        return false;
    }

    // Claim or migrate ownership for this CPU after successful state claim.
    match sp.owner_cpu() {
        Some(owner) if owner == cpu => {}
        Some(owner) => {
            if !sp.try_migrate_owner_cpu(owner, cpu) {
                let _ = sp.transition_to(crate::sp_context::SpState::Idle);
                return false;
            }
        }
        None => {
            if !sp.try_claim_owner_cpu(cpu) {
                let _ = sp.transition_to(crate::sp_context::SpState::Idle);
                return false;
            }
        }
    }

    SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);
    inject_pending_virq(sp);

    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();

    // Make HF_INTERRUPT_GET route to this SP while it is running.
    CURRENT_RUNNING_SP[cpu].store(sp_id, Ordering::Release);

    sp.restore_el1_state();
    crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
    pre_enter_guest(sp);

    // SAFETY: pointer comes from the locked SP context and is exclusive.
    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(
            sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext
        )
    };

    // sp reference preserved across enter_guest() via callee-saved register
    post_enter_guest(cpu);
    sp.save_el1_state();

    CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
    crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
    clear_secure_stage2();
    let preempted = SP_IRQ_PREEMPTED[cpu].swap(false, Ordering::Acquire);
    if preempted {
        if sp
            .transition_to(crate::sp_context::SpState::Preempted)
            .is_err()
        {
            return false;
        }
        sp.set_preempted_cpu(cpu);
    } else {
        if sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
            return false;
        }
        sp.clear_preempted_cpu();
        sp.clear_owner_cpu();
    }

    preempted
}

/// Dispatch an FF-A request and return the appropriate response.
///
/// Pure function: matches on the FF-A function ID in req.x0 and builds
/// a response SmcResult8. Not gated by feature flags so it can be unit
/// tested on the host.
/// Dispatch an FF-A call as if from a specific SP (for cross-SP isolation tests).
/// `sp_id` is the calling SP's partition ID, `vsttbr` is its Stage-2 base (0 for unit tests).
pub fn dispatch_ffa_as_sp(req: &SmcResult8, sp_id: u16, vsttbr: u64) -> SmcResult8 {
    let current_sp = Some((sp_id, vsttbr));
    match req.x0 {
        ffa::FFA_MEM_RETRIEVE_REQ_32 | ffa::FFA_MEM_RETRIEVE_REQ_64 => {
            handle_spmc_mem_retrieve(req, current_sp)
        }
        ffa::FFA_MEM_RELINQUISH => handle_spmc_mem_relinquish(req, current_sp),
        ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
            // SP→SP DIRECT_REQ validation (unit test path — no real SP dispatch)
            let source_id = (req.x1 >> 16) as u16;
            let dest_id = (req.x1 & 0xFFFF) as u16;
            if source_id != sp_id {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }
            if dest_id == sp_id {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }
            if !crate::sp_context::is_registered_sp(dest_id) {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }
            let stack = CALL_STACK.lock();
            if stack.contains(dest_id) {
                return make_error(ffa::FFA_BUSY as u64);
            }
            drop(stack);
            // In unit tests, cannot actually dispatch — return DENIED
            make_error(ffa::FFA_DENIED as u64)
        }
        ffa::FFA_MEM_SHARE_32
        | ffa::FFA_MEM_SHARE_64
        | ffa::FFA_MEM_LEND_32
        | ffa::FFA_MEM_LEND_64
        | ffa::FFA_MEM_DONATE_32
        | ffa::FFA_MEM_DONATE_64 => {
            let is_lend = req.x0 == ffa::FFA_MEM_LEND_32 || req.x0 == ffa::FFA_MEM_LEND_64;
            let is_donate = req.x0 == ffa::FFA_MEM_DONATE_32 || req.x0 == ffa::FFA_MEM_DONATE_64;
            match validate_sp_share(sp_id, req.x1, req.x3, req.x4, req.x5, is_lend, is_donate) {
                Ok(handle) => SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: handle & 0xFFFF_FFFF,
                    x3: handle >> 32,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                },
                Err(code) => make_error(code),
            }
        }
        ffa::FFA_MEM_RECLAIM => match validate_sp_reclaim(sp_id, req.x1, req.x2) {
            Ok(()) => make_success(),
            Err(code) => make_error(code),
        },
        _ => dispatch_ffa(req),
    }
}

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
            let queried_fid = req.x1;

            // Feature IDs (non-function): SRI and NPI return donated SGI INTIDs
            if queried_fid == ffa::FFA_FEATURE_NPI {
                return SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: ffa::NPI_INTID as u64,
                    x3: 0,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                };
            }
            if queried_fid == ffa::FFA_FEATURE_SRI {
                return SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: ffa::SRI_INTID as u64,
                    x3: 0,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                };
            }

            // Function IDs
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
                    | ffa::FFA_MSG_SEND2
                    | ffa::FFA_MSG_WAIT
                    | ffa::FFA_RUN
                    | ffa::FFA_MEM_SHARE_32
                    | ffa::FFA_MEM_LEND_32
                    | ffa::FFA_MEM_DONATE_32
                    | ffa::FFA_MEM_RETRIEVE_REQ_32
                    | ffa::FFA_MEM_RELINQUISH
                    | ffa::FFA_MEM_RECLAIM
                    | ffa::FFA_MEM_FRAG_TX
                    | ffa::FFA_MEM_FRAG_RX
                    | ffa::FFA_NOTIFICATION_BITMAP_CREATE
                    | ffa::FFA_NOTIFICATION_BITMAP_DESTROY
                    | ffa::FFA_NOTIFICATION_BIND
                    | ffa::FFA_NOTIFICATION_UNBIND
                    | ffa::FFA_NOTIFICATION_SET
                    | ffa::FFA_NOTIFICATION_GET
                    | ffa::FFA_NOTIFICATION_INFO_GET_32
                    | ffa::FFA_NOTIFICATION_INFO_GET_64
                    | ffa::FFA_CONSOLE_LOG_32
                    | ffa::FFA_CONSOLE_LOG_64
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
            let state = match crate::sp_context::state_of(sp_id) {
                Some(s) => s,
                None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
            };
            if state != crate::sp_context::SpState::Preempted {
                return make_error(ffa::FFA_DENIED as u64);
            }
            // In sel2 mode, dispatch_request() handles this before we get here.
            // In unit tests (no sel2), just validate the state.
            make_error(ffa::FFA_NOT_SUPPORTED as u64)
        }

        ffa::FFA_RXTX_MAP => handle_rxtx_map(req),
        ffa::FFA_RXTX_UNMAP => handle_rxtx_unmap(),
        ffa::FFA_RX_RELEASE => handle_rx_release(),

        ffa::FFA_PARTITION_INFO_GET => handle_partition_info_get(),

        ffa::FFA_MSG_SEND2 => handle_spmc_msg_send2(req),
        ffa::FFA_MSG_WAIT => handle_spmc_msg_wait_nwd(),

        ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => handle_direct_req(req),

        ffa::FFA_MEM_SHARE_32 | ffa::FFA_MEM_SHARE_64 => handle_spmc_mem_share(req, false, false),
        ffa::FFA_MEM_LEND_32 | ffa::FFA_MEM_LEND_64 => handle_spmc_mem_share(req, true, false),
        ffa::FFA_MEM_RETRIEVE_REQ_32 | ffa::FFA_MEM_RETRIEVE_REQ_64 => {
            handle_spmc_mem_retrieve(req, None)
        }
        ffa::FFA_MEM_RELINQUISH => handle_spmc_mem_relinquish(req, None),
        ffa::FFA_MEM_RECLAIM => handle_spmc_mem_reclaim(req),
        ffa::FFA_MEM_FRAG_TX => handle_spmc_mem_frag_tx(req),
        ffa::FFA_MEM_FRAG_RX => handle_spmc_mem_frag_rx(req),
        ffa::FFA_MEM_DONATE_32 | ffa::FFA_MEM_DONATE_64 => handle_spmc_mem_share(req, false, true),

        // ── Notifications ──────────────────────────────────────────────
        ffa::FFA_NOTIFICATION_BITMAP_CREATE => {
            let part_id = req.x1 as u16;
            match crate::ffa::notifications::bitmap_create(part_id) {
                Ok(()) => make_success(),
                Err(code) => make_error(code as u64),
            }
        }
        ffa::FFA_NOTIFICATION_BITMAP_DESTROY => {
            let part_id = req.x1 as u16;
            match crate::ffa::notifications::bitmap_destroy(part_id) {
                Ok(()) => make_success(),
                Err(code) => make_error(code as u64),
            }
        }
        ffa::FFA_NOTIFICATION_BIND => {
            let sender = ((req.x1 >> 16) & 0xFFFF) as u16;
            let receiver = (req.x1 & 0xFFFF) as u16;
            let flags = req.x2 as u32;
            let bitmap = req.x3 | (req.x4 << 32);
            match crate::ffa::notifications::bind(sender, receiver, flags, bitmap) {
                Ok(()) => make_success(),
                Err(code) => make_error(code as u64),
            }
        }
        ffa::FFA_NOTIFICATION_UNBIND => {
            let sender = ((req.x1 >> 16) & 0xFFFF) as u16;
            let receiver = (req.x1 & 0xFFFF) as u16;
            let bitmap = req.x3 | (req.x4 << 32);
            match crate::ffa::notifications::unbind(sender, receiver, bitmap) {
                Ok(()) => make_success(),
                Err(code) => make_error(code as u64),
            }
        }
        ffa::FFA_NOTIFICATION_SET => {
            let sender = ((req.x1 >> 16) & 0xFFFF) as u16;
            let receiver = (req.x1 & 0xFFFF) as u16;
            let bitmap = req.x3 | (req.x4 << 32);
            match crate::ffa::notifications::set(sender, receiver, bitmap) {
                Ok(()) => make_success(),
                Err(code) => make_error(code as u64),
            }
        }
        ffa::FFA_NOTIFICATION_GET => {
            let receiver = req.x1 as u16;
            match crate::ffa::notifications::get(receiver) {
                Ok(pending) => SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: pending,
                    x3: 0,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                },
                Err(code) => make_error(code as u64),
            }
        }
        ffa::FFA_NOTIFICATION_INFO_GET_32 | ffa::FFA_NOTIFICATION_INFO_GET_64 => {
            let (count, ids) = crate::ffa::notifications::info_get();
            if count == 0 {
                make_error(ffa::FFA_NO_DATA as u64)
            } else {
                let mut packed: u64 = 0;
                for i in 0..count.min(4) {
                    packed |= (ids[i] as u64) << (i * 16);
                }
                SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: count as u64,
                    x3: packed,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                }
            }
        }

        // ── Console log ──────────────────────────────────────────────
        ffa::FFA_CONSOLE_LOG_32 | ffa::FFA_CONSOLE_LOG_64 => handle_console_log(req),

        _ => make_error(ffa::FFA_NOT_SUPPORTED as u64),
    }
}

/// Handle FFA_CONSOLE_LOG: extract packed characters from x2-x7 and write to UART.
fn handle_console_log(req: &SmcResult8) -> SmcResult8 {
    let char_count = req.x1 as usize;
    if char_count == 0 || char_count > 48 {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    let regs = [req.x2, req.x3, req.x4, req.x5, req.x6, req.x7];
    let mut buf = [0u8; 48];
    let mut len = 0;
    for i in 0..char_count {
        let reg_idx = i / 8;
        let byte_idx = i % 8;
        if reg_idx >= 6 {
            break;
        }
        let ch = ((regs[reg_idx] >> (byte_idx * 8)) & 0xFF) as u8;
        if ch != 0 {
            buf[len] = ch;
            len += 1;
        }
    }
    if len > 0 {
        crate::uart_puts(&buf[..len]);
    }

    make_success()
}

// ── Indirect messaging (MSG_SEND2 / MSG_WAIT) ───────────────────────

/// Handle FFA_MSG_SEND2 from NWd — copy message from NWd TX to target SP's mailbox.
///
/// NWd TX layout: sender_id(u16) + receiver_id(u16) + size(u32) + payload.
fn handle_spmc_msg_send2(_req: &SmcResult8) -> SmcResult8 {
    let nwd = NWD_RXTX.lock();
    if !nwd.mapped {
        return make_error(ffa::FFA_DENIED as u64);
    }
    let tx_pa = nwd.tx_pa;
    drop(nwd);

    // Read message header from NWd TX buffer
    let (msg_sender_id, msg_receiver_id, msg_size) = {
        #[cfg(feature = "sel2")]
        {
            // SAFETY: tx_pa is NWd physical address, accessible at S-EL2 via
            // Stage-1 MMU with NS=1. Header is 8 bytes, bounded read.
            unsafe {
                let tx_ptr = tx_pa as *const u8;
                let s = core::ptr::read_unaligned(tx_ptr as *const u16);
                let r = core::ptr::read_unaligned(tx_ptr.add(2) as *const u16);
                let sz = core::ptr::read_unaligned(tx_ptr.add(4) as *const u32);
                (s, r, sz)
            }
        }
        #[cfg(not(feature = "sel2"))]
        {
            // Unit test mode: read from pre-filled TX buffer via NWD_RXTX.tx_pa
            // Tests set tx_pa to a stack buffer address.
            unsafe {
                let tx_ptr = tx_pa as *const u8;
                let s = core::ptr::read_unaligned(tx_ptr as *const u16);
                let r = core::ptr::read_unaligned(tx_ptr.add(2) as *const u16);
                let sz = core::ptr::read_unaligned(tx_ptr.add(4) as *const u32);
                (s, r, sz)
            }
        }
    };

    // Validate receiver is a registered SP
    let mbox_idx = match sp_mailbox_index(msg_receiver_id) {
        Some(idx) if crate::sp_context::is_registered_sp(msg_receiver_id) => idx,
        _ => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    let mut mailboxes = SP_MAILBOXES.lock();
    let mbox = &mut mailboxes[mbox_idx];

    if mbox.msg_pending {
        return make_error(ffa::FFA_BUSY as u64);
    }

    // Copy header + payload to SP's RX buffer
    let copy_len = core::cmp::min((8 + msg_size) as usize, 4096);
    unsafe {
        core::ptr::copy_nonoverlapping(tx_pa as *const u8, mbox.rx_buf.as_mut_ptr(), copy_len);
    }

    mbox.msg_pending = true;
    mbox.msg_sender_id = msg_sender_id;
    make_success()
}

/// Handle FFA_MSG_WAIT from NWd — SPMC doesn't queue messages for NWd.
fn handle_spmc_msg_wait_nwd() -> SmcResult8 {
    make_error(ffa::FFA_NO_DATA as u64)
}

/// Handle FFA_RX_RELEASE from NWd — release NWd's RX buffer.
/// (Separate from SP RX_RELEASE — NWd has its own RXTX state.)
/// Already handled by existing handle_rx_release() for NWd RXTX.

/// Handle FFA_RXTX_MAP — store NWd's TX/RX buffer PAs.
///
/// SPMD at EL3 forwards this from NWd to SPMC. We store the PAs for later
/// use by PARTITION_INFO_GET (which writes descriptors directly to NWd's RX).
fn handle_rxtx_map(req: &SmcResult8) -> SmcResult8 {
    let tx_pa = req.x1;
    let rx_pa = req.x2;
    let page_count = req.x3 as u32;

    crate::log_debug!(
        "[SPMC] RXTX_MAP: tx={:#x} rx={:#x} pages={}\n",
        tx_pa,
        rx_pa,
        page_count
    );

    // Validate alignment
    if tx_pa & 0xFFF != 0 || rx_pa & 0xFFF != 0 || page_count == 0 {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    // In sel2 mode, validate PAs are in S-EL2 Stage-1 accessible NS DRAM range.
    // Only accept addresses that S-EL2 can safely dereference.
    #[cfg(feature = "sel2")]
    if tx_pa < 0x4000_0000 || tx_pa >= 0xC000_0000 || rx_pa < 0x4000_0000 || rx_pa >= 0xC000_0000 {
        crate::log_debug!(
            "[SPMC] RXTX_MAP: rejecting out-of-range tx={:#x} rx={:#x}\n",
            tx_pa,
            rx_pa
        );
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    let mut nwd = NWD_RXTX.lock();
    // Allow re-mapping: pKVM's ffa_map_hyp_buffers() issues a second
    // RXTX_MAP after the host kernel's FF-A driver already registered
    // buffers before pKVM took EL2.  Accept the update silently.
    nwd.tx_pa = tx_pa;
    nwd.rx_pa = rx_pa;
    nwd.page_count = page_count;
    nwd.mapped = true;

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
    let mut nwd = NWD_RXTX.lock();
    if !nwd.mapped {
        return make_error(ffa::FFA_DENIED as u64);
    }
    nwd.tx_pa = 0;
    nwd.rx_pa = 0;
    nwd.page_count = 0;
    nwd.mapped = false;

    // Release NWD_RXTX lock before acquiring fragment locks
    drop(nwd);

    // Clean up any in-flight fragment state (FF-A spec: RXTX_UNMAP invalidates transfers)
    reset_nwd_frag_state();
    {
        let mut frag_rx = NWD_FRAG_RX.lock();
        frag_rx.active = false;
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
    if !NWD_RXTX.lock().mapped {
        return make_error(ffa::FFA_DENIED as u64);
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
        let (mapped, rx_pa, max_bytes) = {
            let nwd = NWD_RXTX.lock();
            let m = nwd.mapped;
            let r = nwd.rx_pa;
            let b = if m {
                nwd.page_count as usize * PAGE_SIZE_4KB as usize
            } else {
                0
            };
            (m, r, b)
        };

        crate::sp_context::for_each_sp(|sp| {
            let offset = count as usize * 24;
            if mapped && offset + 24 <= max_bytes {
                // SAFETY: `rx_pa` points to mapped NWd RX buffer and each 24-byte descriptor write is bounds-checked.
                unsafe {
                    let ptr = (rx_pa as *mut u8).add(offset);
                    // partition_id (u16 LE)
                    core::ptr::write_unaligned(ptr as *mut u16, sp.sp_id());
                    // exec_ctx_count (u16 LE)
                    core::ptr::write_unaligned(ptr.add(2) as *mut u16, 1);
                    // properties (u32 LE) — bit 0: DIRECT_REQ recv, bit 8: AARCH64_EXEC
                    // AARCH64_EXEC tells the Linux FF-A driver to use 64-bit
                    // DIRECT_REQ/RESP variants, matching our SP implementations.
                    core::ptr::write_unaligned(ptr.add(4) as *mut u32, 0x101);
                    // UUID (16 bytes) — read from SpContext
                    core::ptr::copy_nonoverlapping(sp.uuid().as_ptr() as *const u8, ptr.add(8), 16);
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
fn handle_direct_req(req: &SmcResult8) -> SmcResult8 {
    let is_64 = req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_64;
    let resp_fid = if is_64 {
        ffa::FFA_MSG_SEND_DIRECT_RESP_64
    } else {
        ffa::FFA_MSG_SEND_DIRECT_RESP_32
    };

    // Check for SPMD framework message (FFA_FWK_MSG_BIT set in x2, 32-bit only)
    if !is_64 && (req.x2 & ffa::FFA_FWK_MSG_BIT) != 0 {
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
        x0: resp_fid,
        x1: (dest << 16) | source,
        x2: 0,
        x3: req.x3,
        x4: req.x4,
        x5: req.x5,
        x6: req.x6,
        x7: req.x7,
    }
}

/// Handle FFA_MEM_FRAG_TX from NWd: continue a fragmented memory descriptor.
fn handle_spmc_mem_frag_tx(req: &SmcResult8) -> SmcResult8 {
    let handle = (req.x1 & 0xFFFF_FFFF) | ((req.x2 & 0xFFFF_FFFF) << 32);
    let fragment_length = req.x3 as u32;

    let mut frag = NWD_FRAG.lock();
    if !frag.active || frag.handle != handle {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    if fragment_length == 0 || frag.received + fragment_length > frag.total_length {
        frag.active = false;
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    // Copy fragment from NWd TX buffer
    #[cfg(feature = "sel2")]
    {
        let tx_pa = {
            let nwd = NWD_RXTX.lock();
            if !nwd.mapped {
                frag.active = false;
                return make_error(ffa::FFA_DENIED as u64);
            }
            nwd.tx_pa
        };
        if tx_pa < 0x4000_0000 || tx_pa >= 0xC000_0000 {
            frag.active = false;
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        // DSB SY: ensure NWd's fragment writes are visible (cross-CPU cache coherency)
        unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }
        unsafe {
            core::ptr::copy_nonoverlapping(
                tx_pa as *const u8,
                frag.accum_buf.as_mut_ptr().add(frag.received as usize),
                fragment_length as usize,
            );
        }
    }
    #[cfg(not(feature = "sel2"))]
    {
        // Unit test mode: no actual TX buffer to copy from
        // Tests will pre-fill the accumulation buffer directly
    }

    frag.received += fragment_length;

    if frag.received < frag.total_length {
        return SmcResult8 {
            x0: ffa::FFA_MEM_FRAG_RX,
            x1: handle & 0xFFFF_FFFF,
            x2: handle >> 32,
            x3: frag.received as u64,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
    }

    // All fragments received — parse while holding lock (avoids 4KB stack copy),
    // extract result, then release lock before acquiring SPMC_SHARES.
    let total_length = frag.total_length;
    let is_lend = frag.is_lend;
    let is_donate = frag.is_donate;
    let frag_handle = frag.handle;

    let parsed =
        unsafe { crate::ffa::descriptors::parse_mem_region(frag.accum_buf.as_ptr(), total_length) };
    frag.active = false;
    drop(frag); // Release NWD_FRAG lock before SPMC_SHARES lock

    let desc = match parsed {
        Ok(d) => d,
        Err(_) => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if desc.range_count == 0 || desc.range_count > MAX_SHARE_RANGES {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    // Validate receiver is a registered SP
    if !crate::sp_context::is_registered_sp(desc.receiver_id) {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    let count = desc.range_count;
    let mut records = SPMC_SHARES.lock();
    for record in records.iter_mut() {
        if !record.active {
            let mut stored = [(0u64, 0u32); MAX_SHARE_RANGES];
            for i in 0..count {
                stored[i] = desc.ranges[i];
            }
            *record = SpmcShareRecord {
                handle: frag_handle,
                sender_id: desc.sender_id,
                receiver_id: desc.receiver_id,
                ranges: stored,
                range_count: count,
                active: true,
                is_lend,
                is_donate,
                retrieved: false,
            };
            return SmcResult8 {
                x0: ffa::FFA_SUCCESS_32,
                x1: 0,
                x2: frag_handle & 0xFFFF_FFFF,
                x3: frag_handle >> 32,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            };
        }
    }
    make_error(ffa::FFA_NO_MEMORY as u64)
}

/// Handle FFA_MEM_SHARE / FFA_MEM_LEND from NWd.
///
/// In sel2 mode: reads FF-A v1.1 composite memory region descriptor from NWd TX buffer.
/// In unit tests (no sel2): uses register-based protocol (x3=IPA, x4=count, x5=receiver).
fn handle_spmc_mem_share(req: &SmcResult8, is_lend: bool, is_donate: bool) -> SmcResult8 {
    let sender_id: u16;
    let receiver_id: u16;
    let mut ranges = [(0u64, 0u32); MAX_SHARE_RANGES];
    let range_count: usize;

    #[cfg(feature = "sel2")]
    {
        let (mapped, tx_pa) = {
            let nwd = NWD_RXTX.lock();
            (nwd.mapped, nwd.tx_pa)
        };
        let total_length = req.x1 as u32;
        // Descriptor path: RXTX mapped AND total_length > 0 (x1=0 means register-based)
        if mapped && total_length > 0 {
            let fragment_length = req.x2 as u32;

            // Validate tx_pa is in S-EL2 Stage-1 accessible NS DRAM range
            if tx_pa < 0x4000_0000 || tx_pa >= 0xC000_0000 {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }

            // DSB SY: ensure NWd's TX buffer writes are visible to S-EL2.
            // pKVM's per-CPU SPMD may enter S-EL2 on a different physical CPU
            // than the one that wrote the descriptor — L1 D-cache can be stale.
            // SAFETY: DSB SY is a barrier instruction with no side effects.
            unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }

            // Fragmented: first fragment only — initiate reassembly
            if total_length != fragment_length && fragment_length > 0 && total_length > 0 {
                if total_length > 4096 || fragment_length > total_length {
                    return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
                }
                let mut frag = NWD_FRAG.lock();
                if frag.active {
                    return make_error(ffa::FFA_BUSY as u64);
                }
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        tx_pa as *const u8,
                        frag.accum_buf.as_mut_ptr(),
                        fragment_length as usize,
                    );
                }
                frag.total_length = total_length;
                frag.received = fragment_length;
                frag.handle = spmc_alloc_handle();
                frag.is_lend = is_lend;
                frag.is_donate = is_donate;
                // Extract sender_id from FfaMemRegion header (offset 2, u16)
                frag.sender_id = if fragment_length >= 4 {
                    unsafe {
                        core::ptr::read_unaligned(frag.accum_buf.as_ptr().add(2) as *const u16)
                    }
                } else {
                    0
                };
                frag.active = true;
                let h = frag.handle;
                return SmcResult8 {
                    x0: ffa::FFA_MEM_FRAG_RX,
                    x1: h & 0xFFFF_FFFF,
                    x2: h >> 32,
                    x3: fragment_length as u64,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                };
            }

            // Non-fragmented: copy descriptor to local buffer first, then parse.
            // Direct reads from NWd TX buffer are unreliable on multi-CPU pKVM
            // (SPMD enters S-EL2 on different CPU than the one that wrote the descriptor).
            let tlen = total_length as usize;
            if tlen > 4096 {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }
            let mut local_buf = [0u8; 4096];
            unsafe {
                core::ptr::copy_nonoverlapping(tx_pa as *const u8, local_buf.as_mut_ptr(), tlen);
            }
            let parsed = unsafe {
                crate::ffa::descriptors::parse_mem_region(local_buf.as_ptr(), total_length)
            };
            match parsed {
                Ok(desc) => {
                    sender_id = desc.sender_id;
                    receiver_id = desc.receiver_id;
                    if desc.range_count == 0 || desc.range_count > MAX_SHARE_RANGES {
                        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
                    }
                    let count = desc.range_count;
                    for i in 0..count {
                        ranges[i] = desc.ranges[i];
                    }
                    range_count = count;
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
            if count == 0 {
                return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
            }
            ranges[0] = (ipa, count);
            range_count = 1;
        }
    }

    #[cfg(not(feature = "sel2"))]
    {
        // Register-based protocol for unit tests
        sender_id = ((req.x1 >> 16) & 0xFFFF) as u16;
        receiver_id = (req.x5 & 0xFFFF) as u16;
        let ipa = req.x3;
        let count = req.x4 as u32;
        if count == 0 {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        ranges[0] = (ipa, count);
        range_count = 1;
    }

    // Validate receiver is a registered SP
    if !crate::sp_context::is_registered_sp(receiver_id) {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    // Validate page counts and IPA alignment per range
    for i in 0..range_count {
        let (ipa, page_count) = ranges[i];
        // IPA must be 4KB-aligned
        if ipa & 0xFFF != 0 {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        // Reasonable page count limit (256MB / 4KB = 65536 pages max per range)
        if page_count > 65536 {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        // Check for overflow: ipa + page_count * 4096 must not wrap
        if (page_count as u64).checked_mul(PAGE_SIZE_4KB).is_none() {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
        if ipa.checked_add(page_count as u64 * PAGE_SIZE_4KB).is_none() {
            return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
        }
    }

    match record_spmc_share(
        sender_id,
        receiver_id,
        &ranges[..range_count],
        is_lend,
        is_donate,
    ) {
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
fn handle_spmc_mem_retrieve(req: &SmcResult8, _current_sp: Option<(u16, u64)>) -> SmcResult8 {
    // Determine handle: descriptor-based (NWd TX buffer) or register-based.
    // SP-initiated RETRIEVE uses register-based (x1=handle_lo, x2=handle_hi).
    // NWd RETRIEVE (pKVM reclaim path) uses descriptor-based (handle in TX buffer).
    let handle;
    #[cfg(feature = "sel2")]
    {
        if _current_sp.is_some() {
            // SP-initiated: register-based
            handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);
        } else {
            // NWd-initiated: try descriptor-based (TX buffer has FfaMemRegion with handle at offset 8)
            let total_length = req.x1 as u32;
            let (mapped, tx_pa) = {
                let nwd = NWD_RXTX.lock();
                (nwd.mapped, nwd.tx_pa)
            };
            if mapped && total_length > 0 {
                // DSB SY + local copy: NWd TX buffer may be stale on multi-CPU pKVM
                unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }
                let mut h_buf = [0u8; 8];
                unsafe {
                    core::ptr::copy_nonoverlapping((tx_pa + 8) as *const u8, h_buf.as_mut_ptr(), 8);
                }
                handle = u64::from_le_bytes(h_buf);
            } else {
                handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);
            }
        }
    }
    #[cfg(not(feature = "sel2"))]
    {
        handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);
    }

    let (_sender, receiver_id, ranges, range_count, _, _is_donate, retrieved) =
        match lookup_spmc_share(handle) {
            Some(info) => info,
            None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
        };

    // NWd RETRIEVE_REQ (pKVM reclaim path, sel2 only): _current_sp is None.
    // Only return descriptor — don't map pages or mark retrieved.
    // SP-initiated RETRIEVE or non-sel2: map pages and mark retrieved.
    #[cfg(feature = "sel2")]
    let is_nwd_retrieve = _current_sp.is_none();
    #[cfg(not(feature = "sel2"))]
    let is_nwd_retrieve = false;

    if !is_nwd_retrieve {
        // Validate caller is the authorized receiver (isolation: SP1 cannot retrieve SP2's share)
        if let Some((current_sp_id, _)) = _current_sp {
            if current_sp_id != receiver_id {
                return make_error(ffa::FFA_DENIED as u64);
            }
        }

        if retrieved {
            return make_error(ffa::FFA_DENIED as u64);
        }

        // In sel2 mode, map pages into the receiver SP's Secure Stage-2.
        // STAGE2_LOCK serializes all page table walks to prevent TOCTOU races
        // when two CPUs concurrently allocate L2/L3 tables via map_page().
        #[cfg(feature = "sel2")]
        {
            let _s2guard = STAGE2_LOCK.lock();
            let mut mapped = false;
            if let Some((current_sp_id, current_vsttbr)) = _current_sp {
                if current_sp_id == receiver_id {
                    let vsttbr = current_vsttbr;
                    let l0_addr = vsttbr & 0x0000_FFFF_FFFF_F000;
                    let walker = crate::ffa::stage2_walker::Stage2Walker::new(l0_addr);
                    for i in 0..range_count {
                        let (base_ipa, page_count) = ranges[i];
                        for p in 0..page_count as u64 {
                            let ipa = base_ipa + p * PAGE_SIZE_4KB;
                            walker.map_page(ipa, 0b11, 0b10); // S2AP_RW, SW=SHARED_BORROWED
                        }
                    }
                    mapped = true;
                }
            }

            if !mapped {
                let ok = crate::sp_context::with_sp_locked(receiver_id, |sp| {
                    let vsttbr = sp.vsttbr();
                    let l0_addr = vsttbr & 0x0000_FFFF_FFFF_F000;
                    let walker = crate::ffa::stage2_walker::Stage2Walker::new(l0_addr);
                    for i in 0..range_count {
                        let (base_ipa, page_count) = ranges[i];
                        for p in 0..page_count as u64 {
                            let ipa = base_ipa + p * PAGE_SIZE_4KB;
                            walker.map_page(ipa, 0b11, 0b10); // S2AP_RW, SW=SHARED_BORROWED
                        }
                    }
                });
                if ok.is_none() {
                    return make_error(ffa::FFA_BUSY as u64);
                }
            }
        }
        let _ = (receiver_id, &ranges, range_count);

        mark_spmc_retrieved(handle);
    }

    // Build response descriptor and compute total_length
    let total_pages: u32 = ranges[..range_count].iter().map(|(_, c)| *c).sum();
    let mut frag_rx = NWD_FRAG_RX.lock();
    let total_length = match unsafe {
        crate::ffa::descriptors::build_retrieve_resp_descriptor(
            frag_rx.resp_buf.as_mut_ptr(),
            frag_rx.resp_buf.len(),
            _sender,
            receiver_id,
            handle,
            &ranges,
            range_count,
            total_pages,
        )
    } {
        Ok(len) => {
            // Write descriptor to NWd RX buffer if available
            #[cfg(feature = "sel2")]
            {
                let nwd = NWD_RXTX.lock();
                if nwd.mapped {
                    let chunk = (len as usize).min(4096);
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            frag_rx.resp_buf.as_ptr(),
                            nwd.rx_pa as *mut u8,
                            chunk,
                        );
                    }
                    if len > 4096 {
                        frag_rx.active = true;
                        frag_rx.total_length = len;
                        frag_rx.delivered = 4096;
                        frag_rx.handle = handle;
                    }
                }
            }
            len as u64
        }
        Err(_) => 0,
    };
    drop(frag_rx);

    // FF-A spec: x1=total_length, x2=fragment_length (not handle)
    let fragment_length = total_length.min(4096);
    SmcResult8 {
        x0: ffa::FFA_MEM_RETRIEVE_RESP,
        x1: total_length,
        x2: fragment_length,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle FFA_MEM_FRAG_RX from NWd: deliver next fragment of MEM_RETRIEVE_RESP.
fn handle_spmc_mem_frag_rx(req: &SmcResult8) -> SmcResult8 {
    let handle = (req.x1 & 0xFFFF_FFFF) | ((req.x2 & 0xFFFF_FFFF) << 32);
    let frag_offset = req.x3 as u32;

    let mut frag_rx = NWD_FRAG_RX.lock();
    if !frag_rx.active || frag_rx.handle != handle {
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    if frag_offset != frag_rx.delivered {
        frag_rx.active = false;
        return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
    }

    let remaining = frag_rx.total_length - frag_rx.delivered;
    let chunk = (remaining as usize).min(4096);

    #[cfg(feature = "sel2")]
    {
        let nwd = NWD_RXTX.lock();
        if !nwd.mapped {
            frag_rx.active = false;
            return make_error(ffa::FFA_DENIED as u64);
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                frag_rx.resp_buf.as_ptr().add(frag_rx.delivered as usize),
                nwd.rx_pa as *mut u8,
                chunk,
            );
        }
    }

    frag_rx.delivered += chunk as u32;

    if frag_rx.delivered < frag_rx.total_length {
        SmcResult8 {
            x0: ffa::FFA_MEM_FRAG_RX,
            x1: handle & 0xFFFF_FFFF,
            x2: handle >> 32,
            x3: frag_rx.delivered as u64,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        }
    } else {
        frag_rx.active = false;
        SmcResult8 {
            x0: ffa::FFA_SUCCESS_32,
            x1: 0,
            x2: handle & 0xFFFF_FFFF,
            x3: handle >> 32,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        }
    }
}

/// Handle FFA_MEM_RELINQUISH — unmaps pages from receiver SP's Secure Stage-2.
fn handle_spmc_mem_relinquish(req: &SmcResult8, _current_sp: Option<(u16, u64)>) -> SmcResult8 {
    let handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);

    let (_sender, receiver_id, ranges, range_count, _, is_donate, retrieved) =
        match lookup_spmc_share(handle) {
            Some(info) => info,
            None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
        };

    // Donated pages belong permanently to receiver — RELINQUISH is not valid
    if is_donate {
        return make_error(ffa::FFA_DENIED as u64);
    }

    // Validate caller is the authorized receiver (isolation: SP1 cannot relinquish SP2's share)
    if let Some((current_sp_id, _)) = _current_sp {
        if current_sp_id != receiver_id {
            return make_error(ffa::FFA_DENIED as u64);
        }
    }

    if !retrieved {
        return make_error(ffa::FFA_DENIED as u64);
    }

    // In sel2 mode, unmap pages from the receiver SP's Secure Stage-2.
    // STAGE2_LOCK serializes page table modifications across CPUs.
    #[cfg(feature = "sel2")]
    {
        let _s2guard = STAGE2_LOCK.lock();
        let mut unmapped = false;
        if let Some((current_sp_id, current_vsttbr)) = _current_sp {
            if current_sp_id == receiver_id {
                let vsttbr = current_vsttbr;
                let l0_addr = vsttbr & 0x0000_FFFF_FFFF_F000;
                let walker = crate::ffa::stage2_walker::Stage2Walker::new(l0_addr);
                for i in 0..range_count {
                    let (base_ipa, page_count) = ranges[i];
                    for p in 0..page_count as u64 {
                        let ipa = base_ipa + p * PAGE_SIZE_4KB;
                        walker.unmap_page(ipa);
                    }
                }
                unmapped = true;
            }
        }

        if !unmapped {
            let ok = crate::sp_context::with_sp_locked(receiver_id, |sp| {
                let vsttbr = sp.vsttbr();
                let l0_addr = vsttbr & 0x0000_FFFF_FFFF_F000;
                let walker = crate::ffa::stage2_walker::Stage2Walker::new(l0_addr);
                for i in 0..range_count {
                    let (base_ipa, page_count) = ranges[i];
                    for p in 0..page_count as u64 {
                        let ipa = base_ipa + p * PAGE_SIZE_4KB;
                        walker.unmap_page(ipa);
                    }
                }
            });
            if ok.is_none() {
                return make_error(ffa::FFA_BUSY as u64);
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

/// Validate and record an SP-initiated MEM_SHARE/LEND/DONATE.
/// Returns Ok(handle) on success, Err(error_code) on validation failure.
fn validate_sp_share(
    sp_id: u16,
    x1: u64,
    x3: u64,
    x4: u64,
    x5: u64,
    is_lend: bool,
    is_donate: bool,
) -> Result<u64, u64> {
    let sender_id = ((x1 >> 16) & 0xFFFF) as u16;
    let receiver_id = (x5 & 0xFFFF) as u16;
    if sender_id != sp_id {
        return Err(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    if receiver_id == sp_id {
        return Err(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    if !crate::sp_context::is_registered_sp(receiver_id) {
        return Err(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    let ipa = x3;
    let page_count = x4 as u32;
    if page_count == 0 || page_count > 65536 || (ipa & 0xFFF) != 0 {
        return Err(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    if (page_count as u64).checked_mul(PAGE_SIZE_4KB).is_none() {
        return Err(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    if ipa.checked_add(page_count as u64 * PAGE_SIZE_4KB).is_none() {
        return Err(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    let ranges = [(ipa, page_count)];
    record_spmc_share(sp_id, receiver_id, &ranges, is_lend, is_donate)
        .ok_or(ffa::FFA_NO_MEMORY as u64)
}

/// Validate and execute an SP-initiated MEM_RECLAIM.
/// Returns Ok(()) on success, Err(error_code) on failure.
fn validate_sp_reclaim(sp_id: u16, x1: u64, x2: u64) -> Result<(), u64> {
    let handle = (x1 & 0xFFFF_FFFF) | (x2 << 32);
    match lookup_spmc_share(handle) {
        Some((sender, _, _, _, _, _, _)) if sender == sp_id => {
            reclaim_spmc_share(handle).map_err(|code| code as u64)
        }
        Some(_) => Err(ffa::FFA_DENIED as u64),
        None => Err(ffa::FFA_INVALID_PARAMETERS as u64),
    }
}

/// Build an FFA_SUCCESS response with all-zero payload.
fn make_success() -> SmcResult8 {
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
