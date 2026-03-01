//! ARM Generic Interrupt Controller (GIC) Device Driver
//!
//! This module provides emulation for the GIC distributor, redistributor,
//! and CPU interface.

mod distributor;
mod redistributor;

pub use distributor::VirtualGicd;
pub use redistributor::VirtualGicr;

#[cfg(feature = "linux_guest")]
fn apply_stage2_gic_map(
    reg: &crate::mm::region_registry::VmStage2RegionRegistration,
    mapper: &mut crate::arch::aarch64::mm::mmu::DynamicIdentityMapper,
) -> Result<(), &'static str> {
    crate::mm::region_registry::map_region_with_registered_attrs(
        reg,
        mapper,
        crate::platform::GIC_REGION_BASE,
        crate::platform::GIC_REGION_SIZE,
    )
}

#[cfg(feature = "linux_guest")]
fn apply_stage2_gicd_trap(
    _reg: &crate::mm::region_registry::VmStage2RegionRegistration,
    mapper: &mut crate::arch::aarch64::mm::mmu::DynamicIdentityMapper,
) -> Result<(), &'static str> {
    for page in 0..16u64 {
        let addr = crate::dtb::platform_info().gicd_base
            + page * crate::arch::aarch64::defs::PAGE_SIZE_4KB;
        mapper.unmap_4kb_page(addr)?;
    }
    Ok(())
}

#[cfg(feature = "linux_guest")]
fn apply_stage2_gicr_trap(
    _reg: &crate::mm::region_registry::VmStage2RegionRegistration,
    mapper: &mut crate::arch::aarch64::mm::mmu::DynamicIdentityMapper,
) -> Result<(), &'static str> {
    for cpu in 0..crate::platform::num_cpus() {
        let base = crate::dtb::gicr_rd_base(cpu);
        for page in 0..32u64 {
            let addr = base + page * crate::arch::aarch64::defs::PAGE_SIZE_4KB;
            mapper.unmap_4kb_page(addr)?;
        }
    }
    Ok(())
}

#[cfg(feature = "linux_guest")]
crate::register_vm_stage2_region!(
    REGISTER_VM_S2_REGION_GIC_MAP,
    "gic-map",
    10,
    crate::mm::region_registry::RegionMemType::Device,
    crate::mm::region_registry::RegionPerm::ReadWrite,
    crate::mm::region_registry::RegionExec::ExecuteNever,
    crate::mm::region_registry::RegionAction::Map,
    apply_stage2_gic_map
);

#[cfg(feature = "linux_guest")]
crate::register_vm_stage2_region!(
    REGISTER_VM_S2_REGION_GICD_TRAP,
    "gicd-trap",
    100,
    crate::mm::region_registry::RegionMemType::Device,
    crate::mm::region_registry::RegionPerm::ReadWrite,
    crate::mm::region_registry::RegionExec::ExecuteNever,
    crate::mm::region_registry::RegionAction::Trap,
    apply_stage2_gicd_trap
);

#[cfg(feature = "linux_guest")]
crate::register_vm_stage2_region!(
    REGISTER_VM_S2_REGION_GICR_TRAP,
    "gicr-trap",
    110,
    crate::mm::region_registry::RegionMemType::Device,
    crate::mm::region_registry::RegionPerm::ReadWrite,
    crate::mm::region_registry::RegionExec::ExecuteNever,
    crate::mm::region_registry::RegionAction::Trap,
    apply_stage2_gicr_trap
);
