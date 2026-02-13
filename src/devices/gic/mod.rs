//! ARM Generic Interrupt Controller (GIC) Device Driver
//!
//! This module provides emulation for the GIC distributor, redistributor,
//! and CPU interface.

mod distributor;
mod redistributor;

pub use distributor::VirtualGicd;
pub use redistributor::VirtualGicr;
