//! Guest Loader Module
//!
//! This module provides configuration and boot logic for loading
//! real ELF binaries as guests.

/// Guest configuration
///
/// Defines memory layout and entry point for a guest VM.
pub struct GuestConfig {
    /// Guest code load address (where QEMU loads the ELF)
    pub load_addr: u64,
    /// Guest memory size in bytes
    pub mem_size: u64,
    /// Entry point address (usually equals load_addr)
    pub entry_point: u64,
}

impl GuestConfig {
    /// Default configuration for Zephyr RTOS on qemu_cortex_a53
    ///
    /// - Load address: 0x4800_0000
    /// - Memory size: 128MB
    /// - Entry point: 0x4800_0000
    pub const fn zephyr_default() -> Self {
        Self {
            load_addr: 0x4800_0000,
            mem_size: 128 * 1024 * 1024, // 128MB
            entry_point: 0x4800_0000,
        }
    }
}
