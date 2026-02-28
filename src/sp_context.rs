//! Secure Partition context management.
//!
//! Each SP has an `SpContext` that holds its register state (via `VcpuContext`)
//! and a state machine tracking its lifecycle.

use crate::arch::aarch64::defs::SPSR_EL1H_DAIF_MASKED;
use crate::arch::aarch64::regs::VcpuContext;
use core::arch::asm;
use core::cell::UnsafeCell;

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
}

/// SP lifecycle states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpState {
    /// SP has been loaded but not yet booted.
    Reset,
    /// SP has booted and is waiting for a message (called FFA_MSG_WAIT).
    Idle,
    /// SP is currently executing (SPMC ERETs to it).
    Running,
    /// SP is blocked waiting for an event.
    Blocked,
    /// SP was preempted by NS interrupt, resume via FFA_RUN.
    Preempted,
}

/// Per-SP context: register state + metadata.
pub struct SpContext {
    /// Register context passed to enter_guest() for ERET.
    ctx: VcpuContext,
    /// EL1 system registers saved/restored around enter_guest() calls.
    /// Without this, world switches (e.g. pKVM at NS-EL2) corrupt S-EL1 state.
    el1_state: SpEl1State,
    /// FF-A partition ID (e.g. 0x8001).
    id: u16,
    /// Current lifecycle state.
    state: SpState,
    /// Cold boot entry point.
    entry: u64,
    /// Secure Stage-2 VSTTBR value for this SP (set after page table creation).
    vsttbr: u64,
    /// 128-bit UUID from SP manifest (4 x u32 LE words).
    uuid: [u32; 4],
    /// Pending virtual interrupt to inject via HCR_EL2.VI on next entry.
    pending_irq: Option<u32>,
    /// INTIDs owned by this SP, delivered as virtual IRQ (up to 4, 0 = unused).
    owned_intids: [u32; 4],
}

impl SpContext {
    /// Create a new SP context in Reset state.
    pub fn new(sp_id: u16, entry_point: u64, stack_top: u64, uuid: [u32; 4]) -> Self {
        let mut ctx = VcpuContext::default();
        ctx.pc = entry_point;
        ctx.sp = stack_top;
        ctx.sys_regs.sp_el1 = stack_top;
        ctx.spsr_el2 = SPSR_EL1H_DAIF_MASKED;

        Self {
            ctx,
            el1_state: SpEl1State::new(),
            id: sp_id,
            state: SpState::Reset,
            entry: entry_point,
            vsttbr: 0,
            uuid,
            pending_irq: None,
            owned_intids: [0; 4],
        }
    }

    pub fn sp_id(&self) -> u16 {
        self.id
    }

    pub fn state(&self) -> SpState {
        self.state
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

    /// Validate and perform a state transition.
    pub fn transition_to(&mut self, new_state: SpState) -> Result<(), &'static str> {
        let valid = match (self.state, new_state) {
            (SpState::Reset, SpState::Idle) => true,
            (SpState::Idle, SpState::Running) => true,
            (SpState::Running, SpState::Idle) => true,
            (SpState::Running, SpState::Blocked) => true,
            (SpState::Blocked, SpState::Running) => true,
            (SpState::Running, SpState::Preempted) => true,
            (SpState::Preempted, SpState::Running) => true,
            _ => false,
        };
        if valid {
            self.state = new_state;
            Ok(())
        } else {
            Err("invalid SP state transition")
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
    pub fn set_pending_irq(&mut self, intid: u32) {
        self.pending_irq = Some(intid);
    }

    /// Take the pending virtual interrupt (returns None if none pending).
    pub fn take_pending_irq(&mut self) -> Option<u32> {
        self.pending_irq.take()
    }

    /// Check if this SP has a pending interrupt.
    pub fn has_pending_irq(&self) -> bool {
        self.pending_irq.is_some()
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

unsafe impl Sync for SpStore {}

static SP_STORE: SpStore = SpStore {
    contexts: UnsafeCell::new([None, None, None, None]),
};

/// Register a booted SP in the global store.
pub fn register_sp(sp: SpContext) {
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

/// Look up an SP by partition ID (mutable, for dispatch).
pub fn get_sp_mut(sp_id: u16) -> Option<&'static mut SpContext> {
    unsafe {
        let contexts = &mut *SP_STORE.contexts.get();
        for slot in contexts.iter_mut() {
            if let Some(ref mut sp) = slot {
                if sp.sp_id() == sp_id {
                    return Some(sp);
                }
            }
        }
        None
    }
}

/// Check if a partition ID belongs to a registered SP.
pub fn is_registered_sp(sp_id: u16) -> bool {
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for slot in contexts.iter() {
            if let Some(ref sp) = slot {
                if sp.sp_id() == sp_id {
                    return true;
                }
            }
        }
        false
    }
}

/// Iterate over all registered SPs, calling `f` for each one.
///
/// # Safety (internal)
/// The callback `f` must NOT call `register_sp()`, `get_sp_mut()`, or any
/// other function that mutates SP_STORE. Doing so is undefined behavior.
pub fn for_each_sp<F: FnMut(&SpContext)>(mut f: F) {
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for slot in contexts.iter() {
            if let Some(ref sp) = slot {
                f(sp);
            }
        }
    }
}

/// Get the first owned INTID for a given SP (read-only lookup).
/// Used by the IRQ handler to inject virtual interrupts without &mut.
pub fn first_owned_intid_for(sp_id: u16) -> Option<u32> {
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for slot in contexts.iter() {
            if let Some(ref sp) = slot {
                if sp.sp_id() == sp_id {
                    return sp.first_owned_intid();
                }
            }
        }
        None
    }
}

/// Find which SP owns a given INTID.
/// Returns the SP's partition ID, or None.
pub fn find_sp_for_intid(intid: u32) -> Option<u16> {
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for slot in contexts.iter() {
            if let Some(ref sp) = slot {
                if sp.owns_intid(intid) {
                    return Some(sp.sp_id());
                }
            }
        }
        None
    }
}

/// Find any SP that has a pending interrupt. Returns the SP's partition ID, or None.
pub fn find_sp_with_pending_irq() -> Option<u16> {
    unsafe {
        let contexts = &*SP_STORE.contexts.get();
        for slot in contexts.iter() {
            if let Some(ref sp) = slot {
                if sp.has_pending_irq() {
                    return Some(sp.sp_id());
                }
            }
        }
        None
    }
}
