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

/// Secondary pCPU entry point (called from boot.S after PSCI CPU_ON start).
///
/// Sets up EL2 state (VBAR, HCR, Stage-2, GIC) then enters an idle loop
/// waiting for guest PSCI CPU_ON requests.
#[cfg(feature = "multi_pcpu")]
#[no_mangle]
pub extern "C" fn rust_main_secondary(cpu_id: usize) -> ! {
    use hypervisor::arch::aarch64::hypervisor::exception;
    use hypervisor::arch::aarch64::peripherals::gicv3;
    use hypervisor::arch::aarch64::defs::*;
    use core::sync::atomic::Ordering;

    // Early debug: write 'S' directly to UART via assembly
    // This verifies the CPU actually entered rust_main_secondary
    unsafe {
        core::arch::asm!(
            "mov x1, #0x09000000",
            "mov w2, #0x53",  // 'S'
            "strb w2, [x1]",
            out("x1") _,
            out("x2") _,
            options(nostack),
        );
    }

    uart_puts_local(b"[SMP] pCPU ");
    print_digit(cpu_id as u8);
    uart_puts_local(b" started\n");

    // 1. Set VBAR_EL2 (same exception vectors as primary)
    exception::init();

    // 2. Set VTTBR_EL2 / VTCR_EL2 (shared Stage-2 from primary)
    let vttbr = hypervisor::global::SHARED_VTTBR.load(Ordering::Acquire);
    let vtcr = hypervisor::global::SHARED_VTCR.load(Ordering::Acquire);
    unsafe {
        core::arch::asm!(
            "msr vtcr_el2, {vtcr}",
            "msr vttbr_el2, {vttbr}",
            "isb",
            vtcr = in(reg) vtcr,
            vttbr = in(reg) vttbr,
            options(nostack, nomem),
        );
    }

    // 3. HCR_EL2 is set by exception::init(). Enable Stage-2 and clear TWI.
    unsafe {
        let mut hcr: u64;
        core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr);
        hcr |= HCR_VM;     // Enable Stage-2
        hcr &= !HCR_TWI;   // Don't trap WFI (multi-pCPU: WFI passthrough)
        core::arch::asm!("msr hcr_el2, {}", "isb", in(reg) hcr);
    }

    // 4. Configure CPTR_EL2 / MDCR_EL2 (don't trap FP/SIMD/debug)
    unsafe {
        core::arch::asm!(
            "mrs x0, cptr_el2",
            "bic x0, x0, {cptr_tz}",
            "bic x0, x0, {cptr_tfp}",
            "bic x0, x0, {cptr_tsm}",
            "bic x0, x0, {cptr_tcpac}",
            "msr cptr_el2, x0",
            "msr mdcr_el2, xzr",
            "isb",
            cptr_tz = const CPTR_TZ,
            cptr_tfp = const CPTR_TFP,
            cptr_tsm = const CPTR_TSM,
            cptr_tcpac = const CPTR_TCPAC,
            out("x0") _,
            options(nostack),
        );
    }

    // 5. Initialize per-pCPU GIC (system register interface + virtual interface)
    gicv3::init();

    // 6. Set PerCpuContext
    let percpu = hypervisor::percpu::this_cpu();
    percpu.vcpu_id = cpu_id;

    uart_puts_local(b"[SMP] pCPU ");
    print_digit(cpu_id as u8);
    uart_puts_local(b" ready, waiting for CPU_ON\n");

    // 7. Idle loop: WFE until PSCI CPU_ON sets our request
    loop {
        unsafe { core::arch::asm!("wfe") };
        if let Some((entry, ctx)) =
            hypervisor::global::PENDING_CPU_ON_PER_VCPU[cpu_id].take()
        {
            uart_puts_local(b"[SMP] pCPU ");
            print_digit(cpu_id as u8);
            uart_puts_local(b" got CPU_ON, entering guest\n");
            secondary_enter_guest(cpu_id, entry, ctx);
        }
    }
}

