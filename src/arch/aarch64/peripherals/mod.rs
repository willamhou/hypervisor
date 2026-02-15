//! ARM64 Peripheral Drivers
//!
//! This module provides low-level access to ARM-specific peripherals:
//! - GIC (Generic Interrupt Controller) - v2/v3/v4
//! - ARM Generic Timer

pub mod gic;
pub mod gicv3;
pub mod timer;

// Re-export GICv3 (primary) — gic module available as peripherals::gic for GICv2 fallback
pub use gicv3::*;
pub use timer::*;
