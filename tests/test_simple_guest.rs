//! Simple guest test using inline assembly
//!
//! Tests the guest boot path with a minimal guest that:
//! 1. Prints a character via UART
//! 2. Exits via HVC

use hypervisor::uart_puts;
use hypervisor::vm::Vm;

/// Simple guest code that writes to UART then exits
#[repr(C, align(4096))]
struct SimpleGuest {
    code: [u32; 8],
}

/// Guest code at 0x48000000 (simulated - we place it in static memory)
static SIMPLE_GUEST: SimpleGuest = SimpleGuest {
    code: [
        // Write 'Z' to UART (0x09000000)
        // MOVZ x0, #0x0900, LSL #16 -> x0 = 0x09000000
        0xd2a12000, // movz x0, #0x900, lsl #16
        0xd2800b41, // movz x1, #0x5A ('Z' = 90)
        0xb9000001, // str w1, [x0]
        // Exit via HVC
        0xd2800020, // mov x0, #1 (exit hypercall)
        0xd4000002, // hvc #0
        // Should not reach (padding to 8 instructions)
        0xd503207f, // wfe
        0x14000000, // b .
        0x00000000, // nop (padding)
    ],
};

/// Run simple guest test
pub fn run_test() {
    uart_puts(b"\n[TEST] Simple Guest Test\n");
    uart_puts(b"[TEST] ========================\n");

    let guest_addr = &SIMPLE_GUEST.code as *const _ as u64;
    uart_puts(b"[TEST] Guest code at: 0x");
    print_hex(guest_addr);
    uart_puts(b"\n");

    // Create VM
    let mut vm = Vm::new(1);

    // Initialize memory - map the region containing our guest code
    let mem_start = guest_addr & !(2 * 1024 * 1024 - 1);
    vm.init_memory(mem_start, 4 * 1024 * 1024);

    // Create vCPU
    match vm.create_vcpu(0) {
        Ok(vcpu) => {
            vcpu.context_mut().pc = guest_addr;
            vcpu.context_mut().sp = guest_addr + 0x10000; // Arbitrary stack
        }
        Err(e) => {
            uart_puts(b"[TEST] Failed to create vCPU: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
            return;
        }
    }

    // Run guest
    uart_puts(b"[TEST] Running guest (expect 'Z'): ");

    match vm.run() {
        Ok(()) => {
            uart_puts(b"\n[TEST] Simple Guest Test PASSED\n\n");
        }
        Err(e) => {
            uart_puts(b"\n[TEST] Guest error: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
        }
    }
}

fn print_hex(value: u64) {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];

    for i in 0..16 {
        let nibble = ((value >> ((15 - i) * 4)) & 0xF) as usize;
        buffer[i] = HEX_CHARS[nibble];
    }

    uart_puts(&buffer);
}
