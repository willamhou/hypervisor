//! ARM64 Exception Handling
//! 
//! This module provides the interface to the exception vector table
//! and exception handlers for EL2.

use crate::arch::aarch64::regs::VcpuContext;

// External assembly functions defined in exception.S
extern "C" {
    /// Exception vector table base address
    /// 
    /// This is the base address of the exception vector table that should
    /// be loaded into VBAR_EL2.
    pub static exception_vector_table: u8;
    
    /// Enter guest VM
    /// 
    /// This function is implemented in assembly and will:
    /// 1. Restore guest context from VcpuContext
    /// 2. Execute ERET to enter the guest at EL1
    /// 
    /// When the guest exits (due to exception), this function will:
    /// 1. Save guest context to VcpuContext
    /// 2. Return to the caller
    pub fn enter_guest(context: *mut VcpuContext) -> u64;
}

/// Initialize EL2 exception handling
/// 
/// This sets up the exception vector table for EL2 by:
/// 1. Loading VBAR_EL2 with the exception vector table address
/// 2. Configuring HCR_EL2 to enable necessary traps
pub fn init() {
    unsafe {
        // Get the address of the exception vector table
        let vbar = &exception_vector_table as *const _ as u64;
        
        // Load VBAR_EL2 with the exception vector table address
        core::arch::asm!(
            "msr vbar_el2, {vbar}",
            "isb",
            vbar = in(reg) vbar,
            options(nostack, nomem),
        );
        
        // Configure HCR_EL2 (Hypervisor Configuration Register)
        // Bit 31 (RW) = 1: EL1 is AArch64
        // Bit 27 (TGE) = 0: Guest OS runs at EL1
        // Bit 12 (TWI) = 1: Trap WFI to EL2
        // Bit 13 (TWE) = 1: Trap WFE to EL2
        // Bit 3 (AMO) = 1: Route SError to EL2
        // Bit 4 (IMO) = 1: Route IRQ to EL2
        // Bit 5 (FMO) = 1: Route FIQ to EL2
        let hcr: u64 = (1 << 31) | // RW
                       (1 << 12) | // TWI
                       (1 << 13) | // TWE
                       (1 << 3)  | // AMO
                       (1 << 4)  | // IMO
                       (1 << 5);   // FMO
        
        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr,
            options(nostack, nomem),
        );
    }
}

/// Exception handler called from assembly
/// 
/// This is called by the exception vector table when an exception occurs
/// while executing a guest VM.
/// 
/// # Arguments
/// * `context` - The saved vCPU context at the time of the exception
/// 
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Exit to host
#[no_mangle]
pub extern "C" fn handle_exception(context: &mut VcpuContext) -> bool {
    // Read ESR_EL2 to determine exception cause
    let esr: u64;
    unsafe {
        core::arch::asm!(
            "mrs {esr}, esr_el2",
            esr = out(reg) esr,
            options(nostack, nomem),
        );
    }
    context.sys_regs.esr_el2 = esr;
    
    // Read FAR_EL2 for fault address
    let far: u64;
    unsafe {
        core::arch::asm!(
            "mrs {far}, far_el2",
            far = out(reg) far,
            options(nostack, nomem),
        );
    }
    context.sys_regs.far_el2 = far;
    
    // Get exit reason
    let exit_reason = context.exit_reason();
    
    // For now, just handle basic cases
    use crate::arch::aarch64::regs::ExitReason;
    use crate::uart_puts;
    
    match exit_reason {
        ExitReason::WfiWfe => {
            // WFI/WFE: Just advance PC and continue
            // uart_puts(b"[VCPU] WFI/WFE trapped\n");  // Disabled for cleaner output
            context.pc += 4; // Skip the WFI/WFE instruction
            true // Continue
        }
        
        ExitReason::HvcCall => {
            // HVC: Hypercall from guest
            // x0 contains the hypercall number
            let should_continue = handle_hypercall(context);
            // Don't advance PC - ELR_EL2 already points to the next instruction
            should_continue
        }
        
        ExitReason::TrapMsrMrs => {
            uart_puts(b"[VCPU] MSR/MRS trap\n");
            // For now, skip the instruction
            context.pc += 4;
            true // Continue
        }
        
        ExitReason::InstructionAbort => {
            uart_puts(b"[VCPU] Instruction abort at 0x");
            print_hex(context.sys_regs.far_el2);
            uart_puts(b"\n");
            // This is a fatal error
            false // Exit
        }
        
        ExitReason::DataAbort => {
            uart_puts(b"[VCPU] Data abort at 0x");
            print_hex(context.sys_regs.far_el2);
            uart_puts(b"\n");
            // This is a fatal error
            false // Exit
        }
        
        ExitReason::Unknown | ExitReason::Other(_) => {
            // Check if this is an IRQ or FIQ from the vector entry
            // ESR_EL2 might not have meaningful data for IRQ/FIQ
            // We'll try to handle it as an interrupt
            if is_irq_or_fiq() {
                return handle_irq(context);
            }
            
            uart_puts(b"[VCPU] Unknown exception, ESR=0x");
            print_hex(esr);
            uart_puts(b"\n");
            // This is a fatal error
            false // Exit
        }
    }
}

