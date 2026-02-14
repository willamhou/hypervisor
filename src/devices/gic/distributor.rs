/// Virtual GIC Distributor (GICD)
///
/// Trap-and-emulate for the GICv3 distributor with write-through to physical
/// hardware. Guest GICD accesses are trapped via Stage-2 Data Abort, reads
/// are served from shadow state (with corrections like ARE_NS read-as-one),
/// and writes are forwarded to both shadow state and the physical GICD.
///
/// Write-through is required because the physical GIC must stay in sync with
/// the guest's configuration (EnableGrp1NS, ISENABLER, IROUTER, etc.) for
/// physical interrupt forwarding to work correctly.

use crate::devices::MmioDevice;

/// GICD base address
const GICD_BASE: u64 = 0x08000000;
const GICD_SIZE: u64 = 0x10000;

/// GICD_CTLR bit definitions
const GICD_CTLR_ARE_NS: u32 = 1 << 4;  // Affinity Routing Enable, Non-Secure

/// GICD register offsets
const GICD_CTLR: u64 = 0x000;
const GICD_TYPER: u64 = 0x004;
const GICD_IIDR: u64 = 0x008;
// IGROUPR: 0x080..0x0FC (32 regs, 1 bit per interrupt)
const GICD_IGROUPR_BASE: u64 = 0x080;
const GICD_IGROUPR_END: u64 = 0x0FC;
// ISENABLER: 0x100..0x17C
const GICD_ISENABLER_BASE: u64 = 0x100;
const GICD_ISENABLER_END: u64 = 0x17C;
// ICENABLER: 0x180..0x1FC
const GICD_ICENABLER_BASE: u64 = 0x180;
const GICD_ICENABLER_END: u64 = 0x1FC;
// ISPENDR: 0x200..0x27C
const GICD_ISPENDR_BASE: u64 = 0x200;
const GICD_ISPENDR_END: u64 = 0x27C;
// ICPENDR: 0x280..0x2FC
const GICD_ICPENDR_BASE: u64 = 0x280;
const GICD_ICPENDR_END: u64 = 0x2FC;
// ISACTIVER: 0x300..0x37C
const GICD_ISACTIVER_BASE: u64 = 0x300;
const GICD_ISACTIVER_END: u64 = 0x37C;
// ICACTIVER: 0x380..0x3FC
const GICD_ICACTIVER_BASE: u64 = 0x380;
const GICD_ICACTIVER_END: u64 = 0x3FC;
// IPRIORITYR: 0x400..0x7FC (256 regs, 4 bytes per reg, 1 byte per interrupt)
const GICD_IPRIORITYR_BASE: u64 = 0x400;
const GICD_IPRIORITYR_END: u64 = 0x7FC;
// ICFGR: 0xC00..0xC3C (16 regs for SPIs, 2 bits per interrupt)
const GICD_ICFGR_BASE: u64 = 0xC00;
const GICD_ICFGR_END: u64 = 0xCFC;
// IROUTER: 0x6100..0x7FD8 (64-bit per SPI, SPIs 32-1019)
const GICD_IROUTER_BASE: u64 = 0x6100;
const GICD_IROUTER_END: u64 = 0x7FD8;
// PIDR2: 0xFFE8 (Peripheral ID, reports GIC version)
const GICD_PIDR2: u64 = 0xFFE8;

/// Virtual GICD device
pub struct VirtualGicd {
    /// Distributor control register
    ctlr: u32,
    /// Interrupt enable bits (1024 interrupts, 32 regs of 32 bits)
    enabled: [u32; 32],
    /// Interrupt group assignment (1 bit per interrupt)
    igroupr: [u32; 32],
    /// Interrupt priority (1 byte per interrupt, packed 4 per u32)
    ipriorityr: [u32; 256],
    /// Interrupt configuration (2 bits per interrupt)
    icfgr: [u32; 64],
    /// Pending state
    ispendr: [u32; 32],
    /// Active state
    isactiver: [u32; 32],
    /// SPI routing (64-bit affinity per SPI 32-1019)
    irouter: [u64; 988],
    /// Number of online vCPUs (for TYPER.CPUNumber)
    num_cpus: u32,
}

impl VirtualGicd {
    /// Create a new virtual GICD
    pub fn new() -> Self {
        Self {
            ctlr: 0,
            enabled: [0; 32],
            igroupr: [0; 32],
            ipriorityr: [0; 256],
            icfgr: [0; 64],
            ispendr: [0; 32],
            isactiver: [0; 32],
            irouter: [0; 988],
            num_cpus: crate::platform::SMP_CPUS as u32,
        }
    }

    /// Set the number of online vCPUs (affects GICD_TYPER)
    pub fn set_num_cpus(&mut self, n: u32) {
        self.num_cpus = n;
    }

    /// Look up the target vCPU for an SPI via IROUTER.
    /// Returns the Aff0 field (bits [7:0]) which we use as vCPU ID.
    /// Returns 0 for SGIs/PPIs (INTIDs < 32) or out-of-range INTIDs.
    pub fn route_spi(&self, intid: u32) -> usize {
        if intid < 32 || intid >= 1020 {
            return 0;
        }
        let idx = (intid - 32) as usize;
        // Aff0 (bits [7:0]) = vCPU ID in our simple affinity model
        (self.irouter[idx] & 0xFF) as usize
    }

