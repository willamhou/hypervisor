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
        //
        // Bit assignments (from ARM Architecture Reference Manual):
        //   [0]  VM    - Virtualization enable (set later in init_stage2)
        //   [1]  SWIO  - Set/Way Invalidation Override
        //   [3]  FMO   - Route physical FIQ to EL2
        //   [4]  IMO   - Route physical IRQ to EL2
        //   [5]  AMO   - Route physical SError/abort to EL2
        //   [9]  FB    - Force Broadcast TLB/cache maintenance
        //   [10] BSU   - Barrier Shareability Upgrade (01 = Inner Shareable)
        //   [13] TWI   - Trap WFI to EL2
        //   [14] TWE   - Trap WFE to EL2
        //   [31] RW    - EL1 is AArch64
        //   [40] APK   - Don't trap PAC key register accesses from EL1
        //   [41] API   - Don't trap PAC instructions from EL1
        //
        // NOTE: Do NOT set bit 12 (DC = Default Cacheability).
        // DC=1 changes cache attributes when guest MMU is off, which can
        // cause stale page table data during the MMU-on transition.
        let hcr: u64 = (1u64 << 31) | // RW: EL1 is AArch64
                       (1u64 << 1)  | // SWIO: Set/Way Invalidation Override
                       (1u64 << 3)  | // FMO: Route physical FIQ to EL2
                       (1u64 << 4)  | // IMO: Route physical IRQ to EL2
                       (1u64 << 5)  | // AMO: Route physical SError to EL2
                       (1u64 << 9)  | // FB: Force Broadcast TLB/cache maintenance
                       (1u64 << 10) | // BSU[0]: Barrier Shareability Upgrade = IS
                       (1u64 << 13) | // TWI: Trap WFI to EL2
                       (1u64 << 14) | // TWE: Trap WFE to EL2
                       (1u64 << 40) | // APK: Don't trap PAC key register accesses
                       (1u64 << 41);  // API: Don't trap PAC instructions
        
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
static mut TOTAL_EXCEPTION_COUNT: u64 = 0;
const MAX_CONSECUTIVE_EXCEPTIONS: u32 = 100;

