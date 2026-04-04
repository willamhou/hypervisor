//! Secure Partition context management.
//!
//! Each SP has an `SpContext` that holds its register state (via `VcpuContext`)
//! and a state machine tracking its lifecycle.

use crate::arch::aarch64::defs::SPSR_EL1H_DAIF_MASKED;
use crate::arch::aarch64::regs::VcpuContext;
use crate::sync::SpinLock;
use core::arch::asm;
use core::cell::UnsafeCell;
use core::convert::TryFrom;
use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};

/// Lightweight EL1 system register state for Secure Partitions.
///
/// Unlike `VcpuArchState` (designed for NS-EL2 managing full Linux guests),
/// this only saves/restores the EL1 sysregs that SPs need. It does NOT touch
/// GIC virtual interface (ICH_LR/VMCR/HCR), timers, VMPIDR, or PAC keys,
/// which are managed by the SPMC itself at S-EL2.
pub struct SpEl1State {
    pub sctlr_el1: u64,
    pub ttbr0_el1: u64,
    pub ttbr1_el1: u64,
    pub tcr_el1: u64,
    pub mair_el1: u64,
    pub vbar_el1: u64,
    pub cpacr_el1: u64,
    pub contextidr_el1: u64,
    pub tpidr_el1: u64,
    pub tpidrro_el0: u64,
    pub tpidr_el0: u64,
    pub par_el1: u64,
    pub cntkctl_el1: u64,
    pub sp_el1: u64,
    pub sp_el0: u64,
    pub afsr0_el1: u64,
    pub afsr1_el1: u64,
    pub amair_el1: u64,
    pub mdscr_el1: u64,
}

impl Default for SpEl1State {
    fn default() -> Self {
        Self::new()
    }
}

impl SpEl1State {
    pub const fn new() -> Self {
        Self {
            sctlr_el1: 0,
            ttbr0_el1: 0,
            ttbr1_el1: 0,
            tcr_el1: 0,
            mair_el1: 0,
            vbar_el1: 0,
            cpacr_el1: 0,
            contextidr_el1: 0,
            tpidr_el1: 0,
            tpidrro_el0: 0,
            tpidr_el0: 0,
            par_el1: 0,
            cntkctl_el1: 0,
            sp_el1: 0,
            sp_el0: 0,
            afsr0_el1: 0,
            afsr1_el1: 0,
            amair_el1: 0,
            mdscr_el1: 0,
        }
    }

    /// Save EL1 system registers from hardware.
    pub fn save(&mut self) {
        // SAFETY: executes at EL2 and accesses architected EL1 sysregs of the
        // currently running world; caller serializes save/restore ordering.
        unsafe {
            asm!("mrs {}, sctlr_el1", out(reg) self.sctlr_el1, options(nostack, nomem));
            asm!("mrs {}, ttbr0_el1", out(reg) self.ttbr0_el1, options(nostack, nomem));
            asm!("mrs {}, ttbr1_el1", out(reg) self.ttbr1_el1, options(nostack, nomem));
            asm!("mrs {}, tcr_el1", out(reg) self.tcr_el1, options(nostack, nomem));
            asm!("mrs {}, mair_el1", out(reg) self.mair_el1, options(nostack, nomem));
            asm!("mrs {}, vbar_el1", out(reg) self.vbar_el1, options(nostack, nomem));
            asm!("mrs {}, cpacr_el1", out(reg) self.cpacr_el1, options(nostack, nomem));
            asm!("mrs {}, contextidr_el1", out(reg) self.contextidr_el1, options(nostack, nomem));
            asm!("mrs {}, tpidr_el1", out(reg) self.tpidr_el1, options(nostack, nomem));
            asm!("mrs {}, tpidrro_el0", out(reg) self.tpidrro_el0, options(nostack, nomem));
            asm!("mrs {}, tpidr_el0", out(reg) self.tpidr_el0, options(nostack, nomem));
            asm!("mrs {}, par_el1", out(reg) self.par_el1, options(nostack, nomem));
            asm!("mrs {}, cntkctl_el1", out(reg) self.cntkctl_el1, options(nostack, nomem));
            asm!("mrs {}, sp_el1", out(reg) self.sp_el1, options(nostack, nomem));
            asm!("mrs {}, sp_el0", out(reg) self.sp_el0, options(nostack, nomem));
            asm!("mrs {}, afsr0_el1", out(reg) self.afsr0_el1, options(nostack, nomem));
            asm!("mrs {}, afsr1_el1", out(reg) self.afsr1_el1, options(nostack, nomem));
            asm!("mrs {}, amair_el1", out(reg) self.amair_el1, options(nostack, nomem));
            asm!("mrs {}, mdscr_el1", out(reg) self.mdscr_el1, options(nostack, nomem));
        }
    }

