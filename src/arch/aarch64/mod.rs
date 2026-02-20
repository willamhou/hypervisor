//! ARM64/AArch64 architecture support
//!
//! This module contains ARM64-specific virtualization support including:
//! - Register definitions and structures
//! - Exception vector tables and trap handling
//! - VM entry/exit mechanisms
//! - System register access
//! - Memory management (Stage-2 translation)
//! - Peripheral access (GIC, Timer)

pub mod defs;
pub mod hypervisor;
pub mod mm;
pub mod peripherals;
pub mod regs;
pub mod vcpu_arch_state;

// Re-export commonly used types
pub use hypervisor::{enter_guest, exception_vector_table};
pub use mm::{init_stage2, IdentityMapper, MemoryAttributes};
pub use regs::{ExitReason, GeneralPurposeRegs, SystemRegs, VcpuContext};
