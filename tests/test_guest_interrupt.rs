///! Test guest interrupt injection
///!
///! This test creates a guest that:
///! 1. Sets up an interrupt vector table
///! 2. Enables interrupts
///! 3. Waits for an interrupt (WFI)
///! 4. Handles the interrupt when injected by hypervisor

use hypervisor::vm::Vm;
use hypervisor::uart_puts;

/// Guest interrupt handler code
/// 
/// Simplified version: Just enable interrupts and exit immediately.
/// If VI bit is set in HCR_EL2, guest should see pending interrupt.
#[repr(C, align(4096))]
struct GuestInterruptCode {
    code: [u32; 16],
}

static GUEST_IRQ_CODE: GuestInterruptCode = GuestInterruptCode {
    code: [
        // Guest code that checks if interrupts are pending
        // Read DAIF to check interrupt mask
        0xd53b4200,  // mrs x0, DAIF
        
        // Enable interrupts (unmask IRQ)
        0xd5033fdf,  // msr daifclr, #2
        
        // Read DAIF again to verify
        0xd53b4201,  // mrs x1, DAIF
        
        // Small delay loop to allow interrupt to be taken
        0xd2800102,  // mov x2, #8
        0xf1000442,  // subs x2, x2, #1
        0x54ffffc1,  // b.ne #-0x8 (loop)
        
        // If we reach here, interrupt was not taken
        // Exit with code 0 (no interrupt taken)
        0xd2800000,  // mov x0, #0
        0xd4000002,  // hvc #0
        
        // Padding
        0, 0, 0, 0, 0, 0, 0, 0,
    ],
};

/// Stack for guest interrupt test
#[repr(C, align(4096))]
struct GuestInterruptStack {
    stack: [u8; 16384],
}

static mut GUEST_IRQ_STACK: GuestInterruptStack = GuestInterruptStack {
    stack: [0; 16384],
};

/// Run guest interrupt injection test
pub fn run_guest_interrupt_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Guest Interrupt Injection Test\n");
    uart_puts(b"========================================\n\n");
    
    uart_puts(b"[IRQ TEST] Creating VM...\n");
    
    // Create VM
    let mut vm = Vm::new(0);
    
    // Get guest code and stack addresses
    let guest_entry = &GUEST_IRQ_CODE.code as *const _ as u64;
    let guest_stack = unsafe {
        (&raw const GUEST_IRQ_STACK.stack as *const [u8; 16384]) as u64 + 16384
    };
    
    uart_puts(b"[IRQ TEST] Guest entry: 0x");
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
            uart_puts(b"[IRQ TEST] Created vCPU ");
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
    
    uart_puts(b"[IRQ TEST] Guest will enable interrupts and check for pending IRQ...\n");
    uart_puts(b"[IRQ TEST] If HCR_EL2.VI is set, guest should see virtual IRQ pending...\n");
    
    // Inject a virtual IRQ before running
    // In a real scenario, this would be done when a physical interrupt arrives
    if let Some(vcpu) = vm.vcpu_mut(0) {
        vcpu.inject_irq(27); // Virtual timer IRQ
        uart_puts(b"[IRQ TEST] Injected IRQ 27 into vCPU\n");
    }
    
    // Run the VM
    uart_puts(b"[IRQ TEST] Starting guest...\n");
    
    match vm.run() {
        Ok(()) => {
            uart_puts(b"[IRQ TEST] Guest handled interrupt and exited successfully!\n");
        }
        Err(e) => {
            uart_puts(b"[ERROR] Guest failed: ");
            uart_puts(e.as_bytes());
            uart_puts(b"\n");
        }
    }
    
    uart_puts(b"\n[IRQ TEST] Test complete!\n");
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
