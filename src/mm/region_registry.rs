//! VM Stage-2 region registration (macro + linker section).

use crate::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute};
use core::mem::size_of;
use core::ptr::addr_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionMemType {
    Normal,
    Device,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionPerm {
    ReadWrite,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionExec {
    Executable,
    ExecuteNever,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionAction {
    Map,
    Trap,
}

#[repr(C)]
pub struct VmStage2RegionRegistration {
    pub name: &'static str,
    pub priority: u32,
    pub mem_type: RegionMemType,
    pub perm: RegionPerm,
    pub exec: RegionExec,
    pub action: RegionAction,
    pub apply:
        fn(&VmStage2RegionRegistration, &mut DynamicIdentityMapper) -> Result<(), &'static str>,
}

#[macro_export]
macro_rules! register_vm_stage2_region {
    (
        $symbol:ident,
        $name:expr,
        $priority:expr,
        $mem_type:expr,
        $perm:expr,
        $exec:expr,
        $action:expr,
        $apply:path
    ) => {
        #[used]
        #[link_section = ".hv_vm_stage2_regions"]
        static $symbol: $crate::mm::region_registry::VmStage2RegionRegistration =
            $crate::mm::region_registry::VmStage2RegionRegistration {
                name: $name,
                priority: $priority,
                mem_type: $mem_type,
                perm: $perm,
                exec: $exec,
                action: $action,
                apply: $apply,
            };
    };
}

unsafe extern "C" {
    static __hv_vm_stage2_regions_start: u8;
    static __hv_vm_stage2_regions_end: u8;
}

pub fn vm_stage2_region_registrations() -> &'static [VmStage2RegionRegistration] {
    let start = addr_of!(__hv_vm_stage2_regions_start) as usize;
    let end = addr_of!(__hv_vm_stage2_regions_end) as usize;
    let bytes = end.saturating_sub(start);
    let len = bytes / size_of::<VmStage2RegionRegistration>();
    // SAFETY: Linker section contains only `VmStage2RegionRegistration` entries.
    unsafe { core::slice::from_raw_parts(start as *const VmStage2RegionRegistration, len) }
}

fn to_memory_attr(
    mem_type: RegionMemType,
    perm: RegionPerm,
    exec: RegionExec,
) -> Result<MemoryAttribute, &'static str> {
    if exec == RegionExec::Executable {
        return Err("Executable Stage-2 mapping is not supported");
    }
    match (mem_type, perm) {
        (RegionMemType::Normal, RegionPerm::ReadWrite) => Ok(MemoryAttribute::Normal),
        (RegionMemType::Normal, RegionPerm::ReadOnly) => Ok(MemoryAttribute::ReadOnly),
        (RegionMemType::Device, RegionPerm::ReadWrite) => Ok(MemoryAttribute::Device),
        (RegionMemType::Device, RegionPerm::ReadOnly) => Ok(MemoryAttribute::DeviceReadOnly),
    }
}

pub fn map_region_with_registered_attrs(
    reg: &VmStage2RegionRegistration,
    mapper: &mut DynamicIdentityMapper,
    ipa: u64,
    size: u64,
) -> Result<(), &'static str> {
    let attr = to_memory_attr(reg.mem_type, reg.perm, reg.exec)?;
    mapper.map_region(ipa, size, attr)
}
