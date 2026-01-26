//! ARM64 Register Definitions
//! 
//! This module defines the register context that needs to be saved/restored
//! when entering/exiting a virtual machine.

use core::fmt;

/// General Purpose Registers (x0-x30)
/// 
/// In ARM64, we have 31 general purpose registers:
/// - x0-x30: General purpose registers
/// - x29: Frame Pointer (FP)
/// - x30: Link Register (LR)
/// - SP: Stack Pointer (separate from x31)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GeneralPurposeRegs {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
    pub x8: u64,
    pub x9: u64,
    pub x10: u64,
    pub x11: u64,
    pub x12: u64,
    pub x13: u64,
    pub x14: u64,
    pub x15: u64,
    pub x16: u64,
    pub x17: u64,
    pub x18: u64,
    pub x19: u64,
    pub x20: u64,
    pub x21: u64,
    pub x22: u64,
    pub x23: u64,
    pub x24: u64,
    pub x25: u64,
    pub x26: u64,
    pub x27: u64,
    pub x28: u64,
    pub x29: u64, // FP
    pub x30: u64, // LR
}

impl Default for GeneralPurposeRegs {
    fn default() -> Self {
        Self {
            x0: 0, x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
            x8: 0, x9: 0, x10: 0, x11: 0, x12: 0, x13: 0, x14: 0, x15: 0,
            x16: 0, x17: 0, x18: 0, x19: 0, x20: 0, x21: 0, x22: 0, x23: 0,
            x24: 0, x25: 0, x26: 0, x27: 0, x28: 0, x29: 0, x30: 0,
        }
    }
}

/// System Registers
/// 
/// These are the key system registers that need to be managed when
/// running a guest VM. They control the VM's execution state.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SystemRegs {
    /// Stack Pointer (EL1)
    pub sp_el1: u64,
    
    /// Exception Link Register (EL1) - Return address for exceptions
    pub elr_el1: u64,
    
    /// Saved Program Status Register (EL1) - CPU state (flags, mode, etc.)
    pub spsr_el1: u64,
    
    /// System Control Register (EL1) - Controls MMU, caches, etc.
    pub sctlr_el1: u64,
    
    /// Translation Table Base Register 0 (EL1) - Page table base
    pub ttbr0_el1: u64,
    
    /// Translation Table Base Register 1 (EL1) - Kernel page table base
    pub ttbr1_el1: u64,
    
    /// Translation Control Register (EL1) - Controls translation/MMU
    pub tcr_el1: u64,
    
    /// Memory Attribute Indirection Register (EL1)
    pub mair_el1: u64,
    
    /// Vector Base Address Register (EL1) - Exception vector table base
    pub vbar_el1: u64,
    
    /// Context ID Register (EL1) - Process/ASID identifier
    pub contextidr_el1: u64,
    
    /// Thread ID Registers
    pub tpidr_el1: u64,     // OS thread ID
    pub tpidrro_el0: u64,   // User read-only thread ID
    pub tpidr_el0: u64,     // User read-write thread ID
    
    /// Exception Syndrome Register (EL2) - Why did we exit?
    pub esr_el2: u64,
    
    /// Fault Address Register (EL2) - What address caused the fault?
    pub far_el2: u64,
    
    /// Hypervisor Configuration Register
    pub hcr_el2: u64,
    
    /// Counter-timer Virtual Offset
    pub cntvoff_el2: u64,
}

impl Default for SystemRegs {
    fn default() -> Self {
        Self {
            sp_el1: 0,
            elr_el1: 0,
            spsr_el1: 0,
            sctlr_el1: 0,
            ttbr0_el1: 0,
            ttbr1_el1: 0,
            tcr_el1: 0,
            mair_el1: 0,
            vbar_el1: 0,
            contextidr_el1: 0,
            tpidr_el1: 0,
            tpidrro_el0: 0,
            tpidr_el0: 0,
            esr_el2: 0,
            far_el2: 0,
            hcr_el2: 0,
            cntvoff_el2: 0,
        }
    }
}

/// Complete vCPU Register Context
/// 
/// This structure contains all the registers that need to be saved/restored
/// when switching between host and guest execution.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VcpuContext {
    /// General purpose registers
    pub gp_regs: GeneralPurposeRegs,
    
    /// System registers
    pub sys_regs: SystemRegs,
    
    /// Stack pointer for this vCPU context
    pub sp: u64,
    
    /// Program counter - where to resume execution
    pub pc: u64,
}

impl Default for VcpuContext {
    fn default() -> Self {
        Self {
            gp_regs: GeneralPurposeRegs::default(),
            sys_regs: SystemRegs::default(),
            sp: 0,
            pc: 0,
        }
    }
}

impl VcpuContext {
    /// Create a new vCPU context with specified entry point
    pub fn new(entry_point: u64, stack_pointer: u64) -> Self {
        let mut ctx = Self::default();
        ctx.pc = entry_point;
        ctx.sp = stack_pointer;
        ctx.sys_regs.sp_el1 = stack_pointer;
        
        // Set SPSR to EL1h (EL1 with SP_EL1)
        // Bits [3:0] = 0b0101 (EL1h)
        // Bit [6] = 0 (FIQ not masked)
        // Bit [7] = 0 (IRQ not masked)
        // Bit [8] = 0 (SError not masked)
        // Bit [9] = 0 (Debug exceptions not masked)
        ctx.sys_regs.spsr_el1 = 0b0101;
        
        ctx
    }
    
    /// Get the exit reason from ESR_EL2
    pub fn exit_reason(&self) -> ExitReason {
        let ec = (self.sys_regs.esr_el2 >> 26) & 0x3F;
        
        match ec {
            0x00 => ExitReason::Unknown,
            0x01 => ExitReason::WfiWfe,
            0x16 => ExitReason::HvcCall,
            0x18 => ExitReason::TrapMsrMrs,
            0x20 | 0x24 => ExitReason::InstructionAbort,
            0x21 | 0x25 => ExitReason::DataAbort,
            _ => ExitReason::Other(ec),
        }
    }
}

/// VM Exit Reason
/// 
/// Represents why the VM exited and trapped to the hypervisor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// Unknown/undefined reason
    Unknown,
    
    /// WFI (Wait For Interrupt) or WFE (Wait For Event)
    WfiWfe,
    
    /// HVC (Hypervisor Call) instruction
    HvcCall,
    
    /// Trapped MSR/MRS (system register access)
    TrapMsrMrs,
    
    /// Instruction abort (instruction fetch fault)
    InstructionAbort,
    
    /// Data abort (data access fault)
    DataAbort,
    
    /// Other reason with exception class code
    Other(u64),
}

impl fmt::Display for ExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExitReason::Unknown => write!(f, "Unknown"),
            ExitReason::WfiWfe => write!(f, "WFI/WFE"),
            ExitReason::HvcCall => write!(f, "HVC Call"),
            ExitReason::TrapMsrMrs => write!(f, "MSR/MRS Trap"),
            ExitReason::InstructionAbort => write!(f, "Instruction Abort"),
            ExitReason::DataAbort => write!(f, "Data Abort"),
            ExitReason::Other(ec) => write!(f, "Other (EC=0x{:x})", ec),
        }
    }
}
