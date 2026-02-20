use hypervisor::uart_puts;
///! Test MMIO device emulation
///!
///! This test creates a guest that directly accesses UART via MMIO
use hypervisor::vm::Vm;

/// Guest code that accesses UART via MMIO
///
/// Generated from guest_mmio.S using GNU assembler
#[repr(C, align(4096))]
struct GuestCodeMmio {
    code: [u32; 13],
}

static GUEST_CODE_MMIO: GuestCodeMmio = GuestCodeMmio {
    code: [
        // mov x19, #0x9000000 (UART base)
        0xd2a12013, // Write 'M' (0x4D)
        0x528009a1, // mov w1, #0x4d
        0xb9000261, // str w1, [x19]
        // Write 'M' (0x4D)
        0x528009a1, // mov w1, #0x4d
        0xb9000261, // str w1, [x19]
        // Write 'I' (0x49)
        0x52800921, // mov w1, #0x49
        0xb9000261, // str w1, [x19]
        // Write 'O' (0x4F)
        0x528009e1, // mov w1, #0x4f
        0xb9000261, // str w1, [x19]
        // Write '\n' (0x0A)
        0x52800141, // mov w1, #0xa
        0xb9000261, // str w1, [x19]
        // Exit via hypercall
        0xd2800020, // mov x0, #1
        0xd4000002, // hvc #0
    ],
};

/// Stack for the MMIO test guest
#[repr(C, align(4096))]
struct GuestStackMmio {
    stack: [u8; 16384],
}

static mut GUEST_STACK_MMIO: GuestStackMmio = GuestStackMmio { stack: [0; 16384] };

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
    let guest_stack =
        unsafe { (&raw const GUEST_STACK_MMIO.stack as *const [u8; 16384]) as u64 + 16384 };

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
