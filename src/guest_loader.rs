//! Guest Loader Module
//!
//! This module provides configuration and boot logic for loading
//! real ELF binaries as guests.

use crate::vm::Vm;
use crate::uart_puts;
use crate::uart_put_hex;

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

/// Boot a guest VM with the given configuration
///
/// # Arguments
/// * `config` - Guest configuration (memory layout, entry point)
///
/// # Returns
/// * `Ok(())` - Guest exited normally
/// * `Err(msg)` - Error occurred
///
/// # Example
/// ```rust,ignore
/// let config = GuestConfig::zephyr_default();
/// run_guest(&config)?;
/// ```
pub fn run_guest(config: &GuestConfig) -> Result<(), &'static str> {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Guest VM Boot\n");
    uart_puts(b"========================================\n");

    uart_puts(b"[GUEST] Load address: 0x");
    uart_put_hex(config.load_addr);
    uart_puts(b"\n");

    uart_puts(b"[GUEST] Memory size: ");
    uart_put_hex(config.mem_size);
    uart_puts(b" bytes\n");

    uart_puts(b"[GUEST] Entry point: 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"\n\n");

    // Create VM
    uart_puts(b"[GUEST] Creating VM...\n");
    let mut vm = Vm::new(0);

    // Initialize memory mapping for guest
    uart_puts(b"[GUEST] Initializing Stage-2 memory...\n");
    vm.init_memory(config.load_addr, config.mem_size);

    // Create vCPU with guest entry point
    // Stack pointer at end of guest memory region
    let guest_sp = config.load_addr + config.mem_size - 0x1000; // Leave 4KB at top

    uart_puts(b"[GUEST] Creating vCPU...\n");
    uart_puts(b"[GUEST] Stack pointer: 0x");
    uart_put_hex(guest_sp);
    uart_puts(b"\n");

    match vm.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = config.entry_point;
            vcpu.context_mut().sp = guest_sp;
        }
        Err(e) => {
            uart_puts(b"[GUEST] Failed to create vCPU: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
            return Err(e);
        }
    }

    // Enter guest
    uart_puts(b"[GUEST] Entering guest at 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"...\n");
    uart_puts(b"========================================\n\n");

    // Run VM
    vm.run()
}
