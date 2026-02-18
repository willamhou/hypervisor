//! FF-A Proxy — main dispatch for FF-A SMC calls.
//!
//! Routes FF-A function IDs to local handlers or stub SPMC.
//! Stub implementation: returns NOT_SUPPORTED for all calls.

use crate::arch::aarch64::regs::VcpuContext;

/// Handle an FF-A SMC call from guest.
/// Stub — returns NOT_SUPPORTED for all calls.
pub fn handle_ffa_call(context: &mut VcpuContext) -> bool {
    // FFA_ERROR with NOT_SUPPORTED
    context.gp_regs.x0 = 0x84000060; // FFA_ERROR
    context.gp_regs.x2 = 0xFFFF_FFFF; // NOT_SUPPORTED (-1 as i32, 32-bit, no sign extension)
    true
}
