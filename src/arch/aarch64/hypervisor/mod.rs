//! EL2 Hypervisor-specific code
//!
//! This module contains code that runs at EL2 (Hypervisor mode):
//! - Exception handling and trap processing
//! - Instruction decoding for MMIO emulation

pub mod exception;
pub mod decode;

pub use exception::*;
pub use decode::*;
