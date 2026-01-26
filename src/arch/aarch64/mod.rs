//! ARM64/AArch64 architecture support
//! 
//! This module contains ARM64-specific virtualization support including:
//! - Register definitions and structures
//! - Exception vector tables
//! - VM entry/exit mechanisms
//! - System register access
//! - Memory management (Stage-2 translation)

pub mod regs;
pub mod exception;
pub mod mmu;
pub mod gic;
pub mod timer;

pub use regs::*;
pub use exception::*;
pub use mmu::*;
pub use gic::*;
pub use timer::*;
