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
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  MMIO Device Emulation Test\n");
    hypervisor::log_info!("========================================\n\n");

    hypervisor::log_info!("[MMIO TEST] Creating VM...\n");

    // Create VM
    let mut vm = Vm::new(1);

    // Get guest code and stack addresses
    let guest_entry = &GUEST_CODE_MMIO.code as *const _ as u64;
    let guest_stack =
        unsafe { (&raw const GUEST_STACK_MMIO.stack as *const [u8; 16384]) as u64 + 16384 };

    hypervisor::log_info!("[MMIO TEST] Guest entry: {:#018x}\n", guest_entry);

    // Initialize memory mapping
    let mem_start = guest_entry & !(2 * 1024 * 1024 - 1);
    let mem_end = ((guest_stack + 2 * 1024 * 1024 - 1) / (2 * 1024 * 1024)) * (2 * 1024 * 1024);
    let mem_size = mem_end - mem_start;

    vm.init_memory(mem_start, mem_size);

    // Add vCPU
    match vm.add_vcpu(guest_entry, guest_stack) {
        Ok(vcpu_id) => {
            hypervisor::log_info!("[MMIO TEST] Created vCPU {}\n", vcpu_id);
        }
        Err(e) => {
            hypervisor::log_info!("[ERROR] Failed to create vCPU: {}\n", e);
            return;
        }
    }

    // Run the VM
    hypervisor::log_info!("[MMIO TEST] Starting guest...\n");
    hypervisor::log_info!("[GUEST OUTPUT] ");

    match vm.run() {
        Ok(()) => {
            hypervisor::log_info!("[MMIO TEST] Guest exited successfully\n");
        }
        Err(e) => {
            hypervisor::log_info!("[ERROR] Guest failed: {}\n", e);
        }
    }

    hypervisor::log_info!("\n[MMIO TEST] Test complete!\n");
    hypervisor::log_info!("========================================\n\n");
}
