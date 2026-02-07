#![no_std]
#![no_main]

use core::panic::PanicInfo;
use hypervisor::arch::aarch64::hypervisor::exception;
use hypervisor::uart_puts;

// Include test module
mod tests {
    include!("../tests/mod.rs");
}

/// Simple function to write a string to UART using inline assembly
#[inline(never)]
fn uart_puts_local(s: &[u8]) {
    uart_puts(s);
}

/// Rust entry point called from boot.S
#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    uart_puts_local(b"========================================\n");
    uart_puts_local(b"  ARM64 Hypervisor - Sprint 2.4\n");
    uart_puts_local(b"  API Documentation\n");
    uart_puts_local(b"========================================\n");
    uart_puts_local(b"\n");
    uart_puts_local(b"[INIT] Initializing at EL2...\n");
    
    // Initialize exception handling
    uart_puts_local(b"[INIT] Setting up exception vector table...\n");
    exception::init();
    uart_puts_local(b"[INIT] Exception handling initialized\n");
    
    // Initialize GIC - try GICv3 first, fall back to GICv2
    hypervisor::arch::aarch64::peripherals::gicv3::init();
    
    // Initialize timer
    uart_puts_local(b"[INIT] Configuring timer...\n");
    hypervisor::arch::aarch64::peripherals::timer::init_hypervisor_timer();
    hypervisor::arch::aarch64::peripherals::timer::print_timer_info();
    
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
    uart_puts_local(b"\n");

    // Initialize heap
    uart_puts_local(b"[INIT] Initializing heap...\n");
    unsafe { hypervisor::mm::heap::init(); }
    uart_puts_local(b"[INIT] Heap initialized (16MB at 0x41000000)\n\n");

    // Run the allocator test
    tests::run_allocator_test();

    // Run the heap test
    tests::run_heap_test();

    // Run the dynamic page table test
    tests::run_dynamic_pt_test();

    // Run the multi-vCPU test
    tests::run_multi_vcpu_test();

    // Run the scheduler test
    tests::run_scheduler_test();

    // Run the VM scheduler integration test
    tests::run_vm_scheduler_test();

    // Run the MMIO device emulation test
    tests::run_mmio_test();

    // Run the GICv3 virtual interface test
    tests::run_gicv3_virt_test();

    // Run the complete interrupt injection test (with guest exception vector)
    tests::run_complete_interrupt_test();
    
    // Run the original guest test (hypercall)
    tests::run_guest_test();

    // Run the guest loader test
    tests::run_guest_loader_test();

    // Run the simple guest test
    tests::run_simple_guest_test();

    // Check if we should boot a Zephyr guest
    #[cfg(feature = "guest")]
    {
        use hypervisor::guest_loader::{GuestConfig, run_guest};

        uart_puts_local(b"\n[INIT] Booting Zephyr guest VM...\n");

        let config = GuestConfig::zephyr_default();
        match run_guest(&config) {
            Ok(()) => {
                uart_puts_local(b"[INIT] Guest exited normally\n");
            }
            Err(e) => {
                if e == "WFI" {
                    // WFI exit is normal for simple apps that just print and idle
                    uart_puts_local(b"[INIT] Guest completed and is idle\n");
                } else {
                    uart_puts_local(b"[INIT] Guest error: ");
                    uart_puts_local(e.as_bytes());
                    uart_puts_local(b"\n");
                }
            }
        }
    }

    // Check if we should boot a Linux guest
    #[cfg(feature = "linux_guest")]
    {
        use hypervisor::guest_loader::{GuestConfig, run_guest};

        uart_puts_local(b"\n[INIT] Booting Linux guest VM...\n");

        let config = GuestConfig::linux_default();
        match run_guest(&config) {
            Ok(()) => {
                uart_puts_local(b"[INIT] Linux guest exited normally\n");
            }
            Err(e) => {
                uart_puts_local(b"[INIT] Linux guest error: ");
                uart_puts_local(e.as_bytes());
                uart_puts_local(b"\n");
            }
        }
    }

    uart_puts_local(b"\n========================================\n");
    uart_puts_local(b"All Sprints Complete (2.1-2.4)\n");
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