/// Handle hypercalls from guest
/// 
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Exit to host
fn handle_hypercall(context: &mut VcpuContext) -> bool {
    use crate::uart_puts;
    let hypercall_num = context.gp_regs.x0;
    
    match hypercall_num {
        0 => {
            // Hypercall 0: Print character
            // x1 contains the character
            let ch = context.gp_regs.x1 as u8;
            unsafe {
                let uart_base = 0x09000000usize;
                core::arch::asm!(
                    "str {val:w}, [{addr}]",
                    addr = in(reg) uart_base,
                    val = in(reg) ch as u32,
                    options(nostack),
                );
            }
            context.gp_regs.x0 = 0; // Success
            true // Continue
        }
        
        1 => {
            // Hypercall 1: Exit guest
            uart_puts(b"\n[VCPU] Guest requested exit\n");
            context.gp_regs.x0 = 0; // Success
            false // Exit - guest wants to terminate
        }
        
        _ => {
            // Unknown hypercall
            uart_puts(b"\n[VCPU] Unknown hypercall: 0x");
            print_hex(hypercall_num);
            uart_puts(b"\n");
            context.gp_regs.x0 = !0; // Error
            false // Exit on error
        }
    }
}

/// Check if current exception is IRQ or FIQ
fn is_irq_or_fiq() -> bool {
    // For now, we'll assume any exception with ESR_EL2.EC = 0 might be an interrupt
    // A better approach would be to track which vector entry was used
    // TODO: Pass vector information from assembly
    true // Optimistic: try to handle as interrupt
}

/// Handle IRQ interrupts
/// 
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Exit to host
fn handle_irq(context: &mut VcpuContext) -> bool {
    use crate::arch::aarch64::gic::{GICC, VTIMER_IRQ};
    use crate::arch::aarch64::timer;
    use crate::uart_puts;
    
    // Acknowledge the interrupt
    let irq = GICC.acknowledge();
    
    // Check for spurious interrupt (ID 1023)
    if irq == 1023 {
        // Spurious interrupt, just continue
        return true;
    }
    
    uart_puts(b"[IRQ] Received IRQ ");
    print_u32(irq);
    uart_puts(b"\n");
    
    // Handle specific interrupts
    match irq {
        VTIMER_IRQ => {
            // Virtual timer interrupt
            uart_puts(b"[IRQ] Virtual timer interrupt\n");
            
            // Disable the timer to prevent continuous interrupts
            timer::disable_timer();
            
            // Inject interrupt to guest by setting pending interrupt in guest context
            // For now, we'll just print a message
            // TODO: Implement proper interrupt injection
        }
        
        _ => {
            uart_puts(b"[IRQ] Unhandled IRQ: ");
            print_u32(irq);
            uart_puts(b"\n");
        }
    }
    
    // Signal end of interrupt
    GICC.end_of_interrupt(irq);
    
    // Continue running guest
    true
}

/// Helper function to print a 32-bit decimal value
fn print_u32(value: u32) {
    use crate::uart_puts;
    
    if value == 0 {
        uart_puts(b"0");
        return;
    }
    
    let mut buffer = [0u8; 10];
    let mut num = value;
    let mut i = 0;
    
    while num > 0 {
        buffer[i] = b'0' + (num % 10) as u8;
        num /= 10;
        i += 1;
    }
    
    // Print in reverse order
    for j in (0..i).rev() {
        unsafe {
            let uart_base = 0x09000000usize;
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) buffer[j] as u32,
                options(nostack),
            );
        }
    }
}

/// Helper function to print a 64-bit hex value
fn print_hex(value: u64) {
    use crate::uart_puts;
    
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut buffer = [0u8; 16];
    
    for i in 0..16 {
        let nibble = ((value >> ((15 - i) * 4)) & 0xF) as usize;
        buffer[i] = HEX_CHARS[nibble];
    }
    
    uart_puts(&buffer);
}
