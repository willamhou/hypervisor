///! Test MMIO device emulation
///! 
///! This test creates a guest that directly accesses UART via MMIO

use crate::vm::Vm;
use crate::uart_puts;

/// Guest code that accesses UART via MMIO
#[repr(C, align(4096))]
struct GuestCodeMmio {
    code: [u32; 26],
}

static GUEST_CODE_MMIO: GuestCodeMmio = GuestCodeMmio {
    code: [
        // Print "MMIO!" via direct UART access
        // UART base = 0x09000000
        
        // Load UART base address into x19 (callee-saved, safer)
        // We'll build 0x09000000 step by step
        // movz x19, #0x0900
        0xd2812013,  // movz x19, #0x0900 (imm16=0x0900, shift=0)
        // movk x19, #0x0000, lsl #16
        // Actually, let's use a different approach - load via PC-relative
        
        // Alternative: Use an immediate that's easier to encode
        // Let's try: mov x19, #0x09000000 directly
        // But that doesn't work in one instruction for large immediates
        
        // Better approach: use literal load
        // ldr x19, #offset to literal pool
        0x58000133,  // ldr x19, [PC + #0x26*4] = load from literal at offset
        
        // Actually, simplest: just hardcode the address calculation
        // movz x19, #0x0900, lsl #16
        // Encoding: sf=1, opc=10, hw=01, imm16=0x0900
        // 1|10|100101|01|0000100100000000|19 = 0xd2a12013
        0xd2a12013,  // movz x19, #0x0900, lsl #16 = x19 = 0x09000000
        
        // Now x19 = 0x09000000 (UART base)
        
        // Store 'M' (0x4D = 77) to UART
        0xd28009a1,  // mov x1, #77
        0xb9000261,  // str w1, [x19, #0]
        
        // Store 'M' again
        0xd28009a1,  // mov x1, #77
        0xb9000261,  // str w1, [x19]
        
        // Store 'I' (0x49 = 73)
        0xd2800921,  // mov x1, #73
        0xb9000261,  // str w1, [x19]
        
        // Store 'O' (0x4F = 79)
        0xd28009e1,  // mov x1, #79
        0xb9000261,  // str w1, [x19]
        
        // Store '!' (0x21 = 33)
        0xd2800421,  // mov x1, #33
        0xb9000261,  // str w1, [x19]
        
        // Store '\n' (0x0A = 10)
        0xd2800141,  // mov x1, #10
        0xb9000261,  // str w1, [x19]
        
        // Exit via hypercall
        0xd2800020,  // mov x0, #1 (exit hypercall)
        0xd4000002,  // hvc #0
        
        // Padding
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ],
};

/// Stack for the MMIO test guest
#[repr(C, align(4096))]
struct GuestStackMmio {
    stack: [u8; 16384],
}

static mut GUEST_STACK_MMIO: GuestStackMmio = GuestStackMmio {
    stack: [0; 16384],
};

/// Run MMIO test
pub fn run_mmio_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  MMIO Device Emulation Test\n");
    uart_puts(b"========================================\n\n");
    
    uart_puts(b"[MMIO TEST] Creating VM...\n");
    
    // Create VM
    let mut vm = Vm::new(1);
    
    // Get guest code and stack addresses
    let guest_entry = &GUEST_CODE_MMIO.code as *const _ as u64;
    let guest_stack = unsafe {
        (&GUEST_STACK_MMIO.stack as *const [u8; 16384]) as u64 + 16384
    };
    
    uart_puts(b"[MMIO TEST] Guest entry: 0x");
    print_hex(guest_entry);
    uart_puts(b"\n");
    
    // Initialize memory mapping
    let mem_start = guest_entry & !(2 * 1024 * 1024 - 1);
    let mem_end = ((guest_stack + 2 * 1024 * 1024 - 1) / (2 * 1024 * 1024)) * (2 * 1024 * 1024);
    let mem_size = mem_end - mem_start;
    
    vm.init_memory(mem_start, mem_size);
    
    // Add vCPU
    match vm.add_vcpu(guest_entry, guest_stack) {
        Ok(vcpu_id) => {
            uart_puts(b"[MMIO TEST] Created vCPU ");
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
    uart_puts(b"[MMIO TEST] Starting guest...\n");
    uart_puts(b"[GUEST OUTPUT] ");
    
    match vm.run() {
        Ok(()) => {
            uart_puts(b"[MMIO TEST] Guest exited successfully\n");
        }
        Err(e) => {
            uart_puts(b"[ERROR] Guest failed: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
        }
    }
    
    uart_puts(b"\n[MMIO TEST] Test complete!\n");
    uart_puts(b"========================================\n\n");
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
