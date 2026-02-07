//! Architecture-specific code
//!
//! This module contains architecture-specific implementations and
//! portable trait definitions for hypervisor hardware abstraction.

pub mod traits;

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;
