#![no_std]
#![no_main]

use hypervisor::{uart, println};
use core::panic::PanicInfo;

/// Rust entry point called from boot.S
#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    // Initialize UART for console output
    uart::init();
    
    // Print welcome message
    println!("========================================");
    println!("  ARM64 Hypervisor - Milestone 0");
    println!("========================================");
    println!();
    println!("Hello from EL2!");
    println!();
    println!("System Information:");
    println!("  - Exception Level: EL2 (Hypervisor)");
    println!("  - Architecture: AArch64");
    println!("  - Target: QEMU virt machine");
    println!();
    println!("Project initialized successfully!");
    println!("========================================");
    
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
    println!("\n!!! PANIC !!!");
    println!("{}", info);
    
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