    /// Restore EL1 system registers to hardware.
    pub fn restore(&self) {
        // SAFETY: writes back values previously captured by `save()` for the
        // same execution context before re-entering that context.
        unsafe {
            asm!("msr sctlr_el1, {}", in(reg) self.sctlr_el1, options(nostack, nomem));
            asm!("msr ttbr0_el1, {}", in(reg) self.ttbr0_el1, options(nostack, nomem));
            asm!("msr ttbr1_el1, {}", in(reg) self.ttbr1_el1, options(nostack, nomem));
            asm!("msr tcr_el1, {}", in(reg) self.tcr_el1, options(nostack, nomem));
            asm!("msr mair_el1, {}", in(reg) self.mair_el1, options(nostack, nomem));
            asm!("msr vbar_el1, {}", in(reg) self.vbar_el1, options(nostack, nomem));
            asm!("msr cpacr_el1, {}", in(reg) self.cpacr_el1, options(nostack, nomem));
            asm!("msr contextidr_el1, {}", in(reg) self.contextidr_el1, options(nostack, nomem));
            asm!("msr tpidr_el1, {}", in(reg) self.tpidr_el1, options(nostack, nomem));
            asm!("msr tpidrro_el0, {}", in(reg) self.tpidrro_el0, options(nostack, nomem));
            asm!("msr tpidr_el0, {}", in(reg) self.tpidr_el0, options(nostack, nomem));
            asm!("msr par_el1, {}", in(reg) self.par_el1, options(nostack, nomem));
            asm!("msr cntkctl_el1, {}", in(reg) self.cntkctl_el1, options(nostack, nomem));
            asm!("msr sp_el1, {}", in(reg) self.sp_el1, options(nostack, nomem));
            asm!("msr sp_el0, {}", in(reg) self.sp_el0, options(nostack, nomem));
            asm!("msr afsr0_el1, {}", in(reg) self.afsr0_el1, options(nostack, nomem));
            asm!("msr afsr1_el1, {}", in(reg) self.afsr1_el1, options(nostack, nomem));
            asm!("msr amair_el1, {}", in(reg) self.amair_el1, options(nostack, nomem));
            asm!("msr mdscr_el1, {}", in(reg) self.mdscr_el1, options(nostack, nomem));
            asm!("isb", options(nostack, nomem));
        }
    }

