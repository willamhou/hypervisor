/// ARM Generic Interrupt Controller v3/v4 support
///
/// GICv3 introduces major architectural changes:
/// - System register interface instead of MMIO for CPU interface
/// - Redistributors (GICR) instead of CPU interfaces
/// - Affinity routing for SMP support
/// - Virtualization support with virtual CPU interface
///
/// GICv4 adds:
/// - Direct injection of virtual LPIs
/// - Doorbell interrupts
///
/// For EL2 hypervisor usage, we primarily use:
/// - ICC_*_EL2 system registers for interrupt control
/// - ICH_*_EL2 system registers for virtual interrupt injection

use core::arch::asm;

/// Virtual Timer interrupt (PPI 27)
pub const VTIMER_IRQ: u32 = 27;

/// Physical Timer interrupt (PPI 30) 
pub const PTIMER_IRQ: u32 = 30;

/// GICv3 System Register Interface
pub struct GicV3SystemRegs;

impl GicV3SystemRegs {
    /// Read ICC_IAR1_EL1 - Interrupt Acknowledge Register
    /// Returns the INTID of the highest priority pending interrupt
    #[inline]
    pub fn read_iar1() -> u32 {
        let iar: u64;
        unsafe {
            asm!(
                "mrs {iar}, ICC_IAR1_EL1",
                iar = out(reg) iar,
                options(nostack, nomem),
            );
        }
        iar as u32
    }

    /// Write ICC_EOIR1_EL1 - End Of Interrupt Register
    /// Signals completion of interrupt processing
    #[inline]
    pub fn write_eoir1(intid: u32) {
        unsafe {
            asm!(
                "msr ICC_EOIR1_EL1, {intid}",
                intid = in(reg) intid as u64,
                options(nostack, nomem),
            );
        }
    }

    /// Read ICC_CTLR_EL1 - Interrupt Controller Control Register
    #[inline]
    pub fn read_ctlr() -> u32 {
        let ctlr: u64;
        unsafe {
            asm!(
                "mrs {ctlr}, ICC_CTLR_EL1",
                ctlr = out(reg) ctlr,
                options(nostack, nomem),
            );
        }
        ctlr as u32
    }

    /// Write ICC_CTLR_EL1
    #[inline]
    pub fn write_ctlr(value: u32) {
        unsafe {
            asm!(
                "msr ICC_CTLR_EL1, {value}",
                value = in(reg) value as u64,
                options(nostack, nomem),
            );
        }
    }

    /// Read ICC_PMR_EL1 - Priority Mask Register
    /// Only interrupts with higher priority than this value are signaled
    #[inline]
    pub fn read_pmr() -> u32 {
        let pmr: u64;
        unsafe {
            asm!(
                "mrs {pmr}, ICC_PMR_EL1",
                pmr = out(reg) pmr,
                options(nostack, nomem),
            );
        }
        pmr as u32
    }

    /// Write ICC_PMR_EL1
    /// Set to 0xFF to allow all interrupts
    #[inline]
    pub fn write_pmr(priority: u32) {
        unsafe {
            asm!(
                "msr ICC_PMR_EL1, {priority}",
                priority = in(reg) priority as u64,
                options(nostack, nomem),
            );
        }
    }

    /// Read ICC_BPR1_EL1 - Binary Point Register
    #[inline]
    pub fn read_bpr1() -> u32 {
        let bpr: u64;
        unsafe {
            asm!(
                "mrs {bpr}, ICC_BPR1_EL1",
                bpr = out(reg) bpr,
                options(nostack, nomem),
            );
        }
        bpr as u32
    }

    /// Write ICC_BPR1_EL1
    #[inline]
    pub fn write_bpr1(value: u32) {
        unsafe {
            asm!(
                "msr ICC_BPR1_EL1, {value}",
                value = in(reg) value as u64,
                options(nostack, nomem),
            );
        }
    }

    /// Read ICC_IGRPEN1_EL1 - Interrupt Group 1 Enable
    #[inline]
    pub fn read_igrpen1() -> u32 {
        let igrpen: u64;
        unsafe {
            asm!(
                "mrs {igrpen}, ICC_IGRPEN1_EL1",
                igrpen = out(reg) igrpen,
                options(nostack, nomem),
            );
        }
        igrpen as u32
    }

    /// Write ICC_IGRPEN1_EL1
    /// Bit 0: Enable Group 1 interrupts
    #[inline]
    pub fn write_igrpen1(enable: bool) {
        let value = if enable { 1u64 } else { 0u64 };
        unsafe {
            asm!(
                "msr ICC_IGRPEN1_EL1, {value}",
                value = in(reg) value,
                options(nostack, nomem),
            );
        }
    }

