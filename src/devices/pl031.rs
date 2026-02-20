/// Virtual RTC (PL031) device
///
/// Minimal trap-and-emulate PL031 RTC for Linux guest probing.
/// Uses the ARM architectural counter (CNTVCT_EL0 / CNTFRQ_EL0) as the
/// time source so the guest sees monotonically increasing seconds.
///
/// Register map (offsets from base 0x0901_0000):
///   0x000 RTCDR  — Data Register (read-only, current time in seconds)
///   0x004 RTCMR  — Match Register (stub)
///   0x008 RTCLR  — Load Register (write-only, sets epoch)
///   0x00C RTCCR  — Control Register (bit 0 = enable, default 1)
///   0x010 RTCIMSC — Interrupt Mask Set/Clear (stub)
///   0x014 RTCRIS — Raw Interrupt Status (stub)
///   0x018 RTCMIS — Masked Interrupt Status (stub)
///   0x01C RTCICR — Interrupt Clear Register (stub)
///   0xFE0-0xFFC — PrimeCell identification registers
use crate::devices::MmioDevice;

/// PL031 RTC base address (QEMU virt machine)
pub const PL031_BASE: u64 = 0x0901_0000;

const PL031_SIZE: u64 = 0x1000;

// ── Register offsets ────────────────────────────────────────────────

const RTCDR: u64 = 0x000;
const RTCMR: u64 = 0x004;
const RTCLR: u64 = 0x008;
const RTCCR: u64 = 0x00C;
const RTCIMSC: u64 = 0x010;
const RTCRIS: u64 = 0x014;
const RTCMIS: u64 = 0x018;
const RTCICR: u64 = 0x01C;

// PL031 PrimeCell identification registers
const PERIPHID0: u64 = 0xFE0;
const PERIPHID1: u64 = 0xFE4;
const PERIPHID2: u64 = 0xFE8;
const PERIPHID3: u64 = 0xFEC;
const PCELLID0: u64 = 0xFF0;
const PCELLID1: u64 = 0xFF4;
const PCELLID2: u64 = 0xFF8;
const PCELLID3: u64 = 0xFFC;

// ── Counter helpers ─────────────────────────────────────────────────

/// Read the virtual counter (CNTVCT_EL0).
fn read_cntvct() -> u64 {
    let val: u64;
    unsafe {
        core::arch::asm!(
            "mrs {}, cntvct_el0",
            out(reg) val,
            options(nostack, nomem),
        );
    }
    val
}

/// Read the counter frequency (CNTFRQ_EL0).
fn read_cntfrq() -> u64 {
    let val: u64;
    unsafe {
        core::arch::asm!(
            "mrs {}, cntfrq_el0",
            out(reg) val,
            options(nostack, nomem),
        );
    }
    val
}

// ── Virtual PL031 device ────────────────────────────────────────────

/// Virtual PL031 RTC device.
///
/// Tracks a load value (epoch seconds) and the counter snapshot at the
/// time it was set.  RTCDR returns `load_value + elapsed_seconds`.
pub struct VirtualPl031 {
    /// Base epoch set via RTCLR (seconds).
    load_value: u64,
    /// CNTVCT_EL0 snapshot taken when load_value was written.
    load_counter: u64,
    /// Match register (stub — not wired to interrupts).
    match_value: u32,
    /// Control register: bit 0 = RTC enabled.
    control: u32,
    /// Interrupt mask (stub).
    imsc: u32,
    /// Raw interrupt status (stub).
    ris: u32,
}

impl VirtualPl031 {
    pub fn new() -> Self {
        Self {
            load_value: 0,
            load_counter: read_cntvct(),
            match_value: 0,
            control: 1, // enabled by default (matches QEMU)
            imsc: 0,
            ris: 0,
        }
    }

    /// Current RTC time in seconds.
    fn current_time(&self) -> u64 {
        if self.control & 1 == 0 {
            // RTC disabled — freeze at load_value
            return self.load_value;
        }
        let freq = read_cntfrq();
        if freq == 0 {
            return self.load_value;
        }
        let elapsed_ticks = read_cntvct().wrapping_sub(self.load_counter);
        let elapsed_seconds = elapsed_ticks / freq;
        self.load_value + elapsed_seconds
    }
}

impl MmioDevice for VirtualPl031 {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        if size != 4 {
            return Some(0);
        }

        let value = match offset {
            RTCDR => self.current_time(),
            RTCMR => self.match_value as u64,
            RTCLR => 0, // write-only, reads as 0
            RTCCR => self.control as u64,
            RTCIMSC => self.imsc as u64,
            RTCRIS => self.ris as u64,
            RTCMIS => (self.ris & self.imsc) as u64,
            RTCICR => 0, // write-only

            // PL031 Peripheral ID (required for Linux amba-pl031.c probe)
            PERIPHID0 => 0x31,
            PERIPHID1 => 0x10,
            PERIPHID2 => 0x04,
            PERIPHID3 => 0x00,
            PCELLID0 => 0x0D,
            PCELLID1 => 0xF0,
            PCELLID2 => 0x05,
            PCELLID3 => 0xB1,

            _ => 0,
        };

        Some(value)
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        if size != 4 {
            return false;
        }

        match offset {
            RTCMR => {
                self.match_value = value as u32;
                true
            }
            RTCLR => {
                self.load_value = value & 0xFFFF_FFFF;
                self.load_counter = read_cntvct();
                true
            }
            RTCCR => {
                self.control = (value & 1) as u32;
                true
            }
            RTCIMSC => {
                self.imsc = (value & 1) as u32;
                true
            }
            RTCICR => {
                self.ris &= !(value as u32);
                true
            }
            RTCDR => true, // read-only, ignore writes
            _ => true,     // unknown — accept silently
        }
    }

    fn base_address(&self) -> u64 {
        PL031_BASE
    }

    fn size(&self) -> u64 {
        PL031_SIZE
    }
}
