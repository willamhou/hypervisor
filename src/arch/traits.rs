//! Architecture-Portable Trait Definitions
//!
//! These traits abstract the hardware-specific operations needed by the
//! hypervisor core, enabling future support for additional architectures
//! (e.g., RISC-V) without changing the core VM/vCPU logic.

/// Interrupt controller operations (GICv3 on ARM, PLIC on RISC-V)
pub trait InterruptController {
    fn init(&mut self);
    fn enable(&mut self);
    fn disable(&mut self);
    fn acknowledge(&mut self) -> u32;
    fn eoi(&mut self, intid: u32);
    fn deactivate(&mut self, intid: u32);
    fn set_priority_mask(&mut self, mask: u8);
}

/// Virtual interrupt injection (ICH_LR on ARM, vstip on RISC-V)
pub trait VirtualInterruptController {
    fn inject_interrupt(&mut self, intid: u32, priority: u8) -> Result<(), &'static str>;
    fn inject_hw_interrupt(
        &mut self,
        vintid: u32,
        pintid: u32,
        priority: u8,
    ) -> Result<(), &'static str>;
    fn clear_interrupt(&mut self, intid: u32);
    fn pending_count(&self) -> usize;
}

/// Guest timer operations
pub trait GuestTimer {
    fn init_hypervisor(&mut self);
    fn init_guest(&mut self);
    fn is_pending(&self) -> bool;
    fn mask(&mut self);
    fn get_frequency(&self) -> u64;
    fn get_counter(&self) -> u64;
}

/// Memory type for Stage-2 / G-stage mapping
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryType {
    Normal,
    Device,
    ReadOnly,
}

/// Stage-2 / G-stage page table mapper
pub trait Stage2Mapper {
    fn map_region(&mut self, ipa: u64, size: u64, mem_type: MemoryType) -> Result<(), &'static str>;
    fn reset(&mut self);
    fn install(&self);
    fn root_table_addr(&self) -> u64;
}

/// Architecture-specific vCPU context operations
pub trait VcpuContextOps {
    fn new(entry: u64, sp: u64) -> Self;
    fn pc(&self) -> u64;
    fn set_pc(&mut self, val: u64);
    fn sp(&self) -> u64;
    fn set_sp(&mut self, val: u64);
    fn get_reg(&self, n: u8) -> u64;
    fn set_reg(&mut self, n: u8, val: u64);
    fn advance_pc(&mut self);
}

/// Exception cause (decoded from arch-specific registers)
pub trait ExceptionInfo {
    fn is_wfi(&self) -> bool;
    fn is_hypercall(&self) -> bool;
    fn is_data_abort(&self) -> bool;
    fn is_instruction_abort(&self) -> bool;
    fn fault_address(&self) -> Option<u64>;
}
