//! ARM Generic Interrupt Controller (GIC) Device Driver
//!
//! This module provides emulation for the GIC distributor and CPU interface.

mod distributor;

pub use distributor::VirtualGicd;