    /// Restore EL1 system registers to hardware, EXCEPT SP_EL0.
    ///
    /// At S-EL2, the SPMC uses SP_EL0 as its current stack pointer (SPSel=0).
    /// Restoring NWd's SP_EL0 during SPMC execution would corrupt the SPMC's
    /// stack. SP_EL1 is safe to restore (it's EL1's banked SP, not our stack).
    ///
    /// Used by the SPMC event loop to restore NWd EL1 state before forwarding
    /// responses to SPMD. SPMD does NOT save/restore EL1 when SPMD_SPM_AT_SEL2=1
    /// (it only saves/restores EL2), so the SPMC must manage EL1 state itself.
    pub fn restore_except_sp_el0(&self) {
        // SAFETY: same preconditions as `restore()`, except SP_EL0 is
        // intentionally skipped to preserve the SPMC's current stack.
        unsafe {
            asm!("msr sctlr_el1, {}", in(reg) self.sctlr_el1, options(nostack, nomem));
            asm!("msr ttbr0_el1, {}", in(reg) self.ttbr0_el1, options(nostack, nomem));
            asm!("msr ttbr1_el1, {}", in(reg) self.ttbr1_el1, options(nostack, nomem));
            asm!("msr tcr_el1, {}", in(reg) self.tcr_el1, options(nostack, nomem));
            asm!("msr mair_el1, {}", in(reg) self.mair_el1, options(nostack, nomem));
            asm!("msr vbar_el1, {}", in(reg) self.vbar_el1, options(nostack, nomem));
            asm!("msr cpacr_el1, {}", in(reg) self.cpacr_el1, options(nostack, nomem));
            asm!("msr contextidr_el1, {}", in(reg) self.contextidr_el1, options(nostack, nomem));
            asm!("msr tpidr_el1, {}", in(reg) self.tpidr_el1, options(nostack, nomem));
            asm!("msr tpidrro_el0, {}", in(reg) self.tpidrro_el0, options(nostack, nomem));
            asm!("msr tpidr_el0, {}", in(reg) self.tpidr_el0, options(nostack, nomem));
            asm!("msr par_el1, {}", in(reg) self.par_el1, options(nostack, nomem));
            asm!("msr cntkctl_el1, {}", in(reg) self.cntkctl_el1, options(nostack, nomem));
            asm!("msr sp_el1, {}", in(reg) self.sp_el1, options(nostack, nomem));
            // Skip SP_EL0 — SPMC uses it as current stack pointer at S-EL2
            asm!("msr afsr0_el1, {}", in(reg) self.afsr0_el1, options(nostack, nomem));
            asm!("msr afsr1_el1, {}", in(reg) self.afsr1_el1, options(nostack, nomem));
            asm!("msr amair_el1, {}", in(reg) self.amair_el1, options(nostack, nomem));
            asm!("msr mdscr_el1, {}", in(reg) self.mdscr_el1, options(nostack, nomem));
            asm!("isb", options(nostack, nomem));
        }
    }
}

/// SP lifecycle states.
///
/// Represented as u8 for atomic operations — multi-CPU SPMD means multiple
/// CPUs can simultaneously try to dispatch to the same SP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SpState {
    /// SP has been loaded but not yet booted.
    Reset = 0,
    /// SP has booted and is waiting for a message (called FFA_MSG_WAIT).
    Idle = 1,
    /// SP is currently executing (SPMC ERETs to it).
    Running = 2,
    /// SP is blocked waiting for an event.
    Blocked = 3,
    /// SP was preempted by NS interrupt, resume via FFA_RUN.
    Preempted = 4,
}

impl SpState {
    fn try_from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(SpState::Reset),
            1 => Some(SpState::Idle),
            2 => Some(SpState::Running),
            3 => Some(SpState::Blocked),
            4 => Some(SpState::Preempted),
            _ => None,
        }
    }
}

impl TryFrom<u8> for SpState {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        SpState::try_from_u8(value).ok_or(())
    }
}

const NO_PENDING_IRQ: u32 = u32::MAX;
const PENDING_IRQ_SLOTS: usize = 4;
const NO_PREEMPTED_CPU: u16 = u16::MAX;
const NO_OWNER_CPU: u16 = u16::MAX;

