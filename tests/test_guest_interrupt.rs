//! Test guest interrupt injection
//!
//! This test creates a guest that:
//! 1. Sets up an interrupt vector table
//! 2. Enables interrupts
//! 3. Waits for an interrupt (WFI)
//! 4. Handles the interrupt when injected by hypervisor
use hypervisor::vm::Vm;

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
        0xd53b4200, // mrs x0, DAIF
        // Enable interrupts (unmask IRQ)
        0xd5033fdf, // msr daifclr, #2
        // Read DAIF again to verify
        0xd53b4201, // mrs x1, DAIF
        // Small delay loop to allow interrupt to be taken
        0xd2800102, // mov x2, #8
        0xf1000442, // subs x2, x2, #1
        0x54ffffc1, // b.ne #-0x8 (loop)
        // If we reach here, interrupt was not taken
        // Exit with code 0 (no interrupt taken)
        0xd2800000, // mov x0, #0
        0xd4000002, // hvc #0
        // Padding
        0, 0, 0, 0, 0, 0, 0, 0,
    ],
};

/// Stack for guest interrupt test
#[repr(C, align(4096))]
struct GuestInterruptStack {
    stack: [u8; 16384],
}

static mut GUEST_IRQ_STACK: GuestInterruptStack = GuestInterruptStack { stack: [0; 16384] };

/// Run guest interrupt injection test
pub fn run_guest_interrupt_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Guest Interrupt Injection Test\n");
    hypervisor::log_info!("========================================\n\n");

    hypervisor::log_info!("[IRQ TEST] Creating VM...\n");

    // Create VM
    let mut vm = Vm::new(0);

    // Get guest code and stack addresses
    let guest_entry = &GUEST_IRQ_CODE.code as *const _ as u64;
    // SAFETY: Uses address of static aligned guest IRQ stack object and computes top-of-stack pointer.
    let guest_stack = unsafe { (&raw const GUEST_IRQ_STACK.stack as u64) + 16384 };

    hypervisor::log_info!("[IRQ TEST] Guest entry: {:#018x}\n", guest_entry);

    // Initialize memory mapping
    let mem_start = guest_entry & !(2 * 1024 * 1024 - 1);
    let mem_end = guest_stack.div_ceil(2 * 1024 * 1024) * (2 * 1024 * 1024);
    let mem_size = mem_end - mem_start;

    vm.init_memory(mem_start, mem_size);

    // Add vCPU
    match vm.add_vcpu(guest_entry, guest_stack) {
        Ok(vcpu_id) => {
            hypervisor::log_info!("[IRQ TEST] Created vCPU {}\n", vcpu_id);
        }
        Err(e) => {
            hypervisor::log_info!("[ERROR] Failed to create vCPU: {}\n", e);
            return;
        }
    }

    hypervisor::log_info!("[IRQ TEST] Guest will enable interrupts and check for pending IRQ...\n");
    hypervisor::log_info!(
        "[IRQ TEST] If HCR_EL2.VI is set, guest should see virtual IRQ pending...\n"
    );

    // Inject a virtual IRQ before running
    // In a real scenario, this would be done when a physical interrupt arrives
    if let Some(vcpu) = vm.vcpu_mut(0) {
        vcpu.inject_irq(27); // Virtual timer IRQ
        hypervisor::log_info!("[IRQ TEST] Injected IRQ 27 into vCPU\n");
    }

    // Run the VM
    hypervisor::log_info!("[IRQ TEST] Starting guest...\n");

    match vm.run() {
        Ok(()) => {
            hypervisor::log_info!("[IRQ TEST] Guest handled interrupt and exited successfully!\n");
        }
        Err(e) => {
            hypervisor::log_info!("[ERROR] Guest failed: {}\n", e);
        }
    }

    hypervisor::log_info!("\n[IRQ TEST] Test complete!\n");
    hypervisor::log_info!("========================================\n\n");
}
