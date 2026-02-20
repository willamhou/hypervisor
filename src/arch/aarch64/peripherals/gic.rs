use crate::platform;
/// ARM Generic Interrupt Controller (GICv2) support
///
/// This module provides basic GIC configuration for handling interrupts in the hypervisor.
///
/// GIC Architecture:
/// - GICD (Distributor): Manages interrupt prioritization and distribution
/// - GICC (CPU Interface): Per-CPU interface for interrupt acknowledgment and EOI
///
/// Interrupt Types:
/// - SGI (0-15): Software Generated Interrupts
/// - PPI (16-31): Private Peripheral Interrupts (per-CPU, includes timers)
/// - SPI (32-1019): Shared Peripheral Interrupts
use core::ptr::{read_volatile, write_volatile};

/// GICD Register offsets
const GICD_CTLR: u64 = 0x000; // Distributor Control Register
const GICD_TYPER: u64 = 0x004; // Interrupt Controller Type Register
const GICD_ISENABLER: u64 = 0x100; // Interrupt Set-Enable Registers
const GICD_ICENABLER: u64 = 0x180; // Interrupt Clear-Enable Registers
#[allow(dead_code)]
const GICD_ISPENDR: u64 = 0x200; // Interrupt Set-Pending Registers
const GICD_ICPENDR: u64 = 0x280; // Interrupt Clear-Pending Registers
const GICD_IPRIORITYR: u64 = 0x400; // Interrupt Priority Registers
#[allow(dead_code)]
const GICD_ITARGETSR: u64 = 0x800; // Interrupt Processor Targets Registers
#[allow(dead_code)]
const GICD_ICFGR: u64 = 0xC00; // Interrupt Configuration Registers

/// GICC Register offsets
const GICC_CTLR: u64 = 0x000; // CPU Interface Control Register
const GICC_PMR: u64 = 0x004; // Interrupt Priority Mask Register
const GICC_IAR: u64 = 0x00C; // Interrupt Acknowledge Register
const GICC_EOIR: u64 = 0x010; // End of Interrupt Register

/// Virtual Timer interrupt number (PPI 27)
pub const VTIMER_IRQ: u32 = 27;

/// Physical Timer interrupt number (PPI 30)
pub const PTIMER_IRQ: u32 = 30;

/// GIC Distributor wrapper
pub struct GicDistributor {
    base: u64,
}

impl GicDistributor {
    pub const fn new(base: u64) -> Self {
        Self { base }
    }

    /// Read a 32-bit register
    fn read_reg(&self, offset: u64) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    /// Write a 32-bit register
    fn write_reg(&self, offset: u64, value: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, value) }
    }

    /// Initialize the distributor
    pub fn init(&self) {
        // Disable distributor
        self.write_reg(GICD_CTLR, 0);

        // Read the number of interrupt lines
        let typer = self.read_reg(GICD_TYPER);
        let it_lines = ((typer & 0x1F) + 1) * 32;

        // Disable all interrupts
        for i in 0..(it_lines / 32) {
            self.write_reg(GICD_ICENABLER + (i as u64 * 4), 0xFFFF_FFFF);
        }

        // Clear all pending interrupts
        for i in 0..(it_lines / 32) {
            self.write_reg(GICD_ICPENDR + (i as u64 * 4), 0xFFFF_FFFF);
        }

        // Set all priorities to default (0xA0)
        for i in 0..it_lines {
            self.write_reg(GICD_IPRIORITYR + (i as u64), 0xA0);
        }

        // Enable distributor
        self.write_reg(GICD_CTLR, 1);
    }

    /// Enable an interrupt
    pub fn enable_irq(&self, irq: u32) {
        let reg = irq / 32;
        let bit = irq % 32;
        self.write_reg(GICD_ISENABLER + (reg as u64 * 4), 1 << bit);
    }

    /// Disable an interrupt
    pub fn disable_irq(&self, irq: u32) {
        let reg = irq / 32;
        let bit = irq % 32;
        self.write_reg(GICD_ICENABLER + (reg as u64 * 4), 1 << bit);
    }

    /// Set interrupt priority (0 = highest, 255 = lowest)
    pub fn set_priority(&self, irq: u32, priority: u8) {
        self.write_reg(GICD_IPRIORITYR + (irq as u64), priority as u32);
    }
}

/// GIC CPU Interface wrapper
pub struct GicCpuInterface {
    base: u64,
}

impl GicCpuInterface {
    pub const fn new(base: u64) -> Self {
        Self { base }
    }

    /// Read a 32-bit register
    fn read_reg(&self, offset: u64) -> u32 {
        unsafe { read_volatile((self.base + offset) as *const u32) }
    }

    /// Write a 32-bit register
    fn write_reg(&self, offset: u64, value: u32) {
        unsafe { write_volatile((self.base + offset) as *mut u32, value) }
    }

    /// Initialize the CPU interface
    pub fn init(&self) {
        // Set priority mask to lowest priority (allow all interrupts)
        self.write_reg(GICC_PMR, 0xFF);

        // Enable CPU interface
        self.write_reg(GICC_CTLR, 1);
    }

    /// Acknowledge an interrupt (returns interrupt ID)
    pub fn acknowledge(&self) -> u32 {
        self.read_reg(GICC_IAR)
    }

    /// Signal end of interrupt
    pub fn end_of_interrupt(&self, irq: u32) {
        self.write_reg(GICC_EOIR, irq);
    }
}

/// Global GIC instances
pub static GICD: GicDistributor = GicDistributor::new(platform::GICD_BASE);
pub static GICC: GicCpuInterface = GicCpuInterface::new(platform::GICC_BASE);

/// Initialize the GIC
pub fn init() {
    crate::uart_puts(b"[GIC] Initializing GICv2 (system register interface)...\n");

    // For GICv2 in QEMU, we'll skip detailed initialization for now
    // The GIC should be in a usable state by default
    // We'll enable interrupt routing through HCR_EL2 (already done in exception::init)

    crate::uart_puts(b"[GIC] Using default GIC configuration\n");
}
