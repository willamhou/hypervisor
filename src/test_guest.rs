//! Simple inline guest code for testing
//! 
//! This module provides a simple guest program that can be used
//! to test the vCPU framework.

use crate::vm::Vm;
use crate::uart_puts;

/// Guest code area - simple assembly instructions
/// 
/// This is a minimal guest that will:
/// 1. Print 'G' via hypercall
/// 2. Execute WFI (will trap)
/// 3. Exit via hypercall
#[repr(C, align(4096))]
struct GuestCode {
    code: [u32; 16],
}

static GUEST_CODE: GuestCode = GuestCode {
    code: [
        // Print 'G' - Hypercall 0
        0xd2800000,  // mov x0, #0          (hypercall 0: print char)
        0xd28008e1,  // mov x1, #'G' (0x47 = 71 decimal, shifted: 71 << 5 = 0x8E0)
        0xd4000002,  // hvc #0
        
        // Print '!'
        0xd2800000,  // mov x0, #0
        0xd2800421,  // mov x1, #'!' (0x21 = 33, shifted: 33 << 5 = 0x420)
        0xd4000002,  // hvc #0
        
        // Print newline
        0xd2800000,  // mov x0, #0
        0xd2800141,  // mov x1, #'\n' (0x0a = 10, shifted: 10 << 5 = 0x140)
        0xd4000002,  // hvc #0
        
        // Exit - Hypercall 1
        0xd2800020,  // mov x0, #1          (hypercall 1: exit)
        0xd4000002,  // hvc #0
        
        // Padding / should not reach
        0xd503207f,  // wfe
        0x17ffffff,  // b . (branch to self)
        0x00000000,  // padding
        0x00000000,  // padding
        0x00000000,  // padding
    ],
};

/// Stack for the guest (16KB)
#[repr(C, align(4096))]
struct GuestStack {
    stack: [u8; 16384],
}

static mut GUEST_STACK: GuestStack = GuestStack {
    stack: [0; 16384],
};

/// Run a simple guest test
pub fn run_test() {
    uart_puts(b"\n[TEST] Starting guest execution test...\n");
    
    // Create VM
    let mut vm = Vm::new(0);
    
    // Get guest code and stack addresses
    let guest_entry = &GUEST_CODE.code as *const _ as u64;
    let guest_stack = unsafe {
        (&GUEST_STACK.stack as *const [u8; 16384]) as u64 + 16384
    };
    
    uart_puts(b"[TEST] Guest entry point: 0x");
    print_hex(guest_entry);
    uart_puts(b"\n");
    
    uart_puts(b"[TEST] Guest stack: 0x");
    print_hex(guest_stack);
    uart_puts(b"\n");
    
    // Initialize memory mapping
    // Map the region containing guest code and stack
    let mem_start = guest_entry & !(2 * 1024 * 1024 - 1); // Align to 2MB
    let mem_end = ((guest_stack + 2 * 1024 * 1024 - 1) / (2 * 1024 * 1024)) * (2 * 1024 * 1024);
    let mem_size = mem_end - mem_start;
    
    vm.init_memory(mem_start, mem_size);
    
    // Add vCPU with guest entry point
    match vm.add_vcpu(guest_entry, guest_stack) {
        Ok(vcpu_id) => {
            uart_puts(b"[TEST] Created vCPU ");
            print_digit(vcpu_id as u8);
            uart_puts(b"\n");
        }
        Err(e) => {
            uart_puts(b"[ERROR] Failed to create vCPU: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
            return;
        }
    }
    
    // Run the VM
    uart_puts(b"[TEST] Entering guest...\n");
    uart_puts(b"[GUEST] ");  // Guest will print 'G!\n' after this
    
    match vm.run() {
        Ok(()) => {
            uart_puts(b"[TEST] Guest exited successfully\n");
        }
        Err(e) => {
            uart_puts(b"[ERROR] Guest failed: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
        }
    }
    
    uart_puts(b"[TEST] Guest test complete!\n\n");
}

/// Print a 64-bit hex value
fn print_hex(value: u64) {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];
    
    for i in 0..16 {
        let nibble = ((value >> ((15 - i) * 4)) & 0xF) as usize;
        buffer[i] = HEX_CHARS[nibble];
    }
    
    uart_puts(&buffer);
}

/// Print a single digit
fn print_digit(digit: u8) {
    let ch = b'0' + digit;
    uart_puts(&[ch]);
}