    /// Enable interrupt delivery to the CPU
    pub fn enable() {
        // Set priority mask to lowest priority (allow all interrupts)
        Self::write_pmr(0xFF);
        
        // Enable Group 1 interrupts
        Self::write_igrpen1(true);
        
        // Ensure changes are visible
        unsafe {
            asm!("isb", options(nostack, nomem));
        }
    }

    /// Disable interrupt delivery
    pub fn disable() {
        Self::write_igrpen1(false);
        unsafe {
            asm!("isb", options(nostack, nomem));
        }
    }
}

/// GICv3 Virtual Interface (for interrupt injection)
/// 
/// These registers are used by the hypervisor to inject virtual interrupts
/// into the guest. The guest sees these as real interrupts through its
/// ICC_* registers.
pub struct GicV3VirtualInterface;

impl GicV3VirtualInterface {
    /// Read ICH_HCR_EL2 - Hypervisor Control Register
    /// Controls virtual interrupt behavior
    #[inline]
    pub fn read_hcr() -> u32 {
        let hcr: u64;
        unsafe {
            asm!(
                "mrs {hcr}, ICH_HCR_EL2",
                hcr = out(reg) hcr,
                options(nostack, nomem),
            );
        }
        hcr as u32
    }

    /// Write ICH_HCR_EL2
    /// Bit 0 (En): Enable virtual interrupts
    #[inline]
    pub fn write_hcr(value: u32) {
        unsafe {
            asm!(
                "msr ICH_HCR_EL2, {value}",
                value = in(reg) value as u64,
                options(nostack, nomem),
            );
        }
    }

    /// Read ICH_VTR_EL2 - VGIC Type Register
    /// Reports the number of list registers and priority bits
    #[inline]
    pub fn read_vtr() -> u32 {
        let vtr: u64;
        unsafe {
            asm!(
                "mrs {vtr}, ICH_VTR_EL2",
                vtr = out(reg) vtr,
                options(nostack, nomem),
            );
        }
        vtr as u32
    }

    /// Read ICH_LR<n>_EL2 - List Register
    /// Contains virtual interrupt state
    #[inline]
    pub fn read_lr(n: u32) -> u64 {
        let lr: u64;
        unsafe {
            match n {
                0 => asm!("mrs {lr}, ICH_LR0_EL2", lr = out(reg) lr, options(nostack, nomem)),
                1 => asm!("mrs {lr}, ICH_LR1_EL2", lr = out(reg) lr, options(nostack, nomem)),
                2 => asm!("mrs {lr}, ICH_LR2_EL2", lr = out(reg) lr, options(nostack, nomem)),
                3 => asm!("mrs {lr}, ICH_LR3_EL2", lr = out(reg) lr, options(nostack, nomem)),
                _ => lr = 0, // Only 4 LRs typically available
            }
        }
        lr
    }

    /// Write ICH_LR<n>_EL2 - List Register
    /// 
    /// List Register format:
    /// Bits [63:62]: State (00=Invalid, 01=Pending, 10=Active, 11=Pending+Active)
    /// Bit  [61]:    HW (0=software, 1=hardware)
    /// Bit  [60]:    Group (0=Group0, 1=Group1)
    /// Bits [59:56]: Priority
    /// Bits [31:0]:  vINTID (virtual interrupt ID)
    #[inline]
    pub fn write_lr(n: u32, value: u64) {
        unsafe {
            match n {
                0 => asm!("msr ICH_LR0_EL2, {value}", value = in(reg) value, options(nostack, nomem)),
                1 => asm!("msr ICH_LR1_EL2, {value}", value = in(reg) value, options(nostack, nomem)),
                2 => asm!("msr ICH_LR2_EL2, {value}", value = in(reg) value, options(nostack, nomem)),
                3 => asm!("msr ICH_LR3_EL2, {value}", value = in(reg) value, options(nostack, nomem)),
                _ => {}, // Ignore invalid LR number
            }
            asm!("isb", options(nostack, nomem));
        }
    }

