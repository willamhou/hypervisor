//! ARM64 Exception Handling
//!
//! This module provides the interface to the exception vector table
//! and exception handlers for EL2.

use crate::arch::aarch64::regs::VcpuContext;
use crate::arch::aarch64::defs::*;
use crate::uart_puts;
use crate::uart_put_hex;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

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
        // NOTE: Do NOT set bit 12 (DC = Default Cacheability).
        // DC=1 changes cache attributes when guest MMU is off, which can
        // cause stale page table data during the MMU-on transition.
        let hcr: u64 = HCR_RW         // EL1 is AArch64
                      | HCR_SWIO       // Set/Way Invalidation Override
                      | HCR_FMO        // Route physical FIQ to EL2
                      | HCR_IMO        // Route physical IRQ to EL2
                      | HCR_AMO        // Route physical SError to EL2
                      | HCR_FB         // Force Broadcast TLB/cache maintenance
                      | HCR_BSU_INNER  // Barrier Shareability Upgrade = IS
                      | HCR_TWI        // Trap WFI to EL2 (for vCPU scheduling)
                      // TWE NOT set: WFE executes natively (used in spinlocks,
                      // woken by SEV not SGI — trapping would cause deadlock)
                      | HCR_TEA        // Trap External Aborts to EL2
                      | HCR_APK        // Don't trap PAC key register accesses
                      | HCR_API;       // Don't trap PAC instructions

        core::arch::asm!(
            "msr hcr_el2, {hcr}",
            "isb",
            hcr = in(reg) hcr,
            options(nostack, nomem),
        );
    }
}

// Exception loop prevention: track consecutive exceptions
static EXCEPTION_COUNT: AtomicU32 = AtomicU32::new(0);
const MAX_CONSECUTIVE_EXCEPTIONS: u32 = 100;

/// Reset all exception counters (call before entering a new guest)
pub fn reset_exception_counters() {
    EXCEPTION_COUNT.store(0, Ordering::Relaxed);
    WFI_CONSECUTIVE_COUNT.store(0, Ordering::Relaxed);
    LAST_WFI_PC.store(0, Ordering::Relaxed);
}

