///! Simple timer interrupt test at EL2
///! 
///! This test demonstrates handling timer interrupts at EL2 (hypervisor level)

use hypervisor::uart_puts;
use hypervisor::arch::aarch64::peripherals::timer;

/// Run a simple timer interrupt test
#[allow(dead_code)]
pub fn run_timer_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Timer Interrupt Test\n");
    uart_puts(b"========================================\n\n");
    
    uart_puts(b"[TIMER TEST] Configuring timer for 100ms interrupt...\n");
    
    // Get timer frequency
    let freq = timer::get_frequency();
    uart_puts(b"[TIMER TEST] Timer frequency: ");
    hypervisor::uart_put_u64(freq);
    uart_puts(b" Hz\n");
    
    // Calculate ticks for 100ms
    // 100ms = 0.1s, so ticks = freq * 0.1
    let ticks_100ms = (freq / 10) as u32;
    uart_puts(b"[TIMER TEST] Setting timer for ");
    hypervisor::uart_put_u64(ticks_100ms as u64);
    uart_puts(b" ticks (100ms)\n");
    
    // Note: Skipping GIC configuration for now as it requires MMIO access
    // The GIC should be in a usable state by default in QEMU
    uart_puts(b"[TIMER TEST] Using default GIC configuration (skipping MMIO access)\n");
    
    // Enable the timer
    uart_puts(b"[TIMER TEST] Enabling timer...\n");
    timer::enable_timer(ticks_100ms);
    
    uart_puts(b"[TIMER TEST] Waiting for interrupt (WFI)...\n");
    
    // Enable interrupts at EL2 by clearing PSTATE.I
    unsafe {
        core::arch::asm!("msr daifclr, #2"); // Clear I bit (enable IRQ)
    }
    
    // Wait for interrupt (or timer expiry)
    let start_counter = timer::get_counter();
    for i in 0..10 {
        uart_puts(b"[TIMER TEST] Iteration ");
        hypervisor::uart_put_u64(i);
        uart_puts(b", timer status: 0x");
        hypervisor::uart_put_hex(timer::get_ctl());
        uart_puts(b"\n");
        
        // Check if timer fired
        if timer::is_pending() {
            uart_puts(b"[TIMER TEST] Timer interrupt pending detected!\n");
            timer::disable_timer();
            break;
        }
        
        // Check if enough time has passed
        let current_counter = timer::get_counter();
        let elapsed = current_counter - start_counter;
        if elapsed > (ticks_100ms as u64) {
            uart_puts(b"[TIMER TEST] Timer expired (counter check)\n");
            uart_puts(b"[TIMER TEST] Elapsed ticks: ");
            hypervisor::uart_put_u64(elapsed);
            uart_puts(b"\n");
            timer::disable_timer();
            break;
        }
        
        // Small delay via busy loop
        for _ in 0..1000000 {
            unsafe { core::arch::asm!("nop"); }
        }
    }
    
    // Disable interrupts again
    unsafe {
        core::arch::asm!("msr daifset, #2"); // Set I bit (disable IRQ)
    }
    
    uart_puts(b"\n[TIMER TEST] Test complete\n");
    uart_puts(b"========================================\n\n");
}
