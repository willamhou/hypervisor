use hypervisor::arch::aarch64::peripherals::timer;
///! Simple timer interrupt test at EL2
///!
///! This test demonstrates handling timer interrupts at EL2 (hypervisor level)

/// Run a simple timer interrupt test
#[allow(dead_code)]
pub fn run_timer_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Timer Interrupt Test\n");
    hypervisor::log_info!("========================================\n\n");

    hypervisor::log_info!("[TIMER TEST] Configuring timer for 100ms interrupt...\n");

    // Get timer frequency
    let freq = timer::get_frequency();
    hypervisor::log_info!("[TIMER TEST] Timer frequency: {} Hz\n", freq);

    // Calculate ticks for 100ms
    // 100ms = 0.1s, so ticks = freq * 0.1
    let ticks_100ms = (freq / 10) as u32;
    hypervisor::log_info!(
        "[TIMER TEST] Setting timer for {} ticks (100ms)\n",
        ticks_100ms
    );

    // Note: Skipping GIC configuration for now as it requires MMIO access
    // The GIC should be in a usable state by default in QEMU
    hypervisor::log_info!("[TIMER TEST] Using default GIC configuration (skipping MMIO access)\n");

    // Enable the timer
    hypervisor::log_info!("[TIMER TEST] Enabling timer...\n");
    timer::enable_timer(ticks_100ms);

    hypervisor::log_info!("[TIMER TEST] Waiting for interrupt (WFI)...\n");

    // Enable interrupts at EL2 by clearing PSTATE.I
    unsafe {
        core::arch::asm!("msr daifclr, #2"); // Clear I bit (enable IRQ)
    }

    // Wait for interrupt (or timer expiry)
    let start_counter = timer::get_counter();
    for i in 0..10 {
        hypervisor::log_info!(
            "[TIMER TEST] Iteration {}, timer status: {:#018x}\n",
            i,
            timer::get_ctl()
        );

        // Check if timer fired
        if timer::is_pending() {
            hypervisor::log_info!("[TIMER TEST] Timer interrupt pending detected!\n");
            timer::disable_timer();
            break;
        }

        // Check if enough time has passed
        let current_counter = timer::get_counter();
        let elapsed = current_counter - start_counter;
        if elapsed > (ticks_100ms as u64) {
            hypervisor::log_info!("[TIMER TEST] Timer expired (counter check)\n");
            hypervisor::log_info!("[TIMER TEST] Elapsed ticks: {}\n", elapsed);
            timer::disable_timer();
            break;
        }

        // Small delay via busy loop
        for _ in 0..1000000 {
            unsafe {
                core::arch::asm!("nop");
            }
        }
    }

    // Disable interrupts again
    unsafe {
        core::arch::asm!("msr daifset, #2"); // Set I bit (disable IRQ)
    }

    hypervisor::log_info!("\n[TIMER TEST] Test complete\n");
    hypervisor::log_info!("========================================\n\n");
}