/// Exception handler called from assembly
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

    // Check for exception loop
    let count = EXCEPTION_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if count > MAX_CONSECUTIVE_EXCEPTIONS {
        uart_puts(b"\n[FATAL] Too many consecutive exceptions, halting system\n");
        uart_puts(b"[DEBUG] ESR_EL2=0x");
        uart_put_hex(esr);
        uart_puts(b" FAR_EL2=0x");
        uart_put_hex(far);
        uart_puts(b" PC=0x");
        uart_put_hex(context.pc);
        uart_puts(b"\n");
        // Halt the system completely to prevent further execution
        loop {
            unsafe { core::arch::asm!("wfe"); }
        }
    }

    // Get exit reason
    let exit_reason = context.exit_reason();

    use crate::arch::aarch64::regs::ExitReason;

    match exit_reason {
        ExitReason::WfiWfe => {
            // Reset exception counter on successful WFI handling
            EXCEPTION_COUNT.store(0, Ordering::Relaxed);

            // In SMP mode (multiple vCPUs online), always exit on WFI
            // so the scheduler can switch to another vCPU.
            // Still inject timer if pending, but always advance PC and exit.
            let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
            let multi_vcpu = online != 0 && (online & (online - 1)) != 0; // >1 bit set

            if multi_vcpu {
                // Inject timer if pending, then exit for scheduling
                handle_wfi_with_timer_injection(context);
                context.pc += AARCH64_INSN_SIZE;
                false // Exit to scheduler
            } else {
                // Single vCPU: use existing logic
                if handle_wfi_with_timer_injection(context) {
                    context.pc += AARCH64_INSN_SIZE;
                    true // Continue with injected interrupt
                } else {
                    false
                }
            }
        }

        ExitReason::HvcCall => {
            // Reset exception counter on hypercall
            EXCEPTION_COUNT.store(0, Ordering::Relaxed);
            // HVC: Hypercall from guest
            // x0 contains the hypercall number
            let should_continue = handle_hypercall(context);
            // Don't advance PC - ELR_EL2 already points to the next instruction
            should_continue
        }

        ExitReason::TrapMsrMrs => {
            // Reset exception counter on MSR/MRS handling
            EXCEPTION_COUNT.store(0, Ordering::Relaxed);
            handle_msr_mrs_trap(context, esr);
            context.pc += AARCH64_INSN_SIZE;
            true // Continue
        }

        ExitReason::InstructionAbort => {
            uart_puts(b"[VCPU] Instruction abort at FAR=0x");
            uart_put_hex(context.sys_regs.far_el2);
            uart_puts(b" PC=0x");
            uart_put_hex(context.pc);
            uart_puts(b"\n");

            // Read EL1 registers to understand what caused the ORIGINAL EL1 exception
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
            uart_put_hex(elr_el1);
            uart_puts(b" (instruction that caused EL1 exception)\n");
            uart_puts(b"  ESR_EL1  = 0x");
            uart_put_hex(esr_el1);
            let el1_ec = (esr_el1 >> ESR_EC_SHIFT) & ESR_EC_MASK;
            uart_puts(b" (EC=0x");
            uart_put_hex(el1_ec);
            uart_puts(b")\n");
            uart_puts(b"  SPSR_EL1 = 0x");
            uart_put_hex(spsr_el1);
            uart_puts(b"\n");
            uart_puts(b"  SCTLR_EL1= 0x");
            uart_put_hex(sctlr_el1);
            uart_puts(b" (M=");
            if sctlr_el1 & 1 != 0 { uart_puts(b"1"); } else { uart_puts(b"0"); }
            uart_puts(b")\n");
            uart_puts(b"  SP_EL1   = 0x");
            uart_put_hex(sp_el1);
            uart_puts(b"\n");
            uart_puts(b"  FAR_EL1  = 0x");
            uart_put_hex(far_el1);
            uart_puts(b" (faulting address)\n");
            uart_puts(b"  TCR_EL1  = 0x");
            uart_put_hex(tcr_el1);
            uart_puts(b"\n");
            uart_puts(b"  TTBR0_EL1= 0x");
            uart_put_hex(ttbr0_el1);
            uart_puts(b"\n");
            uart_puts(b"  TTBR1_EL1= 0x");
            uart_put_hex(ttbr1_el1);
            uart_puts(b"\n");
            uart_puts(b"  VBAR_EL1 = 0x");
            uart_put_hex(vbar_el1);
            uart_puts(b"\n");

            // Also read ESR_EL2 ISS for more details on the Stage-2 fault
            let iss = esr & ESR_ISS_MASK;
            uart_puts(b"  ESR_EL2 ISS = 0x");
            uart_put_hex(iss);
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
            uart_put_hex(vtcr_el2);
            let vtcr_t0sz = vtcr_el2 & 0x3F;
            let vtcr_sl0 = (vtcr_el2 >> 6) & 0x3;
            let vtcr_ps = (vtcr_el2 >> 16) & 0x7;
            uart_puts(b" (T0SZ=");
            uart_put_hex(vtcr_t0sz);
            uart_puts(b" SL0=");
            uart_put_hex(vtcr_sl0);
            uart_puts(b" PS=");
            uart_put_hex(vtcr_ps);
            uart_puts(b")\n");
            uart_puts(b"  VTTBR_EL2= 0x");
            uart_put_hex(vttbr_el2);
            uart_puts(b"\n");
            uart_puts(b"  HCR_EL2  = 0x");
            uart_put_hex(hcr_el2);
            uart_puts(b" (VM=");
            if hcr_el2 & HCR_VM != 0 { uart_puts(b"1)\n"); } else { uart_puts(b"0)\n"); }
            uart_puts(b"  ID_AA64MMFR0_EL1 = 0x");
            uart_put_hex(id_mmfr0);
            uart_puts(b"\n");

            // Dump Stage-2 L0 table entries to verify
            let s2_l0_base = vttbr_el2 & !PAGE_OFFSET_MASK;
            uart_puts(b"  S2 L0[0] = 0x");
            let s2_l0_0 = unsafe { core::ptr::read_volatile(s2_l0_base as *const u64) };
            uart_put_hex(s2_l0_0);
            uart_puts(b"\n");
            if s2_l0_0 & (PTE_VALID | PTE_TABLE) == (PTE_VALID | PTE_TABLE) {
                let s2_l1_base = s2_l0_0 & PTE_ADDR_MASK;
                uart_puts(b"  S2 L1[0] = 0x");
                let s2_l1_0 = unsafe { core::ptr::read_volatile(s2_l1_base as *const u64) };
                uart_put_hex(s2_l1_0);
                uart_puts(b"\n");
                uart_puts(b"  S2 L1[1] = 0x");
                let s2_l1_1 = unsafe { core::ptr::read_volatile((s2_l1_base + 8) as *const u64) };
                uart_put_hex(s2_l1_1);
                uart_puts(b"\n");
            }

            // Dump the kernel L0 page table entry that caused the fault
            if far_el1 >= 0xFFFF_0000_0000_0000 {
                // TTBR1 translation - walk the page table
                let l0_base = ttbr1_el1 & !PAGE_OFFSET_MASK;
                let l0_index = ((far_el1 >> 39) & PT_INDEX_MASK) as usize;
                let l0_entry_addr = l0_base + (l0_index as u64) * 8;
                let l0_entry = unsafe { core::ptr::read_volatile(l0_entry_addr as *const u64) };
                uart_puts(b"  TTBR1 L0 base = 0x");
                uart_put_hex(l0_base);
                uart_puts(b"\n");
                uart_puts(b"  L0[");
                uart_put_hex(l0_index as u64);
                uart_puts(b"] @ 0x");
                uart_put_hex(l0_entry_addr);
                uart_puts(b" = 0x");
                uart_put_hex(l0_entry);
                let l0_valid = l0_entry & PTE_VALID;
                let l0_type = (l0_entry >> 1) & 1;
                uart_puts(b" (valid=");
                uart_put_hex(l0_valid);
                uart_puts(b" type=");
                uart_put_hex(l0_type);
                uart_puts(b")\n");
                // If L0 entry is valid table, dump the address it points to
                if l0_entry & (PTE_VALID | PTE_TABLE) == (PTE_VALID | PTE_TABLE) {
                    let l1_base = l0_entry & PTE_ADDR_MASK;
                    let l1_index = ((far_el1 >> 30) & PT_INDEX_MASK) as usize;
                    let l1_entry_addr = l1_base + (l1_index as u64) * 8;
                    let l1_entry = unsafe { core::ptr::read_volatile(l1_entry_addr as *const u64) };
                    uart_puts(b"  L1[");
                    uart_put_hex(l1_index as u64);
                    uart_puts(b"] @ 0x");
                    uart_put_hex(l1_entry_addr);
                    uart_puts(b" = 0x");
                    uart_put_hex(l1_entry);
                    uart_puts(b"\n");
                }
            }

            // This is a fatal error
            false // Exit
        }

        ExitReason::DataAbort => {
            // Data abort - determine the faulting IPA (guest physical address).
            //
            // When Stage-2 is enabled (HCR_EL2.VM=1), FAR_EL2 holds the
            // guest virtual address, NOT the IPA. The IPA page is in HPFAR_EL2.
            // We combine HPFAR_EL2 (page frame) with FAR_EL2 (page offset).
            //
            // When the guest MMU is off, VA == IPA so FAR_EL2 also works,
            // but HPFAR_EL2 is still valid and correct.
            let hpfar: u64;
            unsafe {
                core::arch::asm!(
                    "mrs {}, hpfar_el2",
                    out(reg) hpfar,
                    options(nostack, nomem),
                );
            }
            // HPFAR_EL2[43:4] = IPA[47:12] (page number)
            // FAR_EL2[11:0] = page offset within the 4KB page
            let ipa_page = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8;
            let page_offset = context.sys_regs.far_el2 & 0xFFF;
            let addr = ipa_page | page_offset;

            // Try to handle as MMIO
            if handle_mmio_abort(context, addr) {
                // Reset exception counter on successful MMIO
                EXCEPTION_COUNT.store(0, Ordering::Relaxed);
                // Successfully handled, advance PC and continue
                context.pc += AARCH64_INSN_SIZE;

                // Inject any SPIs that were just queued (e.g. virtio completion).
                // Without this, SPIs sit in PENDING_SPIS until the next vCPU exit,
                // causing unacceptable latency for virtio-blk completion interrupts.
                flush_pending_spis_to_hardware();

                true
            } else {
                // Not MMIO or failed to handle
                uart_puts(b"[VCPU] Data abort IPA=0x");
                uart_put_hex(addr);
                uart_puts(b" VA=0x");
                uart_put_hex(context.sys_regs.far_el2);
                uart_puts(b" (not MMIO)\n");
                false // Exit
            }
        }

        ExitReason::Other(ec) => {
            // Handle specific ECs that aren't fatal
            match ec {
                EC_TRAPPED_SIMD_FP => {
                    // Trapped SIMD/FP access - skip instruction
                    // (Should not happen after CPTR_EL2 fix)
                    uart_puts(b"[VCPU] FP/SIMD trap at PC=0x");
                    uart_put_hex(context.pc);
                    uart_puts(b"\n");
                    context.pc += AARCH64_INSN_SIZE;
                    true
                }
                EC_TRAPPED_SVE => {
                    // SVE/SME access trap (CPTR_EL2.TZ or TSM)
                    uart_puts(b"[VCPU] SVE/SME trap at PC=0x");
                    uart_put_hex(context.pc);
                    uart_puts(b"\n");
                    context.pc += AARCH64_INSN_SIZE;
                    true
                }
                EC_SVE_TRAP => {
                    // SVE trapped by CPTR_EL2.TZ when ZEN != 0b11
                    uart_puts(b"[VCPU] SVE trap (EC=0x19) at PC=0x");
                    uart_put_hex(context.pc);
                    uart_puts(b"\n");
                    context.pc += AARCH64_INSN_SIZE;
                    true
                }
                _ => {
                    // Unknown/unhandled exception - fatal
                    uart_puts(b"[VCPU] Unknown exception EC=0x");
                    uart_put_hex(ec);
                    uart_puts(b" ESR=0x");
                    uart_put_hex(esr);
                    uart_puts(b" PC=0x");
                    uart_put_hex(context.pc);
                    uart_puts(b"\n");
                    false // Exit
                }
            }
        }

        ExitReason::Unknown => {
            uart_puts(b"[VCPU] Unknown exception, ESR=0x");
            uart_put_hex(esr);
            uart_puts(b" PC=0x");
            uart_put_hex(context.pc);
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
pub extern "C" fn handle_irq_exception(_context: &mut VcpuContext) -> bool {
    use crate::arch::aarch64::peripherals::gicv3::{GicV3SystemRegs, GicV3VirtualInterface, VTIMER_IRQ};
    use crate::arch::aarch64::peripherals::timer;

    // Reset sync exception counter (guest is making progress)
    EXCEPTION_COUNT.store(0, Ordering::Relaxed);

    // Acknowledge the physical interrupt
    let intid = GicV3SystemRegs::read_iar1();

    // Check for spurious interrupt (INTID >= 1020)
    if intid >= GIC_SPURIOUS_INTID {
        return true;
    }

    match intid {
        0..=15 => {
            // Physical SGI arrived.
            GicV3SystemRegs::write_eoir1(intid);
            GicV3SystemRegs::write_dir(intid);

            #[cfg(feature = "multi_pcpu")]
            {
                // Multi-pCPU: physical SGIs are hypervisor IPIs (wakeup).
                // The virtual SGI is already queued in PENDING_SGIS by the
                // sender. Exit to host so the run loop injects it.
                return false;
            }

            #[cfg(not(feature = "multi_pcpu"))]
            {
                // Single-pCPU: physical SGI → inject into current vCPU.
                let current_vcpu = crate::global::current_vcpu_id();
                if current_vcpu == 0 {
                    let _ = GicV3VirtualInterface::inject_interrupt(intid, IRQ_DEFAULT_PRIORITY);
                } else {
                    crate::global::PENDING_SGIS[0].fetch_or(1 << intid, Ordering::Relaxed);
                }
                return true; // continue guest
            }
        }
        26 => {
            // EL2 hypervisor physical timer (CNTHP) — preemption watchdog.
            // This fires independently of the guest virtual timer, ensuring
            // preemption works even when the guest timer is masked (e.g.,
            // during multi_cpu_stop with IRQs disabled).
            timer::disarm_preemption_timer();
            let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
            let multi_vcpu = online != 0 && (online & (online - 1)) != 0;
            if multi_vcpu {
                crate::global::PREEMPTION_EXIT.store(true, Ordering::Release);
                GicV3SystemRegs::write_eoir1(intid);
                GicV3SystemRegs::write_dir(intid); // No HW linkage
                return false; // exit to host for scheduling
            }
            GicV3SystemRegs::write_eoir1(intid);
            GicV3SystemRegs::write_dir(intid);
            return true;
        }
        33 => {
            // Physical UART RX interrupt (SPI 1 = INTID 33).
            // Read all available bytes from physical UART into global ring buffer.
            loop {
                let fr: u32;
                unsafe {
                    core::arch::asm!(
                        "ldr {val:w}, [{addr}]",
                        addr = in(reg) (0x0900_0000usize + 0x18),
                        val = out(reg) fr,
                        options(nostack, readonly),
                    );
                }
                if fr & (1 << 4) != 0 { break; } // RXFE — FIFO empty
                let data: u32;
                unsafe {
                    core::arch::asm!(
                        "ldr {val:w}, [{addr}]",
                        addr = in(reg) 0x0900_0000usize,
                        val = out(reg) data,
                        options(nostack, readonly),
                    );
                }
                crate::global::UART_RX.push((data & 0xFF) as u8);
            }
            GicV3SystemRegs::write_eoir1(intid);
            GicV3SystemRegs::write_dir(intid);
            return false; // exit to host to deliver RX data to VirtualUart
        }
        27 => {
            // Virtual timer interrupt (PPI 27)
            // Mask the timer to stop continuous firing
            timer::mask_guest_vtimer();

            // Inject virtual interrupt to guest with HW=1.
            // HW=1 links virtual and physical interrupt: guest's virtual EOI
            // automatically deactivates the physical interrupt (pINTID=27).
            let _inject_result = GicV3VirtualInterface::inject_hw_interrupt(
                VTIMER_IRQ, VTIMER_IRQ, IRQ_DEFAULT_PRIORITY,
            );

            // DO NOT modify SPSR_EL2 (guest's saved PSTATE).

            // Single-pCPU only: demand-driven preemption — exit to host
            // when another vCPU has pending SGIs. Multi-pCPU doesn't need this
            // since each vCPU runs on its own pCPU.
            #[cfg(not(feature = "multi_pcpu"))]
            {
                let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
                let multi_vcpu = online != 0 && (online & (online - 1)) != 0;
                if multi_vcpu {
                    let current = crate::global::current_vcpu_id();
                    let mut needs_switch = false;
                    for id in 0..crate::global::MAX_VCPUS {
                        if id != current
                            && crate::global::PENDING_SGIS[id].load(Ordering::Relaxed) != 0
                        {
                            needs_switch = true;
                            break;
                        }
                    }

                    if needs_switch {
                        crate::global::PREEMPTION_EXIT.store(true, Ordering::Release);
                        GicV3SystemRegs::write_eoir1(intid);
                        return false; // exit to host
                    }
                }
            }

            // EOImode=1: priority drop only
            GicV3SystemRegs::write_eoir1(intid);
            // No DIR for HW=1 timer
            return true;
        }
        _ => {
            uart_puts(b"[IRQ] Unhandled INTID=");
            uart_put_hex(intid as u64);
            uart_puts(b"\n");
        }
    }

    // EOImode=1: EOIR only does priority drop (not deactivation).
    GicV3SystemRegs::write_eoir1(intid);

    // For non-HW interrupts, explicitly deactivate
    if intid != 27 {
        GicV3SystemRegs::write_dir(intid);
    }

    true // Continue guest
}

/// Handle MSR/MRS trap (EC=0x18)
///
/// Decodes the ISS to identify the trapped system register and emulates
/// the access.
///
/// ISS encoding (from KVM/ARM):
///   [21:20] Op0, [19:17] Op2, [16:14] Op1, [13:10] CRn, [9:5] Rt, [4:1] CRm, [0] Direction
fn handle_msr_mrs_trap(context: &mut VcpuContext, esr: u64) {
    let iss = (esr & ESR_ISS_MASK) as u32;
    let op0 = (iss >> 20) & 0x3;
    let op2 = (iss >> 17) & 0x7;
    let op1 = (iss >> 14) & 0x7;
    let crn = (iss >> 10) & 0xF;
    let rt  = ((iss >> 5) & 0x1F) as u8;
    let crm = (iss >> 1) & 0xF;
    let is_read = (iss & 1) == 1;

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
        // ICC_SGI1R_EL1 (S3_0_C12_C11_5) — Software Generated Interrupt
        // Trapped by ICH_HCR_EL2.TALL1. Decode target vCPUs and queue SGIs.
        (3, 0, 12, 11, 5) => {
            handle_sgi_trap(value);
        }
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

/// Handle trapped ICC_SGI1R_EL1 write (MSR trap via TALL1)
///
/// Decodes the SGI target affinity and INTID from the value the guest
/// intended to write, then queues SGIs in PENDING_SGIS for injection
/// on vCPU entry.
///
/// ICC_SGI1R_EL1 encoding:
///   [55:48] Aff3, [47:44] RS, [40] IRM, [39:32] Aff2,
///   [27:24] Aff1, [23:16] TargetList, [3:0] INTID
fn handle_sgi_trap(value: u64) {
    use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;

    // ICC_SGI1R_EL1 encoding (from ARM GICv3 spec):
    //   [55:48] Aff3, [47:44] RS, [40] IRM, [39:32] Aff2,
    //   [27:24] INTID, [23:16] Aff1, [15:0] TargetList
    let target_list = (value & 0xFFFF) as u32;          // bits [15:0]
    let intid = ((value >> 24) & 0xF) as u32;           // bits [27:24]
    let irm = (value >> 40) & 1;                        // bit [40]
    let current_vcpu = crate::global::current_vcpu_id();



    #[allow(unused_mut)]
    let mut _remote_queued = false;

    if irm == 1 {
        // IRM=1: target all PEs except self
        let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
        for id in 0..crate::global::MAX_VCPUS {
            if id != current_vcpu && online & (1 << id) != 0 {
                crate::global::PENDING_SGIS[id].fetch_or(1 << intid, Ordering::Release);
                _remote_queued = true;
            }
        }
    } else {
        // IRM=0: target based on TargetList bitmap (bits [15:0]).
        // Bit N of TargetList = PE with Aff0 = (RS * 16) + N.
        for bit in 0..crate::global::MAX_VCPUS {
            if target_list & (1 << bit) == 0 {
                continue;
            }
            let target_vcpu = bit;
            if target_vcpu == current_vcpu {
                // Self-targeting: inject directly into hardware LR
                let _ = GicV3VirtualInterface::inject_interrupt(intid, IRQ_DEFAULT_PRIORITY);
            } else if target_vcpu < crate::global::MAX_VCPUS {
                // Queue for target vCPU
                crate::global::PENDING_SGIS[target_vcpu]
                    .fetch_or(1 << intid, Ordering::Release);
                _remote_queued = true;
            }
        }
    }

    // Multi-pCPU: send physical SGI to wake remote pCPUs from WFI.
    // The virtual SGI is already queued in PENDING_SGIS; the physical SGI
    // just forces the target pCPU out of WFI so it can process the queue.
    // We use SGI 0 as the wakeup IPI (INTID = 0, TargetList = bitmap).
    #[cfg(feature = "multi_pcpu")]
    if _remote_queued {
        // Build target bitmap: all target vCPUs that are remote
        let mut target_bitmap: u64 = 0;
        if irm == 1 {
            let online = crate::global::VCPU_ONLINE_MASK.load(Ordering::Relaxed);
            for id in 0..crate::global::MAX_VCPUS {
                if id != current_vcpu && online & (1 << id) != 0 {
                    target_bitmap |= 1 << id;
                }
            }
        } else {
            for bit in 0..crate::global::MAX_VCPUS {
                if target_list & (1 << bit) != 0 && bit != current_vcpu {
                    target_bitmap |= 1 << bit;
                }
            }
        }
        if target_bitmap != 0 {
            send_physical_sgi(0, target_bitmap as u16);
        }
    }
}

/// Send a physical SGI (IPI) from EL2 to wake remote pCPUs.
///
/// Writes ICC_SGI1R_EL1 at EL2 (not subject to TALL1 trap).
/// INTID is placed in bits [27:24], TargetList in bits [15:0].
/// Assumes all PEs are in Aff1=0, Aff2=0, Aff3=0, RS=0.
#[cfg(feature = "multi_pcpu")]
fn send_physical_sgi(intid: u32, target_list: u16) {
    let val: u64 = ((intid as u64 & 0xF) << 24) | (target_list as u64);
    unsafe {
        core::arch::asm!(
            "msr icc_sgi1r_el1, {val}",
            "isb",
            val = in(reg) val,
            options(nostack, nomem),
        );
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
fn handle_hypercall_with_imm(context: &mut VcpuContext, hvc_imm: u32) -> bool {
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
            let ch = context.gp_regs.x1 as u8;
            uart_puts(&[ch]);
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
            uart_put_hex(hypercall_num);
            uart_puts(b"\n");
            context.gp_regs.x0 = !0; // Error
            false // Exit on error
        }
    }
}

/// Handle Jailhouse debug console hypercall
fn handle_jailhouse_debug_console(context: &mut VcpuContext) -> bool {
    let function = context.gp_regs.x0;

    match function {
        JAILHOUSE_HC_DEBUG_CONSOLE_PUTC => {
            // Output character
            let ch = context.gp_regs.x1 as u8;
            uart_puts(&[ch]);
            context.gp_regs.x0 = 0; // Success
            true // Continue
        }
        JAILHOUSE_HC_DEBUG_CONSOLE_GETC => {
            // Input character - read from real UART
            let uart_base = crate::platform::UART_BASE;
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
    let hvc_imm = (context.sys_regs.esr_el2 & ESR_HVC_IMM_MASK) as u32;
    handle_hypercall_with_imm(context, hvc_imm)
}

/// Handle PSCI (Power State Coordination Interface) calls
///
/// Implements PSCI v0.2 for guest power management.
fn handle_psci(context: &mut VcpuContext, function_id: u64) -> bool {
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
            let target_cpu = context.gp_regs.x1;
            let entry_point = context.gp_regs.x2;
            let context_id = context.gp_regs.x3;

            uart_puts(b"[PSCI] CPU_ON target=0x");
            uart_put_hex(target_cpu);
            uart_puts(b" entry=0x");
            uart_put_hex(entry_point);
            uart_puts(b"\n");

            #[cfg(not(feature = "multi_pcpu"))]
            {
                crate::global::PENDING_CPU_ON.request(target_cpu, entry_point, context_id);
            }
            #[cfg(feature = "multi_pcpu")]
            {
                let target_id = (target_cpu & 0xFF) as usize;
                if target_id < crate::global::MAX_VCPUS {
                    crate::global::PENDING_CPU_ON_PER_VCPU[target_id].request(entry_point, context_id);
                    // Wake the target pCPU from WFE
                    unsafe { core::arch::asm!("sev") };
                }
            }
            context.gp_regs.x0 = PSCI_SUCCESS;
            // Exit to host so run_smp() can pick up the request and boot the vCPU
            false
        }

        PSCI_AFFINITY_INFO_32 | PSCI_AFFINITY_INFO_64 => {
            // Return affinity state: 0 = ON, 1 = OFF, 2 = ON_PENDING
            let target_affinity = context.gp_regs.x1;
            let vcpu_id = target_affinity & 0xFF;
            let online_mask = crate::global::VCPU_ONLINE_MASK.load(Ordering::Acquire);
            if online_mask & (1 << vcpu_id) != 0 {
                context.gp_regs.x0 = 0; // ON
            } else {
                context.gp_regs.x0 = 1; // OFF
            }
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
            uart_put_hex(function_id);
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

    // Get ISS from ESR_EL2
    let iss = (context.sys_regs.esr_el2 & ESR_ISS_MASK) as u32;
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
        uart_put_hex(context.pc);
        uart_puts(b" ISV=0\n");
        return false;
    };

    // Decode the instruction
    let access = match MmioAccess::decode(insn, iss) {
        Some(a) => a,
        None => {
            uart_puts(b"[MMIO] Failed to decode instruction at 0x");
            uart_put_hex(context.pc);
            uart_puts(b"\n");
            return false;
        }
    };

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
                uart_put_hex(addr);
                uart_puts(b"\n");
                false
            }
        }
    }
}

/// WFI counter - track consecutive WFIs to detect infinite loops
static WFI_CONSECUTIVE_COUNT: AtomicU32 = AtomicU32::new(0);
static LAST_WFI_PC: AtomicU64 = AtomicU64::new(0);
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

    let pc = context.pc;
    let last_pc = LAST_WFI_PC.load(Ordering::Relaxed);

    // Check if PC changed - that means guest is making progress
    if pc != last_pc {
        WFI_CONSECUTIVE_COUNT.store(0, Ordering::Relaxed);
        LAST_WFI_PC.store(pc, Ordering::Relaxed);

        // Inject an interrupt on first WFI at new location
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, IRQ_DEFAULT_PRIORITY);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
        return true;
    }

    // Increment WFI counter
    let count = WFI_CONSECUTIVE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    if count > MAX_CONSECUTIVE_WFI {
        uart_puts(b"[WFI] Guest idle (");
        uart_put_hex(count as u64);
        uart_puts(b" WFIs at same PC), exiting\n");
        return false;
    }

    // Check if virtual timer is pending
    if timer::is_guest_vtimer_pending() {
        WFI_CONSECUTIVE_COUNT.store(0, Ordering::Relaxed);
        timer::mask_guest_vtimer();
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, IRQ_DEFAULT_PRIORITY);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
        return true;
    }

    // Check if any virtual interrupt is pending in List Registers
    if GicV3VirtualInterface::pending_count() > 0 {
        WFI_CONSECUTIVE_COUNT.store(0, Ordering::Relaxed);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
        return true;
    }

    // No interrupts pending - inject periodic tick to help guest make progress
    if count % 100 == 0 {
        let _ = GicV3VirtualInterface::inject_interrupt(VTIMER_IRQ, IRQ_DEFAULT_PRIORITY);
        // Don't modify SPSR_EL2 - respect guest's PSTATE.I
    }

    true
}

/// Flush pending SPIs for the current vCPU directly into hardware ICH_LRs.
///
/// Called from the exception handler (still at EL2) right before ERET,
/// so the hardware List Registers are live. This avoids the latency of
/// waiting until the next run_smp() iteration to inject completion interrupts.
fn flush_pending_spis_to_hardware() {
    use crate::arch::aarch64::peripherals::gicv3::GicV3VirtualInterface;

    let vcpu_id = crate::global::current_vcpu_id();
    if vcpu_id >= crate::global::MAX_VCPUS {
        return;
    }

    let pending = crate::global::PENDING_SPIS[vcpu_id].swap(0, Ordering::Acquire);
    if pending == 0 {
        return;
    }

    for bit in 0..32u32 {
        if pending & (1 << bit) == 0 {
            continue;
        }
        let intid = bit + 32; // SPI INTIDs start at 32
        if GicV3VirtualInterface::inject_interrupt(intid, IRQ_DEFAULT_PRIORITY).is_err() {
            // No free LR — re-queue for later
            crate::global::PENDING_SPIS[vcpu_id].fetch_or(1 << bit, Ordering::Relaxed);
        }
    }
}