    /// Inject a virtual interrupt into the guest
    /// 
    /// # Arguments
    /// * `intid` - Interrupt ID to inject
    /// * `priority` - Interrupt priority (0 = highest)
    /// 
    /// # Returns
    /// * `Ok(())` - Interrupt injected successfully
    /// * `Err(msg)` - No free list register available
    pub fn inject_interrupt(intid: u32, priority: u8) -> Result<(), &'static str> {
        // Find a free list register
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & 0x1F) + 1) as u32; // Bits [4:0] = ListRegs - 1
        
        for i in 0..num_lrs {
            let lr = Self::read_lr(i);
            let state = (lr >> 62) & 0x3;
            
            // If state is 00 (Invalid), this LR is free
            if state == 0 {
                // Build LR value:
                // State = 01 (Pending)
                // HW = 0 (software interrupt)
                // Group = 1 (Group1)
                // Priority in bits [55:48] (8 bits)
                // vINTID in bits [31:0]
                let lr_value = (1u64 << 62)                    // State = Pending
                              | (1u64 << 60)                    // Group1
                              | ((priority as u64) << 48)       // Priority
                              | (intid as u64);                 // vINTID
                
                Self::write_lr(i, lr_value);
                return Ok(());
            }
        }
        
        Err("No free list register for interrupt injection")
    }

    /// Clear a virtual interrupt from list registers
    pub fn clear_interrupt(intid: u32) {
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & 0x1F) + 1) as u32;
        
        for i in 0..num_lrs {
            let lr = Self::read_lr(i);
            let lr_intid = (lr & 0xFFFF_FFFF) as u32;
            
            if lr_intid == intid {
                // Set state to Invalid (00)
                Self::write_lr(i, 0);
                return;
            }
        }
    }

    /// Initialize virtual interrupt interface
    pub fn init() {
        // Enable virtual interrupts
        // Bit 0 (En): Enable
        Self::write_hcr(1);
        
        // Clear all list registers
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & 0x1F) + 1) as u32;
        
        for i in 0..num_lrs {
            Self::write_lr(i, 0);
        }
        
        unsafe {
            asm!("isb", options(nostack, nomem));
        }
    }

    /// Get number of available list registers
    pub fn num_list_registers() -> u32 {
        let vtr = Self::read_vtr();
        ((vtr & 0x1F) + 1) as u32
    }
}

/// Check if GICv3 is available
/// Returns true if GICv3 system registers are accessible
pub fn is_gicv3_available() -> bool {
    // Try to read ID_AA64PFR0_EL1 to check GIC system register interface
    let pfr0: u64;
    unsafe {
        asm!(
            "mrs {pfr0}, ID_AA64PFR0_EL1",
            pfr0 = out(reg) pfr0,
            options(nostack, nomem),
        );
    }
    
    // Bits [27:24] = GIC (0000 = None, 0001 = GICv3/v4 via system registers)
    let gic_version = (pfr0 >> 24) & 0xF;
    gic_version >= 1
}

/// Initialize GICv3 for hypervisor use
pub fn init() {
    crate::uart_puts(b"[GIC] Checking GICv3/v4 availability...\n");
    
    if !is_gicv3_available() {
        crate::uart_puts(b"[GIC] GICv3 not available, falling back to GICv2\n");
        // Fall back to GICv2 initialization
        super::gic::init();
        return;
    }
    
    crate::uart_puts(b"[GIC] Initializing GICv3/v4 (system register interface)...\n");
    
    // Read VGIC type to report capabilities
    // Note: This requires EL2 virtualization extensions
    let vtr = GicV3VirtualInterface::read_vtr();
    let num_lrs = ((vtr & 0x1F) + 1) as u32;
    let num_priority_bits = ((vtr >> 29) & 0x7) + 1;
    
    crate::uart_puts(b"[GIC] VGIC capabilities:\n");
    crate::uart_puts(b"  - List Registers: ");
    print_num(num_lrs);
    crate::uart_puts(b"\n");
    crate::uart_puts(b"  - Priority bits: ");
    print_num(num_priority_bits);
    crate::uart_puts(b"\n");
    
    // Initialize virtual interrupt interface
    GicV3VirtualInterface::init();
    
    // Enable interrupt delivery to this CPU
    GicV3SystemRegs::enable();
    
    crate::uart_puts(b"[GIC] GICv3 initialization complete\n");
}

/// Helper to print a number
fn print_num(n: u32) {
    if n >= 10 {
        print_num(n / 10);
    }
    let digit = (b'0' + (n % 10) as u8) as u8;
    crate::uart_puts(&[digit]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lr_format() {
        // Test list register encoding
        let intid = 27u32;
        let priority = 0xA0u8;
        
        let lr_value = (1u64 << 62)                    // State = Pending
                      | (1u64 << 60)                    // Group1
                      | ((priority as u64) << 48)       // Priority
                      | (intid as u64);                 // vINTID
        
        // Extract fields
        let state = (lr_value >> 62) & 0x3;
        let group = (lr_value >> 60) & 0x1;
        let prio = ((lr_value >> 48) & 0xFF) as u8;
        let intid_out = (lr_value & 0xFFFF_FFFF) as u32;
        
        assert_eq!(state, 1); // Pending
        assert_eq!(group, 1); // Group1
        assert_eq!(prio, priority);
        assert_eq!(intid_out, intid);
    }
}
