/// ARM Generic Interrupt Controller v3/v4 support
///
/// GICv3 introduces major architectural changes:
/// - System register interface instead of MMIO for CPU interface
/// - Redistributors (GICR) instead of CPU interfaces
/// - Affinity routing for SMP support
/// - Virtualization support with virtual CPU interface
///
/// For EL2 hypervisor usage, we primarily use:
/// - ICC_*_EL2 system registers for interrupt control
/// - ICH_*_EL2 system registers for virtual interrupt injection

use core::arch::asm;
use super::super::defs::*;

/// Virtual Timer interrupt (PPI 27)
pub const VTIMER_IRQ: u32 = 27;

/// Physical Timer interrupt (PPI 30)
pub const PTIMER_IRQ: u32 = 30;

/// GICv3 System Register Interface
pub struct GicV3SystemRegs;

impl GicV3SystemRegs {
    /// Read ICC_SRE_EL2 - System Register Enable (EL2)
    #[inline]
    pub fn read_sre_el2() -> u32 {
        let sre: u64;
        unsafe {
            asm!(
                "mrs {sre}, ICC_SRE_EL2",
                sre = out(reg) sre,
                options(nostack, nomem),
            );
        }
        sre as u32
    }

    /// Write ICC_SRE_EL2
    /// Bit 0 (SRE): Enable system register interface at EL2
    /// Bit 3 (Enable): Enable lower EL access to ICC_* registers
    #[inline]
    pub fn write_sre_el2(value: u32) {
        unsafe {
            asm!(
                "msr ICC_SRE_EL2, {value}",
                value = in(reg) value as u64,
                options(nostack, nomem),
            );
            asm!("isb", options(nostack, nomem));
        }
    }

    /// Read ICC_SRE_EL1 - System Register Enable (EL1)
    #[inline]
    pub fn read_sre_el1() -> u32 {
        let sre: u64;
        unsafe {
            asm!(
                "mrs {sre}, ICC_SRE_EL1",
                sre = out(reg) sre,
                options(nostack, nomem),
            );
        }
        sre as u32
    }

    /// Write ICC_SRE_EL1
    /// Bit 0 (SRE): Enable system register interface at EL1
    #[inline]
    pub fn write_sre_el1(value: u32) {
        unsafe {
            asm!(
                "msr ICC_SRE_EL1, {value}",
                value = in(reg) value as u64,
                options(nostack, nomem),
            );
            asm!("isb", options(nostack, nomem));
        }
    }

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
    /// With EOImode=0: does both priority drop and deactivation.
    /// With EOImode=1: only does priority drop (use write_dir for deactivation).
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