/// Set up vCPU and enter guest loop for a secondary pCPU.
/// This function never returns — the pCPU runs this vCPU forever (1:1 affinity).
#[cfg(feature = "multi_pcpu")]
fn secondary_enter_guest(cpu_id: usize, entry: u64, ctx_id: u64) -> ! {
    use hypervisor::vcpu::Vcpu;
    use hypervisor::arch::aarch64::defs::*;
    use hypervisor::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;
    use hypervisor::platform;
    use core::sync::atomic::Ordering;

    // Wake this CPU's GICR
    if cpu_id < platform::GICR_RD_BASES.len() {
        let rd_base = platform::GICR_RD_BASES[cpu_id];
        let waker_addr = (rd_base + platform::GICR_WAKER_OFF) as *mut u32;
        unsafe {
            let mut waker = core::ptr::read_volatile(waker_addr);
            waker &= !(1 << 1); // Clear ProcessorSleep
            core::ptr::write_volatile(waker_addr, waker);
            loop {
                let w = core::ptr::read_volatile(waker_addr);
                if w & (1 << 2) == 0 { break; }
            }
        }
    }

    // Create vCPU
    let mut vcpu = Vcpu::new(cpu_id, entry, 0);
    vcpu.context_mut().gp_regs.x0 = ctx_id;
    vcpu.context_mut().spsr_el2 = SPSR_EL1H_DAIF_MASKED;
    vcpu.arch_state_mut().sctlr_el1 = 0x30D0_0800;
    vcpu.arch_state_mut().cpacr_el1 = 3 << 20;
    vcpu.arch_state_mut().init_for_vcpu(cpu_id);

    // Mark vCPU online (current_vcpu_id() uses MPIDR in multi_pcpu mode)
    hypervisor::global::VCPU_ONLINE_MASK.fetch_or(1 << cpu_id, Ordering::Release);

    // Reset exception counters for this pCPU
    hypervisor::arch::aarch64::hypervisor::exception::reset_exception_counters();

    uart_puts_local(b"[SMP] vCPU ");
    print_digit(cpu_id as u8);
    uart_puts_local(b" entering guest at 0x");
    hypervisor::uart_put_hex(entry);
    uart_puts_local(b"\n");

    // Simple run loop: inject pending, enter guest, handle exit
    loop {
        // Ensure PPI 27 (virtual timer) stays enabled at the physical GICR
        hypervisor::vm::ensure_vtimer_enabled(cpu_id);

        // Inject pending SGIs
        let sgi_bits = hypervisor::global::PENDING_SGIS[cpu_id].swap(0, Ordering::Acquire);
        if sgi_bits != 0 {
            let arch = vcpu.arch_state_mut();
            for sgi in 0..16u32 {
                if sgi_bits & (1 << sgi) == 0 { continue; }
                for lr in arch.ich_lr.iter_mut() {
                    if (*lr >> LR_STATE_SHIFT) & LR_STATE_MASK == 0 {
                        *lr = (GicV3VirtualInterface::LR_STATE_PENDING << LR_STATE_SHIFT)
                            | LR_GROUP1_BIT
                            | ((IRQ_DEFAULT_PRIORITY as u64) << LR_PRIORITY_SHIFT)
                            | (sgi as u64);
                        break;
                    }
                }
            }
        }

        // Inject pending SPIs
        let spi_bits = hypervisor::global::PENDING_SPIS[cpu_id].swap(0, Ordering::Acquire);
        if spi_bits != 0 {
            let arch = vcpu.arch_state_mut();
            for bit in 0..32u32 {
                if spi_bits & (1 << bit) == 0 { continue; }
                let intid = bit + 32;
                for lr in arch.ich_lr.iter_mut() {
                    if (*lr >> LR_STATE_SHIFT) & LR_STATE_MASK == 0 {
                        *lr = (GicV3VirtualInterface::LR_STATE_PENDING << LR_STATE_SHIFT)
                            | LR_GROUP1_BIT
                            | ((IRQ_DEFAULT_PRIORITY as u64) << LR_PRIORITY_SHIFT)
                            | (intid as u64);
                        break;
                    }
                }
            }
        }

        // Enter guest
        match vcpu.run() {
            Ok(()) => {
                // Normal exit — loop back, re-enter guest
            }
            Err("WFI") => {
                // WFI: execute real WFI — pCPU idles until next interrupt
                unsafe { core::arch::asm!("wfi") };
            }
            Err(_) => {
                // Other exit — loop back
            }
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
