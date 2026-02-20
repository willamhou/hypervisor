use super::super::defs::*;
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
const TIMER_ENABLE: u64 = 1 << 0; // Enable timer
#[allow(dead_code)]
const TIMER_IMASK: u64 = 1 << 1; // Interrupt mask (1 = masked)
const TIMER_ISTATUS: u64 = 1 << 2; // Interrupt status (read-only)

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
    let tval: u64;
    unsafe {
        asm!("mrs {0:x}, cntv_tval_el0", out(reg) tval);
    }
    tval as u32
}

/// Write the virtual timer countdown value (ticks until interrupt)
pub fn set_tval(tval: u32) {
    unsafe {
        asm!("msr cntv_tval_el0, {0:x}", in(reg) tval as u64);
    }
}

/// Configure hypervisor control of timers
pub fn init_hypervisor_timer() {
    let mut cnthctl: u64;
    unsafe {
        asm!("mrs {}, cnthctl_el2", out(reg) cnthctl);
    }

    // Allow EL1 access to physical counter and timer
    cnthctl |= CNTHCTL_EL1PCTEN | CNTHCTL_EL1PCEN;

    unsafe {
        asm!("msr cnthctl_el2, {}", in(reg) cnthctl);
    }
}

/// Configure timer access for guest VM
pub fn init_guest_timer() {
    let mut cnthctl: u64;
    unsafe {
        asm!("mrs {}, cnthctl_el2", out(reg) cnthctl);
    }

    // Allow EL1 access to physical counter
    cnthctl |= CNTHCTL_EL1PCTEN;

    unsafe {
        asm!("msr cnthctl_el2, {}", in(reg) cnthctl);
        asm!("isb");
    }

    // Set virtual timer offset to 0
    unsafe {
        asm!("msr cntvoff_el2, xzr");
        asm!("isb");
    }
}

/// Check if the guest's virtual timer is enabled and pending
pub fn is_guest_vtimer_pending() -> bool {
    let ctl: u64;
    unsafe {
        asm!("mrs {}, cntv_ctl_el0", out(reg) ctl);
    }
    let enabled = (ctl & TIMER_ENABLE) != 0;
    let pending = (ctl & TIMER_ISTATUS) != 0;
    let masked = (ctl & TIMER_IMASK) != 0;
    enabled && pending && !masked
}

/// Mask the guest's virtual timer interrupt
pub fn mask_guest_vtimer() {
    let mut ctl: u64;
    unsafe {
        asm!("mrs {}, cntv_ctl_el0", out(reg) ctl);
    }
    ctl |= TIMER_IMASK;
    unsafe {
        asm!("msr cntv_ctl_el0, {}", in(reg) ctl);
        asm!("isb");
    }
}

/// Enable the virtual timer with a timeout in ticks
pub fn enable_timer(ticks: u32) {
    set_ctl(0);
    set_tval(ticks);
    set_ctl(TIMER_ENABLE);
}

/// Disable the virtual timer
pub fn disable_timer() {
    set_ctl(0);
}

/// Arm the EL2 hypervisor physical timer (CNTHP) for preemption.
///
/// This timer is independent of the guest virtual timer and fires INTID 26.
/// Used as a preemption watchdog to guarantee context switches even when
/// the guest timer is masked (e.g., during multi_cpu_stop with IRQs disabled).
pub fn arm_preemption_timer() {
    // 10ms at counter frequency
    let ticks = get_frequency() / 100;
    unsafe {
        asm!("msr cnthp_tval_el2, {}", in(reg) ticks, options(nostack, nomem));
        asm!("msr cnthp_ctl_el2, {}", in(reg) 1u64, options(nostack, nomem)); // ENABLE=1, IMASK=0
        asm!("isb", options(nostack, nomem));
    }
}

/// Disarm the EL2 hypervisor physical timer.
pub fn disarm_preemption_timer() {
    unsafe {
        asm!("msr cnthp_ctl_el2, {}", in(reg) 0u64, options(nostack, nomem)); // ENABLE=0
        asm!("isb", options(nostack, nomem));
    }
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