    /// Handle a 64-bit IROUTER read (used for 8-byte accesses)
    fn read_irouter(&self, offset: u64) -> Option<u64> {
        let byte_off = offset - GICD_IROUTER_BASE;
        // IROUTER registers are 8-byte aligned
        if byte_off & 0x7 != 0 {
            return Some(0);
        }
        let idx = (byte_off / 8) as usize;
        if idx < self.irouter.len() {
            Some(self.irouter[idx])
        } else {
            Some(0)
        }
    }

    /// Handle a 64-bit IROUTER write
    fn write_irouter(&mut self, offset: u64, value: u64) {
        let byte_off = offset - GICD_IROUTER_BASE;
        if byte_off & 0x7 != 0 {
            return;
        }
        let idx = (byte_off / 8) as usize;
        if idx < self.irouter.len() {
            self.irouter[idx] = value;
        }
    }
}

impl MmioDevice for VirtualGicd {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        // IROUTER registers are 64-bit; all others are 32-bit.
        // Linux may also do 32-bit reads of IROUTER low/high halves.
        match offset {
            GICD_IROUTER_BASE..=GICD_IROUTER_END if size == 8 => {
                return self.read_irouter(offset);
            }
            GICD_IROUTER_BASE..=GICD_IROUTER_END if size == 4 => {
                // 32-bit access to lower or upper half of a 64-bit IROUTER
                let aligned = offset & !0x7;
                let full = self.read_irouter(aligned).unwrap_or(0);
                if offset & 0x4 == 0 {
                    return Some(full & 0xFFFF_FFFF);
                } else {
                    return Some(full >> 32);
                }
            }
            _ => {}
        }

        if size != 4 {
            return Some(0);
        }

        match offset {
            GICD_CTLR => {
                // ARE_NS (bit 4) is read-as-one (affinity routing always enabled).
                // RWP (bit 31) always reads 0 (writes are instant in emulation).
                let val = self.ctlr | GICD_CTLR_ARE_NS;
                Some(val as u64)
            }

            GICD_TYPER => {
                // ITLinesNumber[4:0] = 31 → (31+1)*32 = 1024 interrupts
                // CPUNumber[7:5] = (num_cpus - 1)
                // SecurityExtn[10] = 0
                // No1N[25] = 1, A3V[24] = 1, IDbits[23:19] = 9 (10 bits, max 1024)
                // MBIS[16] = 0, RSS[26] = 0
                let cpu_num = (self.num_cpus.saturating_sub(1) & 0x7) << 5;
                Some((31 | cpu_num | (1 << 24) | (1 << 25) | (9 << 19)) as u64)
            }

            GICD_IIDR => {
                // Implementer: ARM (0x43B), revision 0, variant 0, product 0
                Some(0x0000_043B)
            }

            GICD_IGROUPR_BASE..=GICD_IGROUPR_END => {
                let reg = ((offset - GICD_IGROUPR_BASE) / 4) as usize;
                if reg < 32 { Some(self.igroupr[reg] as u64) } else { Some(0) }
            }

            GICD_ISENABLER_BASE..=GICD_ISENABLER_END => {
                let reg = ((offset - GICD_ISENABLER_BASE) / 4) as usize;
                if reg < 32 { Some(self.enabled[reg] as u64) } else { Some(0) }
            }

            GICD_ICENABLER_BASE..=GICD_ICENABLER_END => {
                let reg = ((offset - GICD_ICENABLER_BASE) / 4) as usize;
                if reg < 32 { Some(self.enabled[reg] as u64) } else { Some(0) }
            }

            GICD_ISPENDR_BASE..=GICD_ISPENDR_END => {
                let reg = ((offset - GICD_ISPENDR_BASE) / 4) as usize;
                if reg < 32 { Some(self.ispendr[reg] as u64) } else { Some(0) }
            }

            GICD_ICPENDR_BASE..=GICD_ICPENDR_END => {
                let reg = ((offset - GICD_ICPENDR_BASE) / 4) as usize;
                if reg < 32 { Some(self.ispendr[reg] as u64) } else { Some(0) }
            }

            GICD_ISACTIVER_BASE..=GICD_ISACTIVER_END => {
                let reg = ((offset - GICD_ISACTIVER_BASE) / 4) as usize;
                if reg < 32 { Some(self.isactiver[reg] as u64) } else { Some(0) }
            }

            GICD_ICACTIVER_BASE..=GICD_ICACTIVER_END => {
                let reg = ((offset - GICD_ICACTIVER_BASE) / 4) as usize;
                if reg < 32 { Some(self.isactiver[reg] as u64) } else { Some(0) }
            }

            GICD_IPRIORITYR_BASE..=GICD_IPRIORITYR_END => {
                let reg = ((offset - GICD_IPRIORITYR_BASE) / 4) as usize;
                if reg < 256 { Some(self.ipriorityr[reg] as u64) } else { Some(0) }
            }

            GICD_ICFGR_BASE..=GICD_ICFGR_END => {
                let reg = ((offset - GICD_ICFGR_BASE) / 4) as usize;
                if reg < 64 { Some(self.icfgr[reg] as u64) } else { Some(0) }
            }

            GICD_PIDR2 => {
                // ArchRev[7:4] = 0x3 → GICv3
                Some(0x30)
            }

            _ => Some(0),
        }
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        // Write-through to physical GICD at EL2 (bypasses Stage-2).
        // Skip read-only registers; force ARE_NS on CTLR writes.
        let forward = !matches!(offset, GICD_TYPER | GICD_IIDR | GICD_PIDR2);
        if forward {
            let fwd_value = if offset == GICD_CTLR {
                value | GICD_CTLR_ARE_NS as u64 // enforce affinity routing
            } else {
                value
            };
            // SAFETY: GICD_BASE is valid MMIO at EL2 (bypasses Stage-2).
            // GIC registers are naturally aligned; single-threaded access.
            unsafe {
                let phys = GICD_BASE + offset;
                match size {
                    1 => core::ptr::write_volatile(phys as *mut u8, fwd_value as u8),
                    2 => core::ptr::write_volatile(phys as *mut u16, fwd_value as u16),
                    4 => core::ptr::write_volatile(phys as *mut u32, fwd_value as u32),
                    8 => core::ptr::write_volatile(phys as *mut u64, fwd_value),
                    _ => {}
                }
            }
        }

