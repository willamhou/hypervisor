#![no_std]

#[cfg(all(feature = "multi_pcpu", feature = "multi_vm"))]
compile_error!("features `multi_pcpu` and `multi_vm` are mutually exclusive");
#[cfg(all(feature = "sel2", feature = "linux_guest"))]
compile_error!("features `sel2` and `linux_guest` are mutually exclusive");
#[cfg(all(feature = "sel2", feature = "guest"))]
compile_error!("features `sel2` and `guest` are mutually exclusive");

pub mod arch;
pub mod devices;
pub mod dtb;
pub mod ffa;
pub mod global;
pub mod guest_loader;
pub mod log;
pub mod manifest;
pub mod mm;
pub mod percpu;
pub mod platform;
pub mod scheduler;
pub mod secure_stage2;
#[cfg(feature = "sel2")]
pub mod sel2_mmu;
pub mod sp_context;
pub mod spmc_handler;
pub mod sync;
pub mod uart;
pub mod vcpu;
pub mod vcpu_interrupt;
pub mod vm;
pub mod vswitch;

// Note: print!/println!/log_*! macros are exported at the crate root via #[macro_export] in log.rs

/// Simple function to write a byte slice to UART
#[inline]
pub fn uart_puts(s: &[u8]) {
    // SAFETY: Writes bytes to UART MMIO TX register at fixed platform base.
    unsafe {
        let uart = platform::UART_BASE;
        for &byte in s {
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart,
                val = in(reg) byte as u32,
                options(nostack),
            );
        }
    }
}

/// Helper function to print a 64-bit value in hex
#[inline]
pub fn uart_put_hex(value: u64) {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];

    for i in 0..16 {
        let nibble = ((value >> ((15 - i) * 4)) & 0xF) as usize;
        buffer[i] = HEX_CHARS[nibble];
    }

    uart_puts(&buffer);
}

/// Helper function to print a 64-bit value in decimal
#[inline]
pub fn uart_put_u64(value: u64) {
    if value == 0 {
        uart_puts(b"0");
        return;
    }

    let mut buffer = [0u8; 20]; // u64 max is 20 digits
    let mut num = value;
    let mut i = 0;

    while num > 0 {
        buffer[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }

    // Print in reverse order
    for j in (0..i).rev() {
        // SAFETY: Writes one byte at a time to UART MMIO TX register.
        unsafe {
            let uart = platform::UART_BASE;
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart,
                val = in(reg) buffer[j] as u32,
                options(nostack),
            );
        }
    }
}