/// Per-SP context: register state + metadata.
pub struct SpContext {
    /// Register context passed to enter_guest() for ERET.
    ctx: VcpuContext,
    /// EL1 system registers saved/restored around enter_guest() calls.
    /// Without this, world switches (e.g. pKVM at NS-EL2) corrupt S-EL1 state.
    el1_state: SpEl1State,
    /// FF-A partition ID (e.g. 0x8001).
    id: u16,
    /// Current lifecycle state (atomic for multi-CPU safety).
    state: AtomicU8,
    /// Cold boot entry point.
    entry: u64,
    /// Secure Stage-2 VSTTBR value for this SP (set after page table creation).
    vsttbr: u64,
    /// 128-bit UUID from SP manifest (4 x u32 LE words).
    uuid: [u32; 4],
    /// Pending virtual interrupts to inject via HCR_EL2.VI.
    /// Atomic slots so IRQ/HVC paths can update without taking a mutable SP borrow.
    pending_irqs: [AtomicU32; PENDING_IRQ_SLOTS],
    /// CPU that last preempted this SP. Used to enforce same-CPU FFA_RUN policy.
    preempted_cpu: core::sync::atomic::AtomicU16,
    /// Current logical owner CPU for this SP context.
    owner_cpu: core::sync::atomic::AtomicU16,
    /// INTIDs owned by this SP, delivered as virtual IRQ (up to 4, 0 = unused).
    owned_intids: [u32; 4],
}

impl SpContext {
    /// Create a new SP context in Reset state.
    pub fn new(sp_id: u16, entry_point: u64, stack_top: u64, uuid: [u32; 4]) -> Self {
        use crate::arch::aarch64::regs::SystemRegs;
        let ctx = VcpuContext {
            pc: entry_point,
            sp: stack_top,
            spsr_el2: SPSR_EL1H_DAIF_MASKED,
            sys_regs: SystemRegs {
                sp_el1: stack_top,
                ..SystemRegs::default()
            },
            ..VcpuContext::default()
        };

        Self {
            ctx,
            el1_state: SpEl1State::new(),
            id: sp_id,
            state: AtomicU8::new(SpState::Reset as u8),
            entry: entry_point,
            vsttbr: 0,
            uuid,
            pending_irqs: {
                #[allow(clippy::declare_interior_mutable_const)]
                const EMPTY: AtomicU32 = AtomicU32::new(NO_PENDING_IRQ);
                [EMPTY; PENDING_IRQ_SLOTS]
            },
            preempted_cpu: core::sync::atomic::AtomicU16::new(NO_PREEMPTED_CPU),
            owner_cpu: core::sync::atomic::AtomicU16::new(NO_OWNER_CPU),
            owned_intids: [0; 4],
        }
    }

    pub fn sp_id(&self) -> u16 {
        self.id
    }

    pub fn state(&self) -> SpState {
        SpState::try_from(self.state.load(Ordering::Acquire)).expect("corrupt SP state value")
    }

    pub fn entry_point(&self) -> u64 {
        self.entry
    }

    pub fn vsttbr(&self) -> u64 {
        self.vsttbr
    }

    pub fn uuid(&self) -> &[u32; 4] {
        &self.uuid
    }

    pub fn set_vsttbr(&mut self, vsttbr: u64) {
        self.vsttbr = vsttbr;
    }

    /// Get immutable reference to the VcpuContext.
    pub fn vcpu_ctx(&self) -> &VcpuContext {
        &self.ctx
    }

    /// Get mutable reference to the VcpuContext (for enter_guest).
    pub fn vcpu_ctx_mut(&mut self) -> &mut VcpuContext {
        &mut self.ctx
    }

    /// Save EL1 system registers from hardware into this SP's state.
    /// Must be called after enter_guest() returns (SP trapped back to S-EL2).
    pub fn save_el1_state(&mut self) {
        self.el1_state.save();
    }

    /// Restore EL1 system registers from this SP's state to hardware.
    /// Must be called before enter_guest() so the SP runs with correct sysregs.
    pub fn restore_el1_state(&self) {
        self.el1_state.restore();
    }