    /// Write ICC_DIR_EL1 - Deactivate Interrupt Register
    /// Used when EOImode=1 to explicitly deactivate a physical interrupt.
    #[inline]
    pub fn write_dir(intid: u32) {
        unsafe {
            asm!(
                "msr ICC_DIR_EL1, {intid}",
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
        Self::write_pmr(ICC_PMR_ALLOW_ALL);

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

/// List Register state values
impl GicV3VirtualInterface {
    /// LR State: Invalid (free)
    pub const LR_STATE_INVALID: u64 = 0b00;
    /// LR State: Pending
    pub const LR_STATE_PENDING: u64 = 0b01;
    /// LR State: Active
    pub const LR_STATE_ACTIVE: u64 = 0b10;
    /// LR State: Pending and Active
    pub const LR_STATE_PENDING_ACTIVE: u64 = 0b11;
}

impl GicV3VirtualInterface {
    /// Read ICH_HCR_EL2 - Hypervisor Control Register
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

    /// Read ICH_VMCR_EL2 - Virtual Machine Control Register
    #[inline]
    pub fn read_vmcr() -> u32 {
        let vmcr: u64;
        unsafe {
            asm!(
                "mrs {vmcr}, ICH_VMCR_EL2",
                vmcr = out(reg) vmcr,
                options(nostack, nomem),
            );
        }
        vmcr as u32
    }

    /// Write ICH_VMCR_EL2
    #[inline]
    pub fn write_vmcr(value: u32) {
        unsafe {
            asm!(
                "msr ICH_VMCR_EL2, {value}",
                value = in(reg) value as u64,
                options(nostack, nomem),
            );
        }
    }

    /// Read ICH_VTR_EL2 - VGIC Type Register
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
    pub fn inject_interrupt(intid: u32, priority: u8) -> Result<(), &'static str> {
        // Find a free list register
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & VTR_LISTREGS_MASK) + 1) as u32;

        for i in 0..num_lrs {
            let lr = Self::read_lr(i);
            let state = (lr >> LR_STATE_SHIFT) & LR_STATE_MASK;

            // If state is 00 (Invalid), this LR is free
            if state == 0 {
                let lr_value = (Self::LR_STATE_PENDING << LR_STATE_SHIFT)
                              | LR_GROUP1_BIT
                              | ((priority as u64) << LR_PRIORITY_SHIFT)
                              | (intid as u64);

                Self::write_lr(i, lr_value);
                return Ok(());
            }
        }

        Err("No free list register for interrupt injection")
    }

    /// Inject a hardware-linked virtual interrupt (HW=1)
    ///
    /// When HW=1, the guest's virtual EOI automatically deactivates the
    /// physical interrupt identified by `pintid`.
    pub fn inject_hw_interrupt(intid: u32, pintid: u32, priority: u8) -> Result<(), &'static str> {
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & VTR_LISTREGS_MASK) + 1) as u32;

        // First, clean up any stale Active LR for this intid
        for i in 0..num_lrs {
            let lr = Self::read_lr(i);
            let lr_intid = (lr & LR_VINTID_MASK) as u32;
            let state = (lr >> LR_STATE_SHIFT) & LR_STATE_MASK;
            if lr_intid == intid && state == Self::LR_STATE_ACTIVE {
                Self::write_lr(i, 0);
            }
        }

        // Find a free LR and inject
        for i in 0..num_lrs {
            let lr = Self::read_lr(i);
            let state = (lr >> LR_STATE_SHIFT) & LR_STATE_MASK;

            if state == 0 {
                let lr_value = (Self::LR_STATE_PENDING << LR_STATE_SHIFT)
                              | LR_HW_BIT
                              | LR_GROUP1_BIT
                              | ((priority as u64) << LR_PRIORITY_SHIFT)
                              | (((pintid as u64) & LR_PINTID_MASK) << LR_PINTID_SHIFT)
                              | (intid as u64);

                Self::write_lr(i, lr_value);
                return Ok(());
            }
        }

        Err("No free list register for interrupt injection")
    }

    /// Clear a virtual interrupt from list registers
    pub fn clear_interrupt(intid: u32) {
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & VTR_LISTREGS_MASK) + 1) as u32;

        for i in 0..num_lrs {
            let lr = Self::read_lr(i);
            let lr_intid = (lr & LR_VINTID_MASK) as u32;

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
        Self::write_hcr(1);

        // Configure ICH_VMCR_EL2 for guest virtual CPU interface
        // Bits [31:24]: VPMR = 0xFF (allow all priorities)
        // Bit 1: VENG1 = 1 (enable Group 1 interrupts for guest)
        let vmcr: u32 = ((ICC_PMR_ALLOW_ALL as u32) << 24) // VPMR
                        | (1 << 1);                          // VENG1
        Self::write_vmcr(vmcr);

        // Clear all list registers
        let vtr = Self::read_vtr();
        let num_lrs = ((vtr & VTR_LISTREGS_MASK) + 1) as u32;

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
        ((vtr & VTR_LISTREGS_MASK) + 1) as u32
    }

    /// Build a List Register value
    pub fn build_lr(intid: u32, priority: u8) -> u64 {
        (Self::LR_STATE_PENDING << LR_STATE_SHIFT)
            | LR_GROUP1_BIT
            | ((priority as u64) << LR_PRIORITY_SHIFT)
            | (intid as u64)
    }

    /// Extract the state field from a List Register value
    #[inline]
    pub fn get_lr_state(lr: u64) -> u64 {
        (lr >> LR_STATE_SHIFT) & LR_STATE_MASK
    }

    /// Extract the INTID field from a List Register value
    #[inline]
    pub fn get_lr_intid(lr: u64) -> u32 {
        (lr & LR_VINTID_MASK) as u32
    }

    /// Extract the priority field from a List Register value
    #[inline]
    pub fn get_lr_priority(lr: u64) -> u8 {
        ((lr >> LR_PRIORITY_SHIFT) & 0xFF) as u8
    }

    /// Find a free (invalid state) List Register
    pub fn find_free_lr() -> Option<usize> {
        let num_lrs = Self::num_list_registers() as usize;

        for i in 0..num_lrs {
            let lr = Self::read_lr(i as u32);
            if Self::get_lr_state(lr) == Self::LR_STATE_INVALID {
                return Some(i);
            }
        }
        None
    }

    /// Get count of pending interrupts in List Registers
    pub fn pending_count() -> usize {
        let num_lrs = Self::num_list_registers() as usize;
        let mut count = 0;

        for i in 0..num_lrs {
            let lr = Self::read_lr(i as u32);
            let state = Self::get_lr_state(lr);
            if state == Self::LR_STATE_PENDING || state == Self::LR_STATE_PENDING_ACTIVE {
                count += 1;
            }
        }
        count
    }

    /// Check if GICv3 system register interface is available
    pub fn is_available() -> bool {
        is_gicv3_available()
    }

    /// Check ARMv8.4+ features for enhanced virtualization
    pub fn has_armv8_4_features() -> bool {
        let mmfr2: u64;
        unsafe {
            asm!(
                "mrs {mmfr2}, ID_AA64MMFR2_EL1",
                mmfr2 = out(reg) mmfr2,
                options(nostack, nomem),
            );
        }

        // Bits [27:24] = NV support (nested virtualization)
        let nv = (mmfr2 >> 24) & 0xF;
        nv >= 1
    }
}

