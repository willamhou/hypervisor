/// ARM Generic Timer support
/// 
/// ARM provides several timers:
/// - Physical Timer (EL1): Accessed via CNTPCT_EL0, CNTP_*
/// - Virtual Timer (EL1): Accessed via CNTVCT_EL0, CNTV_*
/// - Hypervisor Timer (EL2): Accessed via CNTHCTL_EL2
/// 
/// For guest VMs, we use the Virtual Timer which generates PPI 27.

use core::arch::asm;

/// Timer control register bits
const TIMER_ENABLE: u64 = 1 << 0;    // Enable timer
#[allow(dead_code)]
const TIMER_IMASK: u64 = 1 << 1;     // Interrupt mask (1 = masked)
const TIMER_ISTATUS: u64 = 1 << 2;   // Interrupt status (read-only)

/// Read the virtual counter frequency
pub fn get_frequency() -> u64 {
    let freq: u64;
    unsafe {
        asm!("mrs {}, cntfrq_el0", out(reg) freq);
    }
    freq
}

/// Read the virtual counter value
pub fn get_counter() -> u64 {
    let count: u64;
    unsafe {
        asm!("mrs {}, cntvct_el0", out(reg) count);
    }
    count
}

/// Read the virtual timer control register
pub fn get_ctl() -> u64 {
    let ctl: u64;
    unsafe {
        asm!("mrs {}, cntv_ctl_el0", out(reg) ctl);
    }
    ctl
}

/// Write the virtual timer control register
pub fn set_ctl(ctl: u64) {
    unsafe {
        asm!("msr cntv_ctl_el0, {}", in(reg) ctl);
    }
}

/// Read the virtual timer compare value
pub fn get_cval() -> u64 {
    let cval: u64;
    unsafe {
        asm!("mrs {}, cntv_cval_el0", out(reg) cval);
    }
    cval
}

/// Write the virtual timer compare value
pub fn set_cval(cval: u64) {
    unsafe {
        asm!("msr cntv_cval_el0, {}", in(reg) cval);
    }
}

/// Read the virtual timer countdown value (ticks until interrupt)
pub fn get_tval() -> u32 {
    let tval: u32;
    unsafe {
        asm!("mrs {}, cntv_tval_el0", out(reg) tval);
    }
    tval
}

/// Write the virtual timer countdown value (ticks until interrupt)
pub fn set_tval(tval: u32) {
    unsafe {
        asm!("msr cntv_tval_el0, {}", in(reg) tval);
    }
}

/// Configure hypervisor control of timers
pub fn init_hypervisor_timer() {
    // Read CNTHCTL_EL2
    let mut cnthctl: u64;
    unsafe {
        asm!("mrs {}, cnthctl_el2", out(reg) cnthctl);
    }

    // EL1PCTEN (bit 0): EL0/EL1 can access physical counter
    // EL1PCEN (bit 1): EL0/EL1 can access physical timer
    // EVNTEN (bit 2): Enable event stream
    // EVNTDIR (bit 3): Event stream direction
    // EVNTI (bits 7:4): Event stream divider
    cnthctl |= (1 << 0) | (1 << 1);  // Allow EL1 access to physical counter/timer

    unsafe {
        asm!("msr cnthctl_el2, {}", in(reg) cnthctl);
    }
}

/// Enable the virtual timer with a timeout in ticks
pub fn enable_timer(ticks: u32) {
    // Disable timer first
    set_ctl(0);
    
    // Set timeout
    set_tval(ticks);
    
    // Enable timer with interrupts unmasked
    set_ctl(TIMER_ENABLE);
}

/// Disable the virtual timer
pub fn disable_timer() {
    set_ctl(0);
}

/// Check if timer interrupt is pending
pub fn is_pending() -> bool {
    let ctl = get_ctl();
    (ctl & TIMER_ISTATUS) != 0
}

/// Format a simple timer info string
pub fn print_timer_info() {
    let freq = get_frequency();
    let count = get_counter();
    let ctl = get_ctl();
    
    crate::uart_puts(b"[TIMER] Frequency: ");
    crate::uart_put_u64(freq);
    crate::uart_puts(b" Hz\n");
    
    crate::uart_puts(b"[TIMER] Counter: ");
    crate::uart_put_u64(count);
    crate::uart_puts(b"\n");
    
    crate::uart_puts(b"[TIMER] Control: 0x");
    crate::uart_put_hex(ctl);
    crate::uart_puts(b"\n");
}
