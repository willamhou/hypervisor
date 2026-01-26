//! ARM64 Peripheral Drivers
//!
//! This module provides low-level access to ARM-specific peripherals:
//! - GIC (Generic Interrupt Controller)
//! - ARM Generic Timer

pub mod gic;
pub mod timer;

pub use gic::*;
pub use timer::*;
