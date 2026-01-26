//! ARM64/AArch64 architecture support
//! 
//! This module contains ARM64-specific virtualization support including:
//! - Register definitions and structures
//! - Exception vector tables and trap handling
//! - VM entry/exit mechanisms
//! - System register access
//! - Memory management (Stage-2 translation)
//! - Peripheral access (GIC, Timer)

pub mod regs;
pub mod hypervisor;
pub mod mm;
pub mod peripherals;

pub use regs::*;
pub use hypervisor::*;
pub use mm::*;
pub use peripherals::*;
