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
    /// - Load address: 0x4800_0000 (offset to avoid DTB at 0x40000000)
    /// - Memory size: 128MB
    /// - Entry point: Read from ELF header at runtime
    ///
    /// Note: Zephyr is built with hypervisor_guest.overlay to link at 0x48000000.
    pub fn zephyr_default() -> Self {
        let load_addr: u64 = 0x4800_0000;

        // Read entry point from ELF header
        // ELF64 header: e_entry is at offset 0x18 (24 bytes)
        let entry_point = unsafe {
            let elf_header = load_addr as *const u8;
            // Check ELF magic: 0x7F 'E' 'L' 'F'
            let magic = core::slice::from_raw_parts(elf_header, 4);

            // Debug: print first 8 bytes at load address
            uart_puts(b"[GUEST] First 8 bytes at load addr: ");
            for i in 0..8 {
                let byte = *elf_header.add(i);
                let hex_chars = b"0123456789abcdef";
                uart_puts(&[hex_chars[(byte >> 4) as usize], hex_chars[(byte & 0xf) as usize], b' ']);
            }
            uart_puts(b"\n");

            if magic == [0x7F, b'E', b'L', b'F'] {
                // Valid ELF, read e_entry at offset 0x18
                let e_entry_ptr = (load_addr + 0x18) as *const u64;
                let entry = core::ptr::read_volatile(e_entry_ptr);
                uart_puts(b"[GUEST] ELF detected, e_entry = 0x");
                uart_put_hex(entry);
                uart_puts(b"\n");
                entry
            } else {
                // Not an ELF - QEMU loaded raw segments
                // Check if first instruction is a branch (B imm26)
                // B instruction encoding: 000101 | imm26
                // 0x14xxxxxx = unconditional branch
                uart_puts(b"[GUEST] No ELF magic - checking for branch instruction\n");

                let first_instr = core::ptr::read_volatile(load_addr as *const u32);
                uart_puts(b"[GUEST] First instruction: 0x");
                uart_put_hex(first_instr as u64);
                uart_puts(b"\n");

                if (first_instr >> 26) == 0b000101 {
                    // B imm26 - unconditional branch
                    // imm26 is signed, in units of 4 bytes
                    let imm26 = first_instr & 0x03FF_FFFF;
                    // Sign extend from 26 bits
                    let offset = if imm26 & 0x0200_0000 != 0 {
                        // Negative offset
                        ((imm26 | 0xFC00_0000) as i32) * 4
                    } else {
                        (imm26 as i32) * 4
                    };
                    let target = (load_addr as i64 + offset as i64) as u64;
                    uart_puts(b"[GUEST] Branch to offset ");
                    uart_put_hex(offset as u64);
                    uart_puts(b", target = 0x");
                    uart_put_hex(target);
                    uart_puts(b"\n");
                    target
                } else {
                    // Not a branch, use load address
                    uart_puts(b"[GUEST] Using load address as entry\n");
                    load_addr
                }
            }
        };

        Self {
            load_addr,
            mem_size: 128 * 1024 * 1024, // 128MB
            entry_point,
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

    // Initialize guest timer access (allows guest to use virtual timer)
    uart_puts(b"[GUEST] Configuring virtual timer for guest...\n");
    crate::arch::aarch64::peripherals::timer::init_guest_timer();

    // Enter guest
    uart_puts(b"[GUEST] Entering guest at 0x");
    uart_put_hex(config.entry_point);
    uart_puts(b"...\n");
    uart_puts(b"========================================\n\n");

    // Run VM
    let result = vm.run();

    // Debug: check UART state after guest exits
    uart_puts(b"\n[GUEST] Guest exited, checking UART state...\n");
    unsafe {
        let uart_base = 0x09000000usize;
        // Read UART Flag Register (UARTFR) at offset 0x18
        let uartfr = core::ptr::read_volatile((uart_base + 0x18) as *const u32);
        uart_puts(b"[GUEST] UART FR: 0x");
        let fr_bytes = [
            b"0123456789abcdef"[((uartfr >> 12) & 0xF) as usize],
            b"0123456789abcdef"[((uartfr >> 8) & 0xF) as usize],
            b"0123456789abcdef"[((uartfr >> 4) & 0xF) as usize],
            b"0123456789abcdef"[(uartfr & 0xF) as usize],
        ];
        uart_puts(&fr_bytes);
        uart_puts(b"\n");

        // Write a test character to verify UART still works
        uart_puts(b"[GUEST] Test output after guest: OK\n");
    }

    result
}
