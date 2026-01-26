#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Simple function to write a string to UART using inline assembly
#[inline(never)]
fn uart_puts(s: &[u8]) {
    unsafe {
        let uart_base = 0x09000000usize;
        for &byte in s {
            // Write byte to UART data register
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) byte as u32,
                options(nostack),
            );
        }
    }
}

/// Rust entry point called from boot.S
#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    uart_puts(b"========================================\n");
    uart_puts(b"  ARM64 Hypervisor - Milestone 0\n");
    uart_puts(b"========================================\n");
    uart_puts(b"\n");
    uart_puts(b"Hello from EL2!\n");
    uart_puts(b"\n");
    uart_puts(b"System Information:\n");
    uart_puts(b"  - Exception Level: EL2 (Hypervisor)\n");
    uart_puts(b"  - Architecture: AArch64\n");
    uart_puts(b"  - Target: QEMU virt machine\n");
    uart_puts(b"\n");
    uart_puts(b"Project initialized successfully!\n");
    uart_puts(b"========================================\n");
    
    // Halt - we'll implement proper VM management later
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

/// Panic handler - required for no_std
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    uart_puts(b"\n!!! PANIC !!!\n");
    
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
