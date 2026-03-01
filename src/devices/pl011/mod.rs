//! PL011 UART Device Driver
//!
//! This module provides emulation for the ARM PL011 UART device.

mod emulator;

pub use emulator::VirtualUart;

#[cfg(feature = "linux_guest")]
fn apply_stage2_uart_trap(
    _reg: &crate::mm::region_registry::VmStage2RegionRegistration,
    mapper: &mut crate::arch::aarch64::mm::mmu::DynamicIdentityMapper,
) -> Result<(), &'static str> {
    // UART is intentionally trap-and-emulate. If not mapped, keep success.
    match mapper.unmap_4kb_page(crate::platform::UART_BASE as u64) {
        Ok(()) => Ok(()),
        Err("L1 entry not valid") | Err("L2 entry not valid") => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(feature = "linux_guest")]
crate::register_vm_stage2_region!(
    REGISTER_VM_S2_REGION_UART_TRAP,
    "pl011-uart-trap",
    120,
    crate::mm::region_registry::RegionMemType::Device,
    crate::mm::region_registry::RegionPerm::ReadWrite,
    crate::mm::region_registry::RegionExec::ExecuteNever,
    crate::mm::region_registry::RegionAction::Trap,
    apply_stage2_uart_trap
);
