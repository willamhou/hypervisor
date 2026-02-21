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
/// `dtb_addr` is the host DTB address passed by QEMU in x0, preserved by boot.S in x20.
#[no_mangle]
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    uart_puts_local(b"========================================\n");
    uart_puts_local(b"  ARM64 Hypervisor - Sprint 2.4\n");
    uart_puts_local(b"  API Documentation\n");
    uart_puts_local(b"========================================\n");
    uart_puts_local(b"\n");
    uart_puts_local(b"[INIT] Initializing at EL2...\n");

    // Parse host DTB (before heap init — fdt crate does zero-copy parsing)
    uart_puts_local(b"[INIT] Parsing host DTB at 0x");
    hypervisor::uart_put_hex(dtb_addr as u64);
    uart_puts_local(b"...\n");
    hypervisor::dtb::init(dtb_addr);
    if hypervisor::dtb::is_initialized() {
        let pi = hypervisor::dtb::platform_info();
        uart_puts_local(b"[INIT] DTB: cpus=");
        print_digit(pi.num_cpus as u8);
        uart_puts_local(b" ram=0x");
        hypervisor::uart_put_hex(pi.ram_base);
        uart_puts_local(b"+0x");
        hypervisor::uart_put_hex(pi.ram_size);
        uart_puts_local(b" uart=0x");
        hypervisor::uart_put_hex(pi.uart_base);
        uart_puts_local(b"\n");
        uart_puts_local(b"[INIT] DTB: gicd=0x");
        hypervisor::uart_put_hex(pi.gicd_base);
        uart_puts_local(b" gicr=0x");
        hypervisor::uart_put_hex(pi.gicr_base);
        uart_puts_local(b"\n");
    } else {
        uart_puts_local(b"[INIT] DTB: parse failed, using defaults\n");
    }

    // Initialize exception handling
    uart_puts_local(b"[INIT] Setting up exception vector table...\n");
    exception::init();
    uart_puts_local(b"[INIT] Exception handling initialized\n");

    // Initialize GIC - try GICv3 first, fall back to GICv2
    hypervisor::arch::aarch64::peripherals::gicv3::init();

    // Initialize FF-A proxy (probe for real SPMC at EL3)
    #[cfg(feature = "linux_guest")]
    hypervisor::ffa::proxy::init();

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
    unsafe {
        hypervisor::mm::heap::init();
    }
    uart_puts_local(b"[INIT] Heap initialized (16MB at 0x41000000)\n\n");

    // Run the DTB parsing test (validates DTB init above)
    tests::run_dtb_test();

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

    // Run the MMIO instruction decode test
    tests::run_decode_test();

    // Run the GICD emulation test
    tests::run_gicd_test();

    // Run the GICR emulation test
    tests::run_gicr_test();

    // Run the global state test
    tests::run_global_test();

    // Run the interrupt queue test
    tests::run_irq_test();

    // Run the device manager routing test
    tests::run_device_routing_test();

    // Run multi-VM tests
    tests::run_vm_state_isolation_test();
    tests::run_vmid_vttbr_test();
    tests::run_multi_vm_devices_test();
    tests::run_vm_activate_test();

    // Run the NetRxRing test
    tests::run_net_rx_ring_test();

    // Run the VSwitch test
    tests::run_vswitch_test();

    // Run the VirtioNet device test
    tests::run_virtio_net_test();

    // Run the page ownership test
    tests::run_page_ownership_test();

    // Run the PL031 RTC test
    tests::run_pl031_test();

    // Run the FF-A proxy test
    tests::run_ffa_test();

    // Run the SPMC handler dispatch test
    tests::run_spmc_handler_test();

    // Run the SP context state machine test
    tests::run_sp_context_test();

    // Run the Secure Stage-2 config test
    tests::run_secure_stage2_test();

    // Run the guest interrupt injection test (LAST before guest boot — blocks forever)
    // Skip when booting guests since it never returns.
    #[cfg(not(any(feature = "linux_guest", feature = "guest")))]
    tests::run_guest_interrupt_test();

    // Check if we should boot a Zephyr guest
    #[cfg(feature = "guest")]
    {
        use hypervisor::guest_loader::{run_guest, GuestConfig};

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

    // Check if we should boot multiple VMs
    #[cfg(feature = "multi_vm")]
    {
        uart_puts_local(b"\n[INIT] Booting multi-VM mode...\n");

        match hypervisor::guest_loader::run_multi_vm_guests() {
            Ok(()) => {
                uart_puts_local(b"[INIT] Multi-VM exited normally\n");
            }
            Err(e) => {
                uart_puts_local(b"[INIT] Multi-VM error: ");
                uart_puts_local(e.as_bytes());
                uart_puts_local(b"\n");
            }
        }
    }

    // Check if we should boot a Linux guest (single VM)
    #[cfg(all(feature = "linux_guest", not(feature = "multi_vm")))]
    {
        use hypervisor::guest_loader::{run_guest, GuestConfig};

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

/// S-EL2 SPMC entry point called from boot_sel2.S.
/// SPMD passes: x0=TOS_FW_CONFIG, x1=HW_CONFIG, x4=core_id
#[cfg(feature = "sel2")]
#[no_mangle]
pub extern "C" fn rust_main_sel2(
    manifest_addr: usize,
    hw_config_addr: usize,
    _core_id: usize,
) -> ! {
    // 1. Install exception vectors FIRST (before any memory access that could fault)
    exception::init();

    uart_puts_local(b"========================================\n");
    uart_puts_local(b"  ARM64 SPMC - S-EL2\n");
    uart_puts_local(b"========================================\n\n");

    let current_el: u64;
    unsafe {
        core::arch::asm!("mrs {}, CurrentEL", out(reg) current_el);
    }
    let el = (current_el >> 2) & 0x3;
    uart_puts_local(b"[SPMC] Running at EL");
    print_digit(el as u8);
    uart_puts_local(b"\n");

    // 3. Parse SPMC manifest (TOS_FW_CONFIG in x0)
    uart_puts_local(b"[SPMC] Manifest at 0x");
    hypervisor::uart_put_hex(manifest_addr as u64);
    uart_puts_local(b"\n");
    hypervisor::manifest::init(manifest_addr);
    let mi = hypervisor::manifest::manifest_info();
    uart_puts_local(b"[SPMC] spmc_id=0x");
    hypervisor::uart_put_hex(mi.spmc_id as u64);
    uart_puts_local(b" version=");
    print_digit(mi.maj_ver as u8);
    uart_puts_local(b".");
    print_digit(mi.min_ver as u8);
    uart_puts_local(b"\n");

    // 4. Parse hardware DTB (HW_CONFIG in x1)
    uart_puts_local(b"[SPMC] HW config at 0x");
    hypervisor::uart_put_hex(hw_config_addr as u64);
    uart_puts_local(b"\n");
    if hw_config_addr != 0 {
        hypervisor::dtb::init(hw_config_addr);
    } else {
        uart_puts_local(b"[SPMC] No HW config DTB, using QEMU virt defaults\n");
    }

    // 5. Initialize GIC
    hypervisor::arch::aarch64::peripherals::gicv3::init();
    uart_puts_local(b"[SPMC] GIC initialized\n");

    // 5.5. Initialize secure heap (for page table allocation)
    uart_puts_local(b"[SPMC] Initializing secure heap\n");
    unsafe {
        hypervisor::mm::heap::init_at(
            hypervisor::platform::SECURE_HEAP_START,
            hypervisor::platform::SECURE_HEAP_SIZE,
        );
    }

    // 5.6. Build Secure Stage-2 for SP1
    uart_puts_local(b"[SPMC] Building Secure Stage-2 for SP1\n");
    let mapper = hypervisor::secure_stage2::build_sp_stage2(
        hypervisor::platform::SP1_LOAD_ADDR,
        hypervisor::platform::SP1_MEM_SIZE,
    )
    .expect("Failed to build SP Stage-2");
    let s2_config = hypervisor::secure_stage2::SecureStage2Config::new(mapper.l0_addr());
    s2_config.install();

    // Enable Secure Stage-2 by setting HCR_EL2.VM
    unsafe {
        let hcr: u64;
        core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr);
        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr | hypervisor::arch::aarch64::defs::HCR_VM,
        );
    }

    // 5.7. Create SP context and boot SP1
    uart_puts_local(b"[SPMC] Booting SP1 at 0x");
    hypervisor::uart_put_hex(hypervisor::platform::SP1_LOAD_ADDR);
    uart_puts_local(b"\n");

    let mut sp1 = hypervisor::sp_context::SpContext::new(
        hypervisor::platform::SP1_PARTITION_ID,
        hypervisor::platform::SP1_LOAD_ADDR,
        hypervisor::platform::SP1_STACK_TOP,
    );
    sp1.set_vsttbr(s2_config.vsttbr);

    // ERET to SP1 — SP runs, prints hello, calls FFA_MSG_WAIT, traps back
    {
        use hypervisor::arch::aarch64::enter_guest;
        use hypervisor::arch::aarch64::regs::VcpuContext;
        let _exit = unsafe { enter_guest(sp1.vcpu_ctx_mut() as *mut VcpuContext) };
    }

    // SP trapped back — verify it called FFA_MSG_WAIT
    let (x0, _, _, _, _, _, _, _) = sp1.get_args();
    if x0 == hypervisor::ffa::FFA_MSG_WAIT {
        uart_puts_local(b"[SPMC] SP1 booted, now Idle (FFA_MSG_WAIT received)\n");
        sp1.transition_to(hypervisor::sp_context::SpState::Idle)
            .expect("SP1 transition failed");
    } else {
        uart_puts_local(b"[SPMC] WARNING: SP1 did not call FFA_MSG_WAIT, x0=0x");
        hypervisor::uart_put_hex(x0);
        uart_puts_local(b"\n");
    }

    // Store SP1 context globally for dispatch
    hypervisor::sp_context::register_sp(sp1);

    // 6. Signal SPMD: init complete, receive first NWd request
    uart_puts_local(b"[SPMC] Init complete, signaling SPMD via FFA_MSG_WAIT\n");
    let first_req = hypervisor::manifest::signal_spmc_ready();

    // 7. Enter SPMC event loop (does not return)
    hypervisor::spmc_handler::run_event_loop(first_req);
}

