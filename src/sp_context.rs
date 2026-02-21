//! Secure Partition context management.
//!
//! Each SP has an `SpContext` that holds its register state (via `VcpuContext`)
//! and a state machine tracking its lifecycle.

use crate::arch::aarch64::defs::SPSR_EL1H_DAIF_MASKED;
use crate::arch::aarch64::regs::VcpuContext;
use core::cell::UnsafeCell;

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
}

/// Per-SP context: register state + metadata.
pub struct SpContext {
    /// Register context passed to enter_guest() for ERET.
    ctx: VcpuContext,
    /// FF-A partition ID (e.g. 0x8001).
    id: u16,
    /// Current lifecycle state.
    state: SpState,
    /// Cold boot entry point.
    entry: u64,
    /// Secure Stage-2 VSTTBR value for this SP (set after page table creation).
    vsttbr: u64,
}

impl SpContext {
    /// Create a new SP context in Reset state.
    pub fn new(sp_id: u16, entry_point: u64, stack_top: u64) -> Self {
        let mut ctx = VcpuContext::default();
        ctx.pc = entry_point;
        ctx.sp = stack_top;
        ctx.sys_regs.sp_el1 = stack_top;
        ctx.spsr_el2 = SPSR_EL1H_DAIF_MASKED;

        Self {
            ctx,
            id: sp_id,
            state: SpState::Reset,
            entry: entry_point,
            vsttbr: 0,
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

    /// Validate and perform a state transition.
    pub fn transition_to(&mut self, new_state: SpState) -> Result<(), &'static str> {
        let valid = match (self.state, new_state) {
            (SpState::Reset, SpState::Idle) => true,
            (SpState::Idle, SpState::Running) => true,
            (SpState::Running, SpState::Idle) => true,
            (SpState::Running, SpState::Blocked) => true,
            (SpState::Blocked, SpState::Running) => true,
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
