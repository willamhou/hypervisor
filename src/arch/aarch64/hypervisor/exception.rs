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

// Exception loop prevention: track consecutive exceptions
static mut EXCEPTION_COUNT: u32 = 0;
const MAX_CONSECUTIVE_EXCEPTIONS: u32 = 100;

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
    
    // Check for exception loop
    unsafe {
        EXCEPTION_COUNT += 1;
        if EXCEPTION_COUNT > MAX_CONSECUTIVE_EXCEPTIONS {
            uart_puts(b"\n[FATAL] Too many consecutive exceptions, halting system\n");
            uart_puts(b"[DEBUG] ESR_EL2=0x");
            print_hex(esr);
            uart_puts(b" FAR_EL2=0x");
            print_hex(far);
            uart_puts(b" PC=0x");
            print_hex(context.pc);
            uart_puts(b"\n");
            // Halt the system completely to prevent further execution
            loop {
                core::arch::asm!("wfe");
            }
        }
    }
    
    // Get exit reason
    let exit_reason = context.exit_reason();

    // Count exception types for debugging
    use crate::arch::aarch64::regs::ExitReason as ER;
    static mut TOTAL_EXCEPTION_COUNT: u64 = 0;
    unsafe {
        TOTAL_EXCEPTION_COUNT += 1;
        // Print first few total exceptions
        if TOTAL_EXCEPTION_COUNT <= 10 {
            uart_puts(b"[EXC] #");
            print_hex(TOTAL_EXCEPTION_COUNT);
            uart_puts(b" type=");
            match exit_reason {
                ER::WfiWfe => uart_puts(b"WFI"),
                ER::HvcCall => uart_puts(b"HVC"),
                ER::DataAbort => uart_puts(b"DAB"),
                ER::TrapMsrMrs => uart_puts(b"MSR"),
                _ => uart_puts(b"OTHER"),
            }
            uart_puts(b" PC=0x");
            print_hex(context.pc);
            uart_puts(b"\n");
        }
    }

    // For now, just handle basic cases
    use crate::arch::aarch64::regs::ExitReason;
    use crate::uart_puts;

    match exit_reason {
        ExitReason::WfiWfe => {
            // Reset exception counter on successful WFI handling
            unsafe { EXCEPTION_COUNT = 0; }

            // WFI/WFE: Guest is waiting for interrupt
            // Check if virtual timer is pending and inject it to guest
            if handle_wfi_with_timer_injection(context.pc) {
                // Advance PC past the WFI instruction
                context.pc += 4;
                true // Continue with injected interrupt
            } else {
                // No timer pending, exit to let host decide
                false
            }
        }
        
        ExitReason::HvcCall => {
            // Reset exception counter on hypercall
            unsafe { EXCEPTION_COUNT = 0; }
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
            // Data abort - might be MMIO access
            let far = context.sys_regs.far_el2;

            // Debug: show GIC accesses
            if far >= 0x0800_0000 && far < 0x0A00_0000 {
                // This is a GIC access
                static mut GIC_ACCESS_COUNT: u32 = 0;
                unsafe {
                    GIC_ACCESS_COUNT += 1;
                    if GIC_ACCESS_COUNT <= 5 {
                        uart_puts(b"[GIC MMIO] Access at 0x");
                        print_hex(far);
                        uart_puts(b" PC=0x");
                        print_hex(context.pc);
                        uart_puts(b"\n");
                    }
                }
            }

            // Try to handle as MMIO
            if handle_mmio_abort(context, far) {
                // Reset exception counter on successful MMIO
                unsafe { EXCEPTION_COUNT = 0; }
                // Successfully handled, advance PC and continue
                context.pc += 4;
                true
            } else {
                // Not MMIO or failed to handle
                uart_puts(b"[VCPU] Data abort at 0x");
                print_hex(far);
                uart_puts(b" (not MMIO)\n");
                false // Exit
            }
        }
        
        ExitReason::Unknown | ExitReason::Other(_) => {
            // Don't try to handle as IRQ - unknown exceptions should exit
            // This prevents infinite loops from unhandled exceptions
            uart_puts(b"[VCPU] Unknown exception, ESR=0x");
            print_hex(esr);
            uart_puts(b" PC=0x");
            print_hex(context.pc);
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

/// Handle MMIO data abort
/// 
/// # Returns
/// * `true` if successfully handled
/// * `false` if not MMIO or handling failed
fn handle_mmio_abort(context: &mut VcpuContext, addr: u64) -> bool {
    use crate::arch::aarch64::hypervisor::decode::MmioAccess;
    use crate::uart_puts;
    
    // Get the faulting instruction
    let insn = unsafe {
        core::ptr::read_volatile(context.pc as *const u32)
    };
    
    // Get ISS from ESR_EL2
    let iss = (context.sys_regs.esr_el2 & 0x1FFFFFF) as u32;
    
    // Decode the instruction
    let access = match MmioAccess::decode(insn, iss) {
        Some(a) => a,
        None => {
            uart_puts(b"[MMIO] Failed to decode instruction at 0x");
            print_hex(context.pc);
            uart_puts(b"\n");
            return false;
        }
    };
    
    // Log the access (optional, can be disabled for production)
    // uart_puts(b"[MMIO] Access at 0x");
    // print_hex(addr);
    // uart_puts(if access.is_load() { b" (load)\n" } else { b" (store)\n" });
    
    // Handle the MMIO access
    if access.is_store() {
        // Store: get value from source register
        let value = context.gp_regs.get_reg(access.reg());
        crate::global::DEVICES.handle_mmio(addr, value, access.size(), true);
        true
    } else {
        // Load: get value from device and write to destination register
        match crate::global::DEVICES.handle_mmio(addr, 0, access.size(), false) {
            Some(value) => {
                context.gp_regs.set_reg(access.reg(), value);
                true
            }
            None => {
                uart_puts(b"[MMIO] Read failed at 0x");
                print_hex(addr);
                uart_puts(b"\n");
                false
            }
        }
    }
}

/// WFI counter - track consecutive WFIs to detect infinite loops
static mut WFI_CONSECUTIVE_COUNT: u32 = 0;
static mut LAST_WFI_PC: u64 = 0;
const MAX_CONSECUTIVE_WFI: u32 = 5000;

/// Handle WFI by checking and injecting virtual timer interrupt
///
/// When guest executes WFI, it's waiting for an interrupt.
/// We check if the virtual timer has fired and inject it via GICv3 List Registers.
///
/// Strategy:
/// 1. If timer is pending, inject it and continue
/// 2. If no timer pending, inject one anyway (the guest needs a tick)
/// 3. If guest keeps calling WFI excessively, eventually exit
///
/// # Returns
/// * `true` - Guest should continue (interrupt injected)
/// * `false` - Guest should exit (stuck in WFI loop)
fn handle_wfi_with_timer_injection(pc: u64) -> bool {
    use crate::arch::aarch64::peripherals::timer;
    use crate::arch::aarch64::peripherals::gicv3::{GicV3VirtualInterface, VTIMER_IRQ};
    use crate::uart_puts;

    let count = unsafe { WFI_CONSECUTIVE_COUNT };
    let last_pc = unsafe { LAST_WFI_PC };

    // Check if PC changed - that means guest is making progress
    if pc != last_pc {
        // Reset counter on progress
        unsafe {
            WFI_CONSECUTIVE_COUNT = 0;
            LAST_WFI_PC = pc;
        }
        uart_puts(b"[WFI] New location PC=0x");
        print_hex(pc);
        uart_puts(b"\n");

        // Print VBAR_EL1 on first WFI to check if guest exception vectors are set up
        let vbar: u64;
        unsafe {
            core::arch::asm!("mrs {}, vbar_el1", out(reg) vbar);
        }
        uart_puts(b"[WFI] Guest VBAR_EL1 = 0x");
        print_hex(vbar);
        uart_puts(b"\n");

        // Inject an interrupt on first WFI too - the guest needs it
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, 0xA0);
        return true;
    }

    // Debug output for first few WFIs at same location
    if count < 3 {
        uart_puts(b"[WFI] #");
        print_hex(count as u64);
        uart_puts(b" at PC=0x");
        print_hex(pc);
        uart_puts(b"\n");
    }

    // Increment WFI counter
    unsafe {
        WFI_CONSECUTIVE_COUNT += 1;
        if WFI_CONSECUTIVE_COUNT > MAX_CONSECUTIVE_WFI {
            // Guest is stuck in WFI loop without making progress
            uart_puts(b"[WFI] Stuck at same PC (count=");
            print_hex(WFI_CONSECUTIVE_COUNT as u64);
            uart_puts(b", max=");
            print_hex(MAX_CONSECUTIVE_WFI as u64);
            uart_puts(b"), exiting\n");
            return false;
        }
    }

    // Check if virtual timer is pending
    if timer::is_guest_vtimer_pending() {
        // Reset WFI counter on successful timer handling
        unsafe { WFI_CONSECUTIVE_COUNT = 0; }

        if count < 10 {
            uart_puts(b"[WFI] Timer pending, injecting IRQ 27\n");
        }

        // Mask the timer to prevent continuous firing
        timer::mask_guest_vtimer();

        // Inject virtual timer interrupt (IRQ 27) to guest via GICv3 List Register
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, 0xA0);
        return true;
    }

    // No timer pending - check if any interrupt is pending in List Registers
    if GicV3VirtualInterface::pending_count() > 0 {
        // Reset counter - there's work to do
        unsafe { WFI_CONSECUTIVE_COUNT = 0; }
        return true;
    }

    // No interrupts pending at all
    // Inject a timer interrupt anyway - the guest needs a tick to make progress
    // This simulates a periodic timer tick that RTOSes need for scheduling
    if count % 100 == 0 && count < 1000 {
        if count < 10 {
            uart_puts(b"[WFI] No timer, injecting periodic tick\n");
        }
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, 0xA0);

        // Debug: print GIC virtual interface state after injection
        if count == 0 {
            debug_print_gic_state();
        }
    }

    // Allow guest to continue
    true
}