/// Secondary pCPU entry point (called from boot.S after PSCI CPU_ON start).
///
/// Sets up EL2 state (VBAR, HCR, Stage-2, GIC) then enters an idle loop
/// waiting for guest PSCI CPU_ON requests.
#[cfg(feature = "multi_pcpu")]
#[no_mangle]
pub extern "C" fn rust_main_secondary(cpu_id: usize) -> ! {
    use core::sync::atomic::Ordering;
    use hypervisor::arch::aarch64::defs::*;
    use hypervisor::arch::aarch64::hypervisor::exception;
    use hypervisor::arch::aarch64::peripherals::gicv3;

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
        hcr |= HCR_VM; // Enable Stage-2
        hcr &= !HCR_TWI; // Don't trap WFI (multi-pCPU: WFI passthrough)
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
    unsafe {
        (*hypervisor::percpu::this_cpu()).vcpu_id = cpu_id;
    }

    uart_puts_local(b"[SMP] pCPU ");
    print_digit(cpu_id as u8);
    uart_puts_local(b" ready, waiting for CPU_ON\n");

    // 7. Idle loop: WFE until PSCI CPU_ON sets our request
    loop {
        unsafe { core::arch::asm!("wfe") };
        if let Some((entry, ctx)) = hypervisor::global::PENDING_CPU_ON_PER_VCPU[cpu_id].take() {
            uart_puts_local(b"[SMP] pCPU ");
            print_digit(cpu_id as u8);
            uart_puts_local(b" got CPU_ON, entering guest\n");
            secondary_enter_guest(cpu_id, entry, ctx);
        }
    }
}

