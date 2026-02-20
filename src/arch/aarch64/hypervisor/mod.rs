//! EL2 Hypervisor-specific code
//!
//! This module contains code that runs at EL2 (Hypervisor mode):
//! - Exception handling and trap processing
//! - Instruction decoding for MMIO emulation

pub mod decode;
pub mod exception;

pub use decode::*;
pub use exception::*;