/// Debug: Print GIC virtual interface state
fn debug_print_gic_state() {
    use crate::uart_puts;
    use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;

    uart_puts(b"[DEBUG] GIC state:\n");

    // Read ICH_HCR_EL2
    let hcr = GicV3VirtualInterface::read_hcr();
    uart_puts(b"  ICH_HCR_EL2 = 0x");
    print_hex(hcr as u64);
    uart_puts(b" (En=");
    if hcr & 1 != 0 { uart_puts(b"1)\n"); } else { uart_puts(b"0)\n"); }

    // Read ICH_VMCR_EL2
    let vmcr = GicV3VirtualInterface::read_vmcr();
    uart_puts(b"  ICH_VMCR_EL2 = 0x");
    print_hex(vmcr as u64);
    uart_puts(b"\n");
    uart_puts(b"    VPMR (bits 31:24) = 0x");
    print_hex(((vmcr >> 24) & 0xFF) as u64);
    uart_puts(b"\n");
    uart_puts(b"    VENG1 (bit 1) = ");
    if vmcr & 2 != 0 { uart_puts(b"1\n"); } else { uart_puts(b"0\n"); }

    // Read List Register 0
    let lr0 = GicV3VirtualInterface::read_lr(0);
    uart_puts(b"  ICH_LR0 = 0x");
    print_hex(lr0);
    uart_puts(b"\n");
    let state = (lr0 >> 62) & 0x3;
    uart_puts(b"    State = ");
    match state {
        0 => uart_puts(b"Invalid\n"),
        1 => uart_puts(b"Pending\n"),
        2 => uart_puts(b"Active\n"),
        3 => uart_puts(b"Pending+Active\n"),
        _ => uart_puts(b"Unknown\n"),
    }
    let intid = (lr0 & 0xFFFF_FFFF) as u32;
    uart_puts(b"    INTID = ");
    print_hex(intid as u64);
    uart_puts(b"\n");

    // Read SPSR_EL2 to check if guest IRQs are masked
    let spsr: u64;
    unsafe {
        core::arch::asm!("mrs {}, spsr_el2", out(reg) spsr);
    }
    uart_puts(b"  SPSR_EL2 = 0x");
    print_hex(spsr);
    uart_puts(b" (I=");
    if spsr & (1 << 7) != 0 { uart_puts(b"1-masked)\n"); } else { uart_puts(b"0-unmasked)\n"); }
}

/// Handle IRQ interrupts
/// 
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Exit to host
fn handle_irq(_context: &mut VcpuContext) -> bool {
    use crate::arch::aarch64::peripherals::gic::{GICC, VTIMER_IRQ};
    use crate::arch::aarch64::peripherals::timer;
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