/// Set up vCPU and enter guest loop for a secondary pCPU.
/// Returns if the vCPU terminates (CPU_OFF/SYSTEM_OFF/SYSTEM_RESET),
/// allowing the pCPU to return to the idle loop for potential reuse.
#[cfg(feature = "multi_pcpu")]
fn secondary_enter_guest(cpu_id: usize, entry: u64, ctx_id: u64) {
    use core::sync::atomic::Ordering;
    use hypervisor::arch::aarch64::defs::*;
    use hypervisor::platform;
    use hypervisor::vcpu::Vcpu;

    // Wake this CPU's GICR
    if cpu_id < platform::num_cpus() {
        let rd_base = hypervisor::dtb::gicr_rd_base(cpu_id);
        let waker_addr = (rd_base + platform::GICR_WAKER_OFF) as *mut u32;
        unsafe {
            let mut waker = core::ptr::read_volatile(waker_addr);
            waker &= !(1 << 1); // Clear ProcessorSleep
            core::ptr::write_volatile(waker_addr, waker);
            loop {
                let w = core::ptr::read_volatile(waker_addr);
                if w & (1 << 2) == 0 {
                    break;
                }
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
    hypervisor::global::vm_state(0)
        .vcpu_online_mask
        .fetch_or(1 << cpu_id, Ordering::Release);

    // Reset exception counters for this pCPU
    hypervisor::arch::aarch64::hypervisor::exception::reset_exception_counters();

    uart_puts_local(b"[SMP] vCPU ");
    print_digit(cpu_id as u8);
    uart_puts_local(b" entering guest at 0x");
    hypervisor::uart_put_hex(entry);
    uart_puts_local(b"\n");

    // Run loop: inject pending, enter guest, handle exit.
    // Uses shared inject_pending_sgis/spis helpers (with re-queue on LR full).
    loop {
        // Ensure PPI 27 (virtual timer) stays enabled at the physical GICR
        hypervisor::vm::ensure_vtimer_enabled(cpu_id);

        // Inject pending SGIs and SPIs (shared with run_vcpu)
        hypervisor::vm::inject_pending_sgis(&mut vcpu);
        hypervisor::vm::inject_pending_spis(&mut vcpu);

        // Enter guest
        match vcpu.run() {
            Ok(()) => {
                // Check for terminal PSCI exits (CPU_OFF, SYSTEM_OFF, SYSTEM_RESET)
                if hypervisor::global::vm_state(0).terminal_exit[cpu_id]
                    .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    uart_puts_local(b"[SMP] vCPU ");
                    print_digit(cpu_id as u8);
                    uart_puts_local(b" terminal exit\n");
                    hypervisor::global::vm_state(0)
                        .vcpu_online_mask
                        .fetch_and(!(1 << cpu_id), Ordering::Release);
                    // Return to idle loop — pCPU can be reused for future CPU_ON
                    break;
                }
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
    // Returns to idle loop in rust_main_secondary for potential CPU_ON reuse
}

/// Print a single digit (0-9)
fn print_digit(digit: u8) {
    let ch = b'0' + digit;
    uart_puts_local(&[ch]);
}

/// Panic handler - required for no_std
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    uart_puts_local(b"\n!!! PANIC !!!\n");
    if let Some(location) = info.location() {
        uart_puts_local(b"  at ");
        uart_puts_local(location.file().as_bytes());
        uart_puts_local(b":");
        print_u32(location.line());
        uart_puts_local(b"\n");
    }
    if let Some(msg) = info.message().as_str() {
        uart_puts_local(b"  ");
        uart_puts_local(msg.as_bytes());
        uart_puts_local(b"\n");
    }

    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

/// Print a u32 value in decimal
fn print_u32(mut val: u32) {
    if val == 0 {
        uart_puts_local(b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 0;
    while val > 0 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        uart_puts_local(&[buf[i]]);
    }
}
