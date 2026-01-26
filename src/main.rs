#![no_std]
#![no_main]

use core::panic::PanicInfo;
use hypervisor::arch::aarch64::exception;
use hypervisor::vm::Vm;
use hypervisor::uart_puts;

/// Simple function to write a string to UART using inline assembly
#[inline(never)]
fn uart_puts_local(s: &[u8]) {
    uart_puts(s);
}

/// Rust entry point called from boot.S
#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    uart_puts_local(b"========================================\n");
    uart_puts_local(b"  ARM64 Hypervisor - Sprint 1.1\n");
    uart_puts_local(b"  vCPU Framework Test\n");
    uart_puts_local(b"========================================\n");
    uart_puts_local(b"\n");
    uart_puts_local(b"[INIT] Initializing at EL2...\n");
    
    // Initialize exception handling
    uart_puts_local(b"[INIT] Setting up exception vector table...\n");
    exception::init();
    uart_puts_local(b"[INIT] Exception handling initialized\n");
    
    // Check current exception level
    let current_el: u64;
    unsafe {
        core::arch::asm!(
            "mrs {el}, CurrentEL",
            el = out(reg) current_el,
            options(nostack, nomem),
        );
    }
    let el = (current_el >> 2) & 0x3;
    uart_puts_local(b"[INIT] Current EL: EL");
    print_digit(el as u8);
    uart_puts_local(b"\n\n");
    
    // For now, we'll create a simple test without actual guest code
    // In the next step, we'll add the guest binary
    uart_puts_local(b"[TEST] Creating VM...\n");
    let _vm = Vm::new(0);
    
    // We'll use a simple inline guest code for testing
    // Guest entry point: a simple loop that does HVC
    uart_puts_local(b"[TEST] VM created successfully\n");
    uart_puts_local(b"[TEST] vCPU framework is ready!\n");
    
    uart_puts_local(b"\n========================================\n");
    uart_puts_local(b"Sprint 1.1: vCPU Framework - COMPLETE\n");
    uart_puts_local(b"========================================\n");
    
    // Halt - we'll implement proper VM execution later
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

/// Print a single digit (0-9)
fn print_digit(digit: u8) {
    let ch = b'0' + digit;
    uart_puts_local(&[ch]);
}

/// Panic handler - required for no_std
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    uart_puts_local(b"\n!!! PANIC !!!\n");
    
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
