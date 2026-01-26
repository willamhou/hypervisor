#![no_std]

pub mod uart;
pub mod arch;
pub mod vcpu;
pub mod vm;
pub mod test_guest;

// Note: println! macro is exported at the crate root via #[macro_export]
// It can be used as: use hypervisor::println;

/// Simple function to write a byte slice to UART
#[inline]
pub fn uart_puts(s: &[u8]) {
    unsafe {
        let uart_base = 0x09000000usize;
        for &byte in s {
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) byte as u32,
                options(nostack),
            );
        }
    }
}