    /// Validate and perform a state transition (non-atomic store).
    /// Use when the caller already holds exclusive access (e.g., after CAS lock).
    pub fn transition_to(&mut self, new_state: SpState) -> Result<(), &'static str> {
        let current = self.state();
        let valid = match (current, new_state) {
            (SpState::Reset, SpState::Idle) => true,
            (SpState::Idle, SpState::Running) => true,
            (SpState::Running, SpState::Idle) => true,
            (SpState::Running, SpState::Blocked) => true,
            (SpState::Blocked, SpState::Running) => true,
            (SpState::Blocked, SpState::Preempted) => true, // chain preemption (SP-to-SP)
            (SpState::Running, SpState::Preempted) => true,
            (SpState::Preempted, SpState::Running) => true,
            _ => false,
        };
        if valid {
            self.state.store(new_state as u8, Ordering::Release);
            Ok(())
        } else {
            Err("invalid SP state transition")
        }
    }

    /// Atomically transition from expected_state to new_state.
    /// Returns Ok(()) if the CAS succeeded, Err if the SP was not in expected_state.
    /// Used by multi-CPU dispatch to prevent two CPUs from entering the same SP.
    pub fn try_transition(&self, expected: SpState, new_state: SpState) -> Result<(), SpState> {
        match self.state.compare_exchange(
            expected as u8,
            new_state as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(actual) => Err(SpState::try_from(actual).expect("corrupt SP state value")),
        }
    }

    /// Set x0-x7 in the context (for passing DIRECT_REQ args before ERET).
    pub fn set_args(
        &mut self,
        x0: u64,
        x1: u64,
        x2: u64,
        x3: u64,
        x4: u64,
        x5: u64,
        x6: u64,
        x7: u64,
    ) {
        self.ctx.gp_regs.x0 = x0;
        self.ctx.gp_regs.x1 = x1;
        self.ctx.gp_regs.x2 = x2;
        self.ctx.gp_regs.x3 = x3;
        self.ctx.gp_regs.x4 = x4;
        self.ctx.gp_regs.x5 = x5;
        self.ctx.gp_regs.x6 = x6;
        self.ctx.gp_regs.x7 = x7;
    }

    /// Read x0-x7 from the context (after SP traps back with DIRECT_RESP).
    pub fn get_args(&self) -> (u64, u64, u64, u64, u64, u64, u64, u64) {
        (
            self.ctx.gp_regs.x0,
            self.ctx.gp_regs.x1,
            self.ctx.gp_regs.x2,
            self.ctx.gp_regs.x3,
            self.ctx.gp_regs.x4,
            self.ctx.gp_regs.x5,
            self.ctx.gp_regs.x6,
            self.ctx.gp_regs.x7,
        )
    }

    /// Set the INTID ownership array for this SP.
    pub fn set_owned_intids(&mut self, intids: [u32; 4]) {
        self.owned_intids = intids;
    }

    /// Check if this SP owns the given INTID.
    pub fn owns_intid(&self, intid: u32) -> bool {
        self.owned_intids.iter().any(|&id| id != 0 && id == intid)
    }

    /// Set a pending virtual interrupt for injection on next SP entry.
    pub fn set_pending_irq(&self, intid: u32) {
        // Deduplicate first to avoid filling slots with repeated INTIDs.
        for slot in self.pending_irqs.iter() {
            if slot.load(Ordering::Acquire) == intid {
                return;
            }
        }

        for slot in self.pending_irqs.iter() {
            if slot
                .compare_exchange(NO_PENDING_IRQ, intid, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return;
            }
        }

        crate::log_warn!("[SPMC] pending IRQ slots full, dropping intid={}\n", intid);
    }

    /// Take the pending virtual interrupt (returns None if none pending).
    pub fn take_pending_irq(&self) -> Option<u32> {
        for slot in self.pending_irqs.iter() {
            let intid = slot.swap(NO_PENDING_IRQ, Ordering::AcqRel);
            if intid != NO_PENDING_IRQ {
                return Some(intid);
            }
        }
        None
    }

    /// Check if this SP has a pending interrupt.
    pub fn has_pending_irq(&self) -> bool {
        self.pending_irqs
            .iter()
            .any(|slot| slot.load(Ordering::Acquire) != NO_PENDING_IRQ)
    }

    /// Record which CPU preempted this SP most recently.
    pub fn set_preempted_cpu(&self, cpu: usize) {
        self.preempted_cpu.store(cpu as u16, Ordering::Release);
    }

    /// Return the CPU that preempted this SP, if any.
    pub fn preempted_cpu(&self) -> Option<usize> {
        let cpu = self.preempted_cpu.load(Ordering::Acquire);
        if cpu == NO_PREEMPTED_CPU {
            None
        } else {
            Some(cpu as usize)
        }
    }

    /// Clear recorded preempted CPU ownership.
    pub fn clear_preempted_cpu(&self) {
        self.preempted_cpu
            .store(NO_PREEMPTED_CPU, Ordering::Release);
    }

    /// Return current owner CPU of this SP context, if any.
    pub fn owner_cpu(&self) -> Option<usize> {
        let cpu = self.owner_cpu.load(Ordering::Acquire);
        if cpu == NO_OWNER_CPU {
            None
        } else {
            Some(cpu as usize)
        }
    }

    /// Try to claim owner when there is no current owner.
    pub fn try_claim_owner_cpu(&self, cpu: usize) -> bool {
        self.owner_cpu
            .compare_exchange(
                NO_OWNER_CPU,
                cpu as u16,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Try to migrate ownership from one CPU to another.
    pub fn try_migrate_owner_cpu(&self, from_cpu: usize, to_cpu: usize) -> bool {
        self.owner_cpu
            .compare_exchange(
                from_cpu as u16,
                to_cpu as u16,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Clear owner CPU when SP is no longer running/preempted.
    pub fn clear_owner_cpu(&self) {
        self.owner_cpu.store(NO_OWNER_CPU, Ordering::Release);
    }

    /// Return the first non-zero owned INTID, if any.
    pub fn first_owned_intid(&self) -> Option<u32> {
        self.owned_intids.iter().copied().find(|&id| id != 0)
    }
}

// ── Global SP store ─────────────────────────────────────────────────

const MAX_SPS: usize = 4;

struct SpStore {
    contexts: UnsafeCell<[Option<SpContext>; MAX_SPS]>,
}

// SAFETY: all access to `contexts` is serialized by `SP_STORE_LOCK`; no
// concurrent mutable aliases are created across callers.
unsafe impl Sync for SpStore {}

static SP_STORE: SpStore = SpStore {
    contexts: UnsafeCell::new([None, None, None, None]),
};
static SP_STORE_LOCK: SpinLock<()> = SpinLock::new(());

/// Per-SP dispatch lock. Prevents two CPUs from simultaneously entering
/// mutable SP dispatch paths for the same SP, which would create aliasing
/// &mut references.
/// Index matches SP_STORE slot (not SP ID). Locked before mutable access,
/// unlocked after dispatch completes.
use core::sync::atomic::AtomicBool;
static SP_DISPATCH_LOCK: [AtomicBool; MAX_SPS] = [
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
    AtomicBool::new(false),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpLockError {
    NotFound,
    Busy,
}

pub struct SpDispatchGuard {
    slot: usize,
}

impl SpDispatchGuard {
    pub fn sp_mut(&mut self) -> &mut SpContext {
        // SAFETY: per-slot dispatch lock guarantees exclusive access for this
        // slot. Use raw-pointer indexing to avoid creating `&mut` to the whole
        // array (which would alias with other slots on other CPUs).
        let slot = unsafe {
            let base = (*SP_STORE.contexts.get()).as_mut_ptr();
            &mut *base.add(self.slot)
        };
        slot.as_mut()
            .expect("SP slot empty while dispatch guard held")
    }
}

impl Drop for SpDispatchGuard {
    fn drop(&mut self) {
        SP_DISPATCH_LOCK[self.slot].store(false, Ordering::Release);
    }
}

/// Try to acquire the dispatch lock for an SP (by partition ID).
/// Returns a guard on success.
pub fn try_lock_sp(sp_id: u16) -> Result<SpDispatchGuard, SpLockError> {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for read-only slot scan.
    let contexts = unsafe { &*SP_STORE.contexts.get() };
    for (i, slot) in contexts.iter().enumerate() {
        if let Some(ref sp) = slot {
            if sp.sp_id() == sp_id {
                // Attempt atomic lock acquisition
                if SP_DISPATCH_LOCK[i]
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return Ok(SpDispatchGuard { slot: i });
                }
                return Err(SpLockError::Busy); // Found SP but locked by another CPU
            }
        }
    }
    Err(SpLockError::NotFound) // SP not found
}

/// Register a booted SP in the global store.
pub fn register_sp(sp: SpContext) {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK`; this is the only mutable access
    // path for inserting into `SP_STORE`.
    unsafe {
        let contexts = &mut *SP_STORE.contexts.get();
        for slot in contexts.iter_mut() {
            if slot.is_none() {
                *slot = Some(sp);
                return;
            }
        }
        panic!("No free SP slots");
    }
}

/// Run a closure with exclusive mutable access to an SP (if lock can be acquired).
/// Returns None if SP does not exist or is currently locked by another CPU.
pub fn with_sp_locked<R, F: FnOnce(&mut SpContext) -> R>(sp_id: u16, f: F) -> Option<R> {
    let mut guard = try_lock_sp(sp_id).ok()?;
    Some(f(guard.sp_mut()))
}

/// Read a specific SP's state without taking a mutable borrow.
pub fn state_of(sp_id: u16) -> Option<SpState> {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for read-only slot scan.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.sp_id() == sp_id {
                return Some(sp.state());
            }
        }
        None
    }
}

/// Set pending IRQ for one SP by partition ID.
pub fn set_pending_irq_for(sp_id: u16, intid: u32) -> bool {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK`; only atomic IRQ slots are mutated.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.sp_id() == sp_id {
                sp.set_pending_irq(intid);
                return true;
            }
        }
        false
    }
}

/// Consume pending IRQ for one SP by partition ID.
pub fn take_pending_irq_for(sp_id: u16) -> Option<u32> {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for slot traversal.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.sp_id() == sp_id {
                return sp.take_pending_irq();
            }
        }
        None
    }
}

