//! Architecture-specific code
//! 
//! This module contains ARM64/AArch64 specific implementations

#[cfg(target_arch = "aarch64")]
pub mod aarch64;

#[cfg(target_arch = "aarch64")]
pub use aarch64::*;