/// Check if GICv3 is available
pub fn is_gicv3_available() -> bool {
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
        super::gic::init();
        return;
    }

    crate::uart_puts(b"[GIC] Initializing GICv3/v4 (system register interface)...\n");

    // Configure ICC_SRE_EL2 - CRITICAL for guest interrupt handling
    let sre_el2: u32 = ICC_SRE_SRE | ICC_SRE_ENABLE;
    GicV3SystemRegs::write_sre_el2(sre_el2);
    crate::uart_puts(b"[GIC] ICC_SRE_EL2 configured (Enable=1, SRE=1)\n");

    // Also set ICC_SRE_EL1 to enable system register interface for guest
    GicV3SystemRegs::write_sre_el1(ICC_SRE_SRE);

    // Read VGIC type to report capabilities
    let vtr = GicV3VirtualInterface::read_vtr();
    let num_lrs = ((vtr & VTR_LISTREGS_MASK) + 1) as u32;
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

    // Set EOImode=1 so EOIR only does priority drop (not deactivation).
    let ctlr = GicV3SystemRegs::read_ctlr();
    GicV3SystemRegs::write_ctlr(ctlr | ICC_CTLR_EOIMODE);
    crate::uart_puts(b"[GIC] ICC_CTLR_EL1.EOImode=1 (split priority drop/deactivation)\n");

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
        let intid = 27u32;
        let priority = IRQ_DEFAULT_PRIORITY;

        let lr_value = (GicV3VirtualInterface::LR_STATE_PENDING << LR_STATE_SHIFT)
                      | LR_GROUP1_BIT
                      | ((priority as u64) << LR_PRIORITY_SHIFT)
                      | (intid as u64);

        // Extract fields
        let state = (lr_value >> LR_STATE_SHIFT) & LR_STATE_MASK;
        let group = (lr_value >> 60) & 0x1;
        let prio = ((lr_value >> LR_PRIORITY_SHIFT) & 0xFF) as u8;
        let intid_out = (lr_value & LR_VINTID_MASK) as u32;

        assert_eq!(state, 1); // Pending
        assert_eq!(group, 1); // Group1
        assert_eq!(prio, priority);
        assert_eq!(intid_out, intid);
    }
}
