#![no_std]

pub mod arch;
pub mod devices;
pub mod dtb;
pub mod ffa;
pub mod global;
pub mod guest_loader;
pub mod manifest;
pub mod mm;
pub mod spmc_handler;
pub mod sp_context;
pub mod percpu;
pub mod platform;
pub mod scheduler;
pub mod sync;
pub mod uart;
pub mod vcpu;
pub mod vcpu_interrupt;
pub mod vm;
pub mod vswitch;

// Note: println! macro is exported at the crate root via #[macro_export]
// It can be used as: use hypervisor::println;

/// Simple function to write a byte slice to UART
#[inline]
pub fn uart_puts(s: &[u8]) {
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
