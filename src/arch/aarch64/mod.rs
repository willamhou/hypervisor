//! ARM64/AArch64 architecture support
//! 
//! This module contains ARM64-specific virtualization support including:
//! - Register definitions and structures
//! - Exception vector tables
//! - VM entry/exit mechanisms
//! - System register access

pub mod regs;
pub mod exception;

pub use regs::*;
pub use exception::*;