/// Check if a partition ID belongs to a registered SP.
pub fn is_registered_sp(sp_id: u16) -> bool {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for read-only slot scan.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.sp_id() == sp_id {
                return true;
            }
        }
        false
    }
}

/// Iterate over all registered SPs, calling `f` for each one.
///
/// # Safety (internal)
/// The callback `f` must NOT call `register_sp()`, `with_sp_locked()`, or any
/// other function that mutates SP_STORE. Doing so is undefined behavior.
pub fn for_each_sp<F: FnMut(&SpContext)>(mut f: F) {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK`; callback receives shared refs only.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            f(sp);
        }
    }
}

/// Get the first owned INTID for a given SP (read-only lookup).
/// Used by the IRQ handler to inject virtual interrupts without &mut.
pub fn first_owned_intid_for(sp_id: u16) -> Option<u32> {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for read-only slot scan.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.sp_id() == sp_id {
                return sp.first_owned_intid();
            }
        }
        None
    }
}

/// Find which SP owns a given INTID.
/// Returns the SP's partition ID, or None.
pub fn find_sp_for_intid(intid: u32) -> Option<u16> {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for read-only slot scan.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.owns_intid(intid) {
                return Some(sp.sp_id());
            }
        }
        None
    }
}

/// Find any SP that has a pending interrupt. Returns the SP's partition ID, or None.
pub fn find_sp_with_pending_irq() -> Option<u16> {
    let _store_guard = SP_STORE_LOCK.lock();
    // SAFETY: protected by `SP_STORE_LOCK` for read-only slot scan.
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for sp in contexts.iter().flatten() {
            if sp.has_pending_irq() {
                return Some(sp.sp_id());
            }
        }
        None
    }
}