        // IROUTER: 64-bit or split 32-bit writes
        match offset {
            GICD_IROUTER_BASE..=GICD_IROUTER_END if size == 8 => {
                self.write_irouter(offset, value);
                return true;
            }
            GICD_IROUTER_BASE..=GICD_IROUTER_END if size == 4 => {
                let aligned = offset & !0x7;
                let old = self.read_irouter(aligned).unwrap_or(0);
                let val32 = value & 0xFFFF_FFFF;
                let new = if offset & 0x4 == 0 {
                    (old & 0xFFFF_FFFF_0000_0000) | val32
                } else {
                    (old & 0x0000_0000_FFFF_FFFF) | (val32 << 32)
                };
                self.write_irouter(aligned, new);
                return true;
            }
            _ => {}
        }

        if size != 4 {
            return true; // Silently accept non-32-bit writes
        }

        let val = (value & 0xFFFF_FFFF) as u32;

        match offset {
            GICD_CTLR => {
                self.ctlr = val | GICD_CTLR_ARE_NS;
                true
            }

            GICD_IGROUPR_BASE..=GICD_IGROUPR_END => {
                let reg = ((offset - GICD_IGROUPR_BASE) / 4) as usize;
                if reg < 32 { self.igroupr[reg] = val; }
                true
            }

            GICD_ISENABLER_BASE..=GICD_ISENABLER_END => {
                let reg = ((offset - GICD_ISENABLER_BASE) / 4) as usize;
                if reg < 32 { self.enabled[reg] |= val; }
                true
            }

            GICD_ICENABLER_BASE..=GICD_ICENABLER_END => {
                let reg = ((offset - GICD_ICENABLER_BASE) / 4) as usize;
                if reg < 32 { self.enabled[reg] &= !val; }
                true
            }

            GICD_ISPENDR_BASE..=GICD_ISPENDR_END => {
                let reg = ((offset - GICD_ISPENDR_BASE) / 4) as usize;
                if reg < 32 { self.ispendr[reg] |= val; }
                true
            }

            GICD_ICPENDR_BASE..=GICD_ICPENDR_END => {
                let reg = ((offset - GICD_ICPENDR_BASE) / 4) as usize;
                if reg < 32 { self.ispendr[reg] &= !val; }
                true
            }

            GICD_ISACTIVER_BASE..=GICD_ISACTIVER_END => {
                let reg = ((offset - GICD_ISACTIVER_BASE) / 4) as usize;
                if reg < 32 { self.isactiver[reg] |= val; }
                true
            }

            GICD_ICACTIVER_BASE..=GICD_ICACTIVER_END => {
                let reg = ((offset - GICD_ICACTIVER_BASE) / 4) as usize;
                if reg < 32 { self.isactiver[reg] &= !val; }
                true
            }

            GICD_IPRIORITYR_BASE..=GICD_IPRIORITYR_END => {
                let reg = ((offset - GICD_IPRIORITYR_BASE) / 4) as usize;
                if reg < 256 { self.ipriorityr[reg] = val; }
                true
            }

            GICD_ICFGR_BASE..=GICD_ICFGR_END => {
                let reg = ((offset - GICD_ICFGR_BASE) / 4) as usize;
                if reg < 64 { self.icfgr[reg] = val; }
                true
            }

            _ => true, // Silently accept writes to unimplemented registers
        }
    }

    fn base_address(&self) -> u64 {
        GICD_BASE
    }

    fn size(&self) -> u64 {
        GICD_SIZE
    }
}