/// Reset all exception counters (call before entering a new guest)
pub fn reset_exception_counters() {
    unsafe {
        EXCEPTION_COUNT = 0;
        TOTAL_EXCEPTION_COUNT = 0;
        WFI_CONSECUTIVE_COUNT = 0;
        LAST_WFI_PC = 0;
    }
}

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

    // Count exceptions for debugging
    unsafe {
        TOTAL_EXCEPTION_COUNT += 1;
    }

    // For now, just handle basic cases
    use crate::arch::aarch64::regs::ExitReason;
    use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;
    use crate::uart_puts;

    match exit_reason {
        ExitReason::WfiWfe => {
            // Reset exception counter on successful WFI handling
            unsafe { EXCEPTION_COUNT = 0; }

            // WFI/WFE: Guest is waiting for interrupt
            // Check if virtual timer is pending and inject it to guest
            if handle_wfi_with_timer_injection(context) {
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
            // Reset exception counter on MSR/MRS handling
            unsafe { EXCEPTION_COUNT = 0; }
            handle_msr_mrs_trap(context, esr);
            context.pc += 4;
            true // Continue
        }
        
        ExitReason::InstructionAbort => {
            uart_puts(b"[VCPU] Instruction abort at FAR=0x");
            print_hex(context.sys_regs.far_el2);
            uart_puts(b" PC=0x");
            print_hex(context.pc);
            uart_puts(b"\n");

            // Read EL1 registers to understand what caused the ORIGINAL EL1 exception
            // (Before the guest tried to jump to VBAR_EL1+offset which caused this Stage-2 fault)
            let elr_el1: u64;
            let esr_el1: u64;
            let spsr_el1: u64;
            let sctlr_el1: u64;
            let sp_el1: u64;
            let far_el1: u64;
            let tcr_el1: u64;
            let ttbr0_el1: u64;
            let ttbr1_el1: u64;
            let vbar_el1: u64;
            unsafe {
                core::arch::asm!("mrs {}, elr_el1", out(reg) elr_el1);
                core::arch::asm!("mrs {}, esr_el1", out(reg) esr_el1);
                core::arch::asm!("mrs {}, spsr_el1", out(reg) spsr_el1);
                core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr_el1);
                core::arch::asm!("mrs {}, sp_el1", out(reg) sp_el1);
                core::arch::asm!("mrs {}, far_el1", out(reg) far_el1);
                core::arch::asm!("mrs {}, tcr_el1", out(reg) tcr_el1);
                core::arch::asm!("mrs {}, ttbr0_el1", out(reg) ttbr0_el1);
                core::arch::asm!("mrs {}, ttbr1_el1", out(reg) ttbr1_el1);
                core::arch::asm!("mrs {}, vbar_el1", out(reg) vbar_el1);
            }
            uart_puts(b"[VCPU] EL1 state at crash:\n");
            uart_puts(b"  ELR_EL1  = 0x");
            print_hex(elr_el1);
            uart_puts(b" (instruction that caused EL1 exception)\n");
            uart_puts(b"  ESR_EL1  = 0x");
            print_hex(esr_el1);
            let el1_ec = (esr_el1 >> 26) & 0x3F;
            uart_puts(b" (EC=0x");
            print_hex(el1_ec);
            uart_puts(b")\n");
            uart_puts(b"  SPSR_EL1 = 0x");
            print_hex(spsr_el1);
            uart_puts(b"\n");
            uart_puts(b"  SCTLR_EL1= 0x");
            print_hex(sctlr_el1);
            uart_puts(b" (M=");
            if sctlr_el1 & 1 != 0 { uart_puts(b"1"); } else { uart_puts(b"0"); }
            uart_puts(b")\n");
            uart_puts(b"  SP_EL1   = 0x");
            print_hex(sp_el1);
            uart_puts(b"\n");
            uart_puts(b"  FAR_EL1  = 0x");
            print_hex(far_el1);
            uart_puts(b" (faulting address)\n");
            uart_puts(b"  TCR_EL1  = 0x");
            print_hex(tcr_el1);
            uart_puts(b"\n");
            uart_puts(b"  TTBR0_EL1= 0x");
            print_hex(ttbr0_el1);
            uart_puts(b"\n");
            uart_puts(b"  TTBR1_EL1= 0x");
            print_hex(ttbr1_el1);
            uart_puts(b"\n");
            uart_puts(b"  VBAR_EL1 = 0x");
            print_hex(vbar_el1);
            uart_puts(b"\n");

            // Also read ESR_EL2 ISS for more details on the Stage-2 fault
            let iss = esr & 0x1FFFFFF;
            uart_puts(b"  ESR_EL2 ISS = 0x");
            print_hex(iss as u64);
            uart_puts(b"\n");

            // Dump VTCR_EL2, VTTBR_EL2, and HCR_EL2
            let vtcr_el2: u64;
            let vttbr_el2: u64;
            let hcr_el2: u64;
            let id_mmfr0: u64;
            unsafe {
                core::arch::asm!("mrs {}, vtcr_el2", out(reg) vtcr_el2);
                core::arch::asm!("mrs {}, vttbr_el2", out(reg) vttbr_el2);
                core::arch::asm!("mrs {}, hcr_el2", out(reg) hcr_el2);
                core::arch::asm!("mrs {}, id_aa64mmfr0_el1", out(reg) id_mmfr0);
            }
            uart_puts(b"  VTCR_EL2 = 0x");
            print_hex(vtcr_el2);
            let vtcr_t0sz = vtcr_el2 & 0x3F;
            let vtcr_sl0 = (vtcr_el2 >> 6) & 0x3;
            let vtcr_ps = (vtcr_el2 >> 16) & 0x7;
            uart_puts(b" (T0SZ=");
            print_hex(vtcr_t0sz);
            uart_puts(b" SL0=");
            print_hex(vtcr_sl0);
            uart_puts(b" PS=");
            print_hex(vtcr_ps);
            uart_puts(b")\n");
            uart_puts(b"  VTTBR_EL2= 0x");
            print_hex(vttbr_el2);
            uart_puts(b"\n");
            uart_puts(b"  HCR_EL2  = 0x");
            print_hex(hcr_el2);
            uart_puts(b" (VM=");
            if hcr_el2 & 1 != 0 { uart_puts(b"1)\n"); } else { uart_puts(b"0)\n"); }
            uart_puts(b"  ID_AA64MMFR0_EL1 = 0x");
            print_hex(id_mmfr0);
            uart_puts(b"\n");

            // Dump Stage-2 L0 table entries to verify
            let s2_l0_base = vttbr_el2 & !0xFFF;
            uart_puts(b"  S2 L0[0] = 0x");
            let s2_l0_0 = unsafe { core::ptr::read_volatile(s2_l0_base as *const u64) };
            print_hex(s2_l0_0);
            uart_puts(b"\n");
            if s2_l0_0 & 0x3 == 0x3 {
                let s2_l1_base = s2_l0_0 & 0x0000_FFFF_FFFF_F000;
                uart_puts(b"  S2 L1[0] = 0x");
                let s2_l1_0 = unsafe { core::ptr::read_volatile(s2_l1_base as *const u64) };
                print_hex(s2_l1_0);
                uart_puts(b"\n");
                uart_puts(b"  S2 L1[1] = 0x");
                let s2_l1_1 = unsafe { core::ptr::read_volatile((s2_l1_base + 8) as *const u64) };
                print_hex(s2_l1_1);
                uart_puts(b"\n");
            }

            // Dump the kernel L0 page table entry that caused the fault
            if far_el1 >= 0xFFFF_0000_0000_0000 {
                // TTBR1 translation - walk the page table
                let l0_base = ttbr1_el1 & !0xFFF;
                let l0_index = ((far_el1 >> 39) & 0x1FF) as usize;
                let l0_entry_addr = l0_base + (l0_index as u64) * 8;
                let l0_entry = unsafe { core::ptr::read_volatile(l0_entry_addr as *const u64) };
                uart_puts(b"  TTBR1 L0 base = 0x");
                print_hex(l0_base);
                uart_puts(b"\n");
                uart_puts(b"  L0[");
                print_hex(l0_index as u64);
                uart_puts(b"] @ 0x");
                print_hex(l0_entry_addr);
                uart_puts(b" = 0x");
                print_hex(l0_entry);
                let l0_valid = l0_entry & 1;
                let l0_type = (l0_entry >> 1) & 1;
                uart_puts(b" (valid=");
                print_hex(l0_valid);
                uart_puts(b" type=");
                print_hex(l0_type);
                uart_puts(b")\n");
                // If L0 entry is valid table, dump the address it points to
                if l0_entry & 0x3 == 0x3 {
                    let l1_base = l0_entry & 0x0000_FFFF_FFFF_F000;
                    let l1_index = ((far_el1 >> 30) & 0x1FF) as usize;
                    let l1_entry_addr = l1_base + (l1_index as u64) * 8;
                    let l1_entry = unsafe { core::ptr::read_volatile(l1_entry_addr as *const u64) };
                    uart_puts(b"  L1[");
                    print_hex(l1_index as u64);
                    uart_puts(b"] @ 0x");
                    print_hex(l1_entry_addr);
                    uart_puts(b" = 0x");
                    print_hex(l1_entry);
                    uart_puts(b"\n");
                }
            }

            // This is a fatal error
            false // Exit
        }
        
        ExitReason::DataAbort => {
            // Data abort - might be MMIO access
            let far = context.sys_regs.far_el2;

            // Debug: show UART accesses from Zephyr guest (0x4800xxxx)
            if far >= 0x0900_0000 && far < 0x0901_0000 && context.pc >= 0x4800_0000 {
                // This is a UART access from Zephyr guest
                static mut UART_ACCESS_COUNT: u32 = 0;
                unsafe {
                    UART_ACCESS_COUNT += 1;
                    if UART_ACCESS_COUNT <= 20 {
                        uart_puts(b"[UART MMIO] Guest PC=0x");
                        print_hex(context.pc);
                        uart_puts(b" addr=0x");
                        print_hex(far);
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
        
        ExitReason::Other(ec) => {
            // Handle specific ECs that aren't fatal
            match ec {
                0x07 => {
                    // Trapped SIMD/FP access - skip instruction
                    // (Should not happen after CPTR_EL2 fix)
                    uart_puts(b"[VCPU] FP/SIMD trap at PC=0x");
                    print_hex(context.pc);
                    uart_puts(b"\n");
                    context.pc += 4;
                    true
                }
                0x09 => {
                    // SVE/SME access trap (CPTR_EL2.TZ or TSM)
                    // After CPTR_EL2 fix this shouldn't occur, but handle gracefully
                    uart_puts(b"[VCPU] SVE/SME trap at PC=0x");
                    print_hex(context.pc);
                    uart_puts(b"\n");
                    context.pc += 4;
                    true
                }
                0x19 => {
                    // SVE trapped by CPTR_EL2.TZ when ZEN != 0b11
                    uart_puts(b"[VCPU] SVE trap (EC=0x19) at PC=0x");
                    print_hex(context.pc);
                    uart_puts(b"\n");
                    context.pc += 4;
                    true
                }
                _ => {
                    // Unknown/unhandled exception - fatal
                    uart_puts(b"[VCPU] Unknown exception EC=0x");
                    print_hex(ec);
                    uart_puts(b" ESR=0x");
                    print_hex(esr);
                    uart_puts(b" PC=0x");
                    print_hex(context.pc);
                    uart_puts(b"\n");
                    false // Exit
                }
            }
        }

        ExitReason::Unknown => {
            uart_puts(b"[VCPU] Unknown exception, ESR=0x");
            print_hex(esr);
            uart_puts(b" PC=0x");
            print_hex(context.pc);
            uart_puts(b"\n");
            false // Exit
        }
    }
}

/// IRQ exception handler called from assembly (irq_exception_handler)
///
/// This handles physical IRQs that trap from the guest to EL2
/// (e.g., virtual timer interrupt). Unlike sync exceptions, ESR_EL2
/// is NOT valid - we acknowledge via ICC_IAR1_EL1.
///
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Exit to host
#[no_mangle]
pub extern "C" fn handle_irq_exception(context: &mut VcpuContext) -> bool {
    use crate::arch::aarch64::peripherals::gicv3::{GicV3SystemRegs, GicV3VirtualInterface, VTIMER_IRQ};
    use crate::arch::aarch64::peripherals::timer;
    use crate::uart_puts;

    // Reset sync exception counter (guest is making progress)
    unsafe { EXCEPTION_COUNT = 0; }

    // Acknowledge the physical interrupt
    let intid = GicV3SystemRegs::read_iar1();

    // Check for spurious interrupt (INTID >= 1020)
    if intid >= 1020 {
        return true;
    }

    match intid {
        27 => {
            // Virtual timer interrupt (PPI 27)
            // Mask the timer to stop continuous firing
            timer::mask_guest_vtimer();

            // Inject virtual interrupt to guest with HW=1.
            // HW=1 links virtual and physical interrupt: guest's virtual EOI
            // automatically deactivates the physical interrupt (pINTID=27).
            let _inject_result = GicV3VirtualInterface::inject_hw_interrupt(VTIMER_IRQ, VTIMER_IRQ, 0xA0);

            // DO NOT modify SPSR_EL2 (guest's saved PSTATE).
            // The virtual IRQ is pending in the LR. When we ERET back, the guest
            // resumes with its original PSTATE. If the guest had interrupts disabled
            // (PSTATE.I=1, e.g. holding a spinlock), the virtual IRQ stays pending
            // until the guest re-enables interrupts - preventing deadlock.
        }
        _ => {
            uart_puts(b"[IRQ] Unhandled INTID=");
            print_hex(intid as u64);
            uart_puts(b"\n");
        }
    }

    // EOImode=1: EOIR only does priority drop (not deactivation).
    // This is required for HW=1 interrupts (timer) where guest's virtual
    // EOI handles physical deactivation.
    GicV3SystemRegs::write_eoir1(intid);

    // For non-HW interrupts, explicitly deactivate the physical interrupt
    // since there's no HW link to do it automatically.
    // INTID 27 (timer) uses HW=1, so guest virtual EOI handles deactivation.
    if intid != 27 {
        GicV3SystemRegs::write_dir(intid);
    }

    true // Always continue guest after IRQ
}

/// Handle MSR/MRS trap (EC=0x18)
///
/// Decodes the ISS to identify the trapped system register and emulates
/// the access. For MRS (reads): writes the register value to the destination
/// register. For MSR (writes): reads the value from the source register
/// and writes to the system register.
///
/// ISS encoding (from KVM/ARM):
///   [21:20] Op0, [19:17] Op2, [16:14] Op1, [13:10] CRn, [9:5] Rt, [4:1] CRm, [0] Direction
fn handle_msr_mrs_trap(context: &mut VcpuContext, esr: u64) {
    use crate::uart_puts;

    let iss = (esr & 0x1FFFFFF) as u32;
    let op0 = (iss >> 20) & 0x3;
    let op2 = (iss >> 17) & 0x7;
    let op1 = (iss >> 14) & 0x7;
    let crn = (iss >> 10) & 0xF;
    let rt  = ((iss >> 5) & 0x1F) as u8;
    let crm = (iss >> 1) & 0xF;
    let is_read = (iss & 1) == 1;

    // Log first 20 MSR/MRS traps with register encoding
    static mut MSR_TRAP_COUNT: u32 = 0;
    unsafe {
        MSR_TRAP_COUNT += 1;
        if MSR_TRAP_COUNT <= 3 {
            if is_read {
                uart_puts(b"[MSR] MRS x");
            } else {
                uart_puts(b"[MSR] MSR x");
            }
            // Print register number
            if rt >= 10 {
                uart_puts(&[b'0' + (rt / 10), b'0' + (rt % 10)]);
            } else {
                uart_puts(&[b'0' + rt]);
            }
            uart_puts(b", S");
            uart_puts(&[b'0' + op0 as u8]);
            uart_puts(b"_");
            uart_puts(&[b'0' + op1 as u8]);
            uart_puts(b"_C");
            if crn >= 10 {
                uart_puts(&[b'0' + (crn / 10) as u8, b'0' + (crn % 10) as u8]);
            } else {
                uart_puts(&[b'0' + crn as u8]);
            }
            uart_puts(b"_C");
            if crm >= 10 {
                uart_puts(&[b'0' + (crm / 10) as u8, b'0' + (crm % 10) as u8]);
            } else {
                uart_puts(&[b'0' + crm as u8]);
            }
            uart_puts(b"_");
            uart_puts(&[b'0' + op2 as u8]);
            uart_puts(b" PC=0x");
            print_hex(context.pc);
            uart_puts(b"\n");
        }
    }

    if is_read {
        // MRS: Read system register, write value to Rt
        let value = emulate_mrs(op0, op1, crn, crm, op2);
        if rt < 31 {
            context.gp_regs.set_reg(rt, value);
        }
        // rt=31 means xzr, discard result
    } else {
        // MSR: Read value from Rt, write to system register
        let value = if rt < 31 {
            context.gp_regs.get_reg(rt)
        } else {
            0 // xzr
        };
        emulate_msr(op0, op1, crn, crm, op2, value);
    }
}

/// Emulate MRS (system register read) for trapped registers
///
/// Returns the value that should be placed in the destination register.
fn emulate_mrs(op0: u32, op1: u32, crn: u32, crm: u32, op2: u32) -> u64 {
    match (op0, op1, crn, crm, op2) {
        // Debug registers (Op0=2) - return safe defaults
        (2, 0, 0, 2, 2) => {
            // MDSCR_EL1 - Debug Status and Control
            unsafe {
                let val: u64;
                core::arch::asm!("mrs {}, mdscr_el1", out(reg) val);
                val
            }
        }
        (2, 0, 1, 1, 4) => {
            // OSLSR_EL1 - OS Lock Status (report unlocked)
            1 << 3 // OSLM=1 (OS Lock implemented), OSLK=0 (unlocked)
        }
        (2, 0, 1, 3, 4) => {
            // OSDLR_EL1 - OS Double Lock Register (report unlocked)
            0
        }
        // PMU registers (Op0=3, Op1=3, CRn=9) - return 0 (no PMU)
        (3, 3, 9, _, _) => 0,
        // PMU registers (Op0=3, Op1=0, CRn=9) - return 0
        (3, 0, 9, _, _) => 0,
        // Any other trapped register: Read-As-Zero
        _ => 0,
    }
}

/// Emulate MSR (system register write) for trapped registers
///
/// Writes the value to the system register if we know how, otherwise ignores.
fn emulate_msr(op0: u32, op1: u32, crn: u32, crm: u32, op2: u32, value: u64) {
    match (op0, op1, crn, crm, op2) {
        // Debug registers
        (2, 0, 0, 2, 2) => {
            // MDSCR_EL1 - Debug Status and Control
            unsafe {
                core::arch::asm!("msr mdscr_el1, {}", in(reg) value);
            }
        }
        (2, 0, 1, 0, 4) => {
            // OSLAR_EL1 - OS Lock Access (write-only)
            unsafe {
                core::arch::asm!("msr oslar_el1, {}", in(reg) value);
            }
        }
        (2, 0, 1, 3, 4) => {
            // OSDLR_EL1 - OS Double Lock
            // Ignore (don't actually lock)
        }
        // PMU registers - ignore writes
        (3, 3, 9, _, _) | (3, 0, 9, _, _) => {}
        // Any other trapped register: Write-Ignored
        _ => {}
    }
}

// PSCI function IDs (ARM Standard)
const PSCI_VERSION: u64 = 0x84000000;
const PSCI_CPU_SUSPEND_32: u64 = 0x84000001;
const PSCI_CPU_OFF: u64 = 0x84000002;
const PSCI_CPU_ON_32: u64 = 0x84000003;
const PSCI_CPU_ON_64: u64 = 0xC4000003;
const PSCI_AFFINITY_INFO_32: u64 = 0x84000004;
const PSCI_AFFINITY_INFO_64: u64 = 0xC4000004;
const PSCI_MIGRATE_INFO_TYPE: u64 = 0x84000006;
const PSCI_SYSTEM_OFF: u64 = 0x84000008;
const PSCI_SYSTEM_RESET: u64 = 0x84000009;
const PSCI_FEATURES: u64 = 0x8400000A;

// PSCI return values
const PSCI_SUCCESS: u64 = 0;
const PSCI_NOT_SUPPORTED: u64 = 0xFFFFFFFF; // -1 as unsigned
const PSCI_INVALID_PARAMS: u64 = 0xFFFFFFFE; // -2 as unsigned
const PSCI_DENIED: u64 = 0xFFFFFFFD; // -3 as unsigned

// PSCI version: v0.2
const PSCI_VERSION_0_2: u64 = 0x00000002;

// Jailhouse debug console constants
// HVC #0x4a48 is "JH" in ASCII - Jailhouse hypercall signature
const JAILHOUSE_HVC_IMMEDIATE: u32 = 0x4a48;
const JAILHOUSE_HC_DEBUG_CONSOLE_PUTC: u64 = 8;
const JAILHOUSE_HC_DEBUG_CONSOLE_GETC: u64 = 9;

/// Handle hypercalls from guest
///
/// Supports:
/// - Custom hypercalls (x0 = 0, 1, ...)
/// - PSCI standard calls (x0 has bit 31 set)
/// - Jailhouse debug console (HVC #0x4a48)
///
/// # Arguments
/// * `context` - vCPU context
/// * `hvc_imm` - HVC immediate value from ESR_EL2[15:0]
///
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Exit to host
fn handle_hypercall_with_imm(context: &mut VcpuContext, hvc_imm: u32) -> bool {
    use crate::uart_puts;

    // Check for Jailhouse debug console hypercall
    if hvc_imm == JAILHOUSE_HVC_IMMEDIATE {
        return handle_jailhouse_debug_console(context);
    }

    // Standard hypercall handling (HVC #0)
    let hypercall_num = context.gp_regs.x0;

    // Check if this is a PSCI call (bit 31 set indicates SMC/HVC standard call)
    if hypercall_num & 0x80000000 != 0 {
        return handle_psci(context, hypercall_num);
    }

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

/// Handle Jailhouse debug console hypercall
///
/// This implements the Jailhouse hypervisor's debug console interface:
/// - HVC #0x4a48 with x0=8: Output character in x1 to console
///
/// This allows Zephyr with CONFIG_JAILHOUSE_DEBUG_CONSOLE=y to print to
/// the hypervisor's UART without needing its own UART driver.
fn handle_jailhouse_debug_console(context: &mut VcpuContext) -> bool {
    let function = context.gp_regs.x0;

    match function {
        JAILHOUSE_HC_DEBUG_CONSOLE_PUTC => {
            // Output character
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
        JAILHOUSE_HC_DEBUG_CONSOLE_GETC => {
            // Input character - read from real UART
            // Returns character in x0, or -1 if no character available
            let uart_base = 0x09000000usize;
            let uart_fr = uart_base + 0x18; // Flag register

            unsafe {
                // Check if RX FIFO has data (FR bit 4 = RXFE, 0 = has data)
                let fr: u32;
                core::arch::asm!(
                    "ldr {val:w}, [{addr}]",
                    addr = in(reg) uart_fr,
                    val = out(reg) fr,
                    options(nostack, readonly),
                );

                if fr & (1 << 4) == 0 {
                    // Data available, read it
                    let ch: u32;
                    core::arch::asm!(
                        "ldr {val:w}, [{addr}]",
                        addr = in(reg) uart_base,
                        val = out(reg) ch,
                        options(nostack, readonly),
                    );
                    context.gp_regs.x0 = (ch & 0xFF) as u64;
                } else {
                    // No data available
                    context.gp_regs.x0 = !0u64; // -1
                }
            }
            true // Continue
        }
        _ => {
            // Unknown Jailhouse function - just return success silently
            context.gp_regs.x0 = 0;
            true
        }
    }
}

/// Legacy wrapper for backward compatibility
fn handle_hypercall(context: &mut VcpuContext) -> bool {
    // Extract HVC immediate from ESR_EL2[15:0]
    let hvc_imm = (context.sys_regs.esr_el2 & 0xFFFF) as u32;
    handle_hypercall_with_imm(context, hvc_imm)
}

/// Handle PSCI (Power State Coordination Interface) calls
///
/// Implements PSCI v0.2 for guest power management.
///
/// # Arguments
/// * `context` - vCPU context
/// * `function_id` - PSCI function ID from x0
///
/// # Returns
/// * `true` - Continue running guest
/// * `false` - Guest should exit
fn handle_psci(context: &mut VcpuContext, function_id: u64) -> bool {
    use crate::uart_puts;

    // Debug: log PSCI calls (first few only)
    static mut PSCI_CALL_COUNT: u32 = 0;
    unsafe {
        PSCI_CALL_COUNT += 1;
        if PSCI_CALL_COUNT <= 10 {
            uart_puts(b"[PSCI] Call: 0x");
            print_hex(function_id);
            uart_puts(b"\n");
        }
    }

    match function_id {
        PSCI_VERSION => {
            // Return PSCI v0.2
            context.gp_regs.x0 = PSCI_VERSION_0_2;
            uart_puts(b"[PSCI] VERSION -> 0.2\n");
            true
        }

        PSCI_FEATURES => {
            // Query if a PSCI function is supported
            let feature_id = context.gp_regs.x1;
            let result = match feature_id {
                PSCI_VERSION | PSCI_CPU_OFF | PSCI_SYSTEM_OFF |
                PSCI_SYSTEM_RESET | PSCI_FEATURES => PSCI_SUCCESS,
                PSCI_CPU_ON_32 | PSCI_CPU_ON_64 => PSCI_SUCCESS,
                PSCI_AFFINITY_INFO_32 | PSCI_AFFINITY_INFO_64 => PSCI_SUCCESS,
                _ => PSCI_NOT_SUPPORTED,
            };
            context.gp_regs.x0 = result;
            true
        }

        PSCI_CPU_OFF => {
            // CPU off - for single vCPU, this is like system halt
            uart_puts(b"[PSCI] CPU_OFF\n");
            context.gp_regs.x0 = PSCI_SUCCESS;
            // For single-core guest, CPU_OFF means we're done
            false
        }

        PSCI_CPU_ON_32 | PSCI_CPU_ON_64 => {
            // CPU on - secondary CPU startup
            // x1 = target CPU MPIDR
            // x2 = entry point
            // x3 = context ID
            let _target_cpu = context.gp_regs.x1;
            let _entry_point = context.gp_regs.x2;
            let _context_id = context.gp_regs.x3;

            uart_puts(b"[PSCI] CPU_ON (not fully implemented)\n");
            // For now, return success but don't actually start another CPU
            // Real implementation would need multi-vCPU support
            context.gp_regs.x0 = PSCI_SUCCESS;
            true
        }

        PSCI_AFFINITY_INFO_32 | PSCI_AFFINITY_INFO_64 => {
            // Return affinity state
            // 0 = ON, 1 = OFF, 2 = ON_PENDING
            // For single vCPU, always return ON for CPU 0
            let _target_affinity = context.gp_regs.x1;
            context.gp_regs.x0 = 0; // ON
            true
        }

        PSCI_MIGRATE_INFO_TYPE => {
            // Return migration type (2 = not supported)
            context.gp_regs.x0 = 2;
            true
        }

        PSCI_SYSTEM_OFF => {
            // System shutdown
            uart_puts(b"[PSCI] SYSTEM_OFF\n");
            false // Exit guest
        }

        PSCI_SYSTEM_RESET => {
            // System reset
            uart_puts(b"[PSCI] SYSTEM_RESET\n");
            // For now, just exit - could implement reset later
            false
        }

        PSCI_CPU_SUSPEND_32 => {
            // CPU suspend - treat like WFI
            uart_puts(b"[PSCI] CPU_SUSPEND\n");
            context.gp_regs.x0 = PSCI_SUCCESS;
            true
        }

        _ => {
            // Unknown PSCI function
            uart_puts(b"[PSCI] Unknown function: 0x");
            print_hex(function_id);
            uart_puts(b"\n");
            context.gp_regs.x0 = PSCI_NOT_SUPPORTED;
            true // Continue but return error
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

    // Get ISS from ESR_EL2
    let iss = (context.sys_regs.esr_el2 & 0x1FFFFFF) as u32;
    let isv = (iss >> 24) & 1;

    // Try ISS-based decode first (works even when guest MMU is on)
    // Only read instruction from context.pc if ISV=0 AND pc is a plausible physical address
    // (when guest MMU is on, context.pc is a virtual address we can't read from EL2)
    let insn = if isv == 1 {
        0 // ISS decode doesn't need the instruction
    } else if context.pc < 0x8000_0000_0000 {
        // PC looks like a physical address, safe to read
        unsafe { core::ptr::read_volatile(context.pc as *const u32) }
    } else {
        // PC is a virtual address (guest MMU is on), can't read instruction
        uart_puts(b"[MMIO] Can't decode: guest VA PC=0x");
        print_hex(context.pc);
        uart_puts(b" ISV=0\n");
        return false;
    };

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
const MAX_CONSECUTIVE_WFI: u32 = 500_000;

/// Handle WFI by checking and injecting virtual timer interrupt
///
/// When guest executes WFI, it's waiting for an interrupt.
/// We check if the virtual timer has fired and inject it via GICv3 List Registers.
///
/// # Returns
/// * `true` - Guest should continue (interrupt injected)
/// * `false` - Guest should exit (stuck in WFI loop)
fn handle_wfi_with_timer_injection(context: &mut VcpuContext) -> bool {
    use crate::arch::aarch64::peripherals::timer;
    use crate::arch::aarch64::peripherals::gicv3::{GicV3VirtualInterface, VTIMER_IRQ};
    use crate::uart_puts;

    let pc = context.pc;
    let count = unsafe { WFI_CONSECUTIVE_COUNT };
    let last_pc = unsafe { LAST_WFI_PC };

    // Check if PC changed - that means guest is making progress
    if pc != last_pc {
        unsafe {
            WFI_CONSECUTIVE_COUNT = 0;
            LAST_WFI_PC = pc;
        }

        // Inject an interrupt on first WFI at new location
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, 0xA0);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
        return true;
    }

    // Increment WFI counter
    unsafe {
        WFI_CONSECUTIVE_COUNT += 1;
        if WFI_CONSECUTIVE_COUNT > MAX_CONSECUTIVE_WFI {
            uart_puts(b"[WFI] Guest idle (");
            print_hex(WFI_CONSECUTIVE_COUNT as u64);
            uart_puts(b" WFIs at same PC), exiting\n");
            return false;
        }
    }

    // Check if virtual timer is pending
    if timer::is_guest_vtimer_pending() {
        unsafe { WFI_CONSECUTIVE_COUNT = 0; }
        timer::mask_guest_vtimer();
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, 0xA0);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
        return true;
    }

    // Check if any virtual interrupt is pending in List Registers
    if GicV3VirtualInterface::pending_count() > 0 {
        unsafe { WFI_CONSECUTIVE_COUNT = 0; }
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
        return true;
    }

    // No interrupts pending - inject periodic tick to help guest make progress
    let current_count = unsafe { WFI_CONSECUTIVE_COUNT };
    if current_count % 100 == 0 {
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, 0xA0);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
    }

    true
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
