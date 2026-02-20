/// Virtual GIC Redistributor (GICR)
///
/// Emulates GICv3 redistributors for all vCPUs. Each vCPU has a 128KB region:
///   - RD frame   (0x00000..0x0FFFF): CTLR, TYPER, WAKER, PIDR2
///   - SGI frame  (0x10000..0x1FFFF): IGROUPR0, ISENABLER0, IPRIORITYR, etc.
///
/// Address routing: base = 0x080A_0000, vcpu_id = offset / 0x20000.
use crate::devices::MmioDevice;

/// Size per redistributor (RD + SGI frames)
const GICR_PER_CPU: u64 = 0x20000; // 128KB
/// Maximum vCPUs supported (compile-time capacity)
const MAX_VCPUS: usize = crate::platform::MAX_SMP_CPUS;

// ── RD frame register offsets ────────────────────────────────────────
const GICR_CTLR: u64 = 0x0000;
const GICR_IIDR: u64 = 0x0004;
const GICR_TYPER: u64 = 0x0008; // 64-bit
const GICR_STATUSR: u64 = 0x0010;
const GICR_WAKER: u64 = 0x0014;
const GICR_PIDR2: u64 = 0xFFE8;

// ── SGI frame register offsets (relative to SGI base = RD + 0x10000) ─
const GICR_IGROUPR0: u64 = 0x0080;
const GICR_ISENABLER0: u64 = 0x0100;
const GICR_ICENABLER0: u64 = 0x0180;
const GICR_ISPENDR0: u64 = 0x0200;
const GICR_ICPENDR0: u64 = 0x0280;
const GICR_ISACTIVER0: u64 = 0x0300;
const GICR_ICACTIVER0: u64 = 0x0380;
const GICR_IPRIORITYR_BASE: u64 = 0x0400;
const GICR_IPRIORITYR_END: u64 = 0x041C;
const GICR_ICFGR0: u64 = 0x0C00;
const GICR_ICFGR1: u64 = 0x0C04;

/// Per-vCPU GICR state (SGIs 0-15 + PPIs 16-31 = INTIDs 0-31)
#[derive(Copy, Clone)]
struct GicrState {
    // RD frame
    ctlr: u32,
    waker: u32,

    // SGI frame
    igroupr0: u32,
    isenabler0: u32,
    ispendr0: u32,
    isactiver0: u32,
    ipriorityr: [u32; 8], // 8 regs × 4 INTIDs = 32 INTIDs
    icfgr: [u32; 2],      // ICFGR0 (SGIs, RO edge) + ICFGR1 (PPIs)
}

impl GicrState {
    const fn new() -> Self {
        Self {
            ctlr: 0,
            waker: 0x06, // ProcessorSleep=1, ChildrenAsleep=1 at reset
            igroupr0: 0,
            isenabler0: 0,
            ispendr0: 0,
            isactiver0: 0,
            ipriorityr: [0; 8],
            icfgr: [
                0xAAAA_AAAA, // ICFGR0: SGIs are edge-triggered (RO)
                0x0000_0000, // ICFGR1: PPIs default level-triggered
            ],
        }
    }
}

/// Virtual GIC Redistributor covering all vCPUs
pub struct VirtualGicr {
    state: [GicrState; MAX_VCPUS],
    num_vcpus: usize,
}

impl VirtualGicr {
    /// Create a new GICR emulator for `n` vCPUs.
    ///
    /// # Panics
    /// Panics if `num_vcpus > MAX_VCPUS`.
    pub fn new(num_vcpus: usize) -> Self {
        assert!(num_vcpus <= MAX_VCPUS, "num_vcpus exceeds MAX_VCPUS");
        Self {
            state: [GicrState::new(); MAX_VCPUS],
            num_vcpus,
        }
    }

    /// Build GICR_TYPER value for a given vCPU
    ///
    /// GICR_TYPER layout (GICv3 spec):
    ///   [63:32] Affinity_Value (Aff3[63:56], Aff2[55:48], Aff1[47:40], Aff0[39:32])
    ///   [23:8]  Processor_Number
    ///   [4]     Last (1 = last redistributor in this series)
    fn typer_value(&self, vcpu_id: usize) -> u64 {
        let aff0 = (vcpu_id as u64) << 32; // Aff0 at bits [39:32]
        let proc_num = (vcpu_id as u64) << 8; // Processor_Number at bits [23:8]
        let last = if vcpu_id == self.num_vcpus - 1 {
            1u64 << 4
        } else {
            0
        };
        aff0 | proc_num | last
    }

    /// Decode offset into (vcpu_id, is_sgi_frame, frame_offset)
    fn decode_offset(&self, offset: u64) -> Option<(usize, bool, u64)> {
        let vcpu_id = (offset / GICR_PER_CPU) as usize;
        if vcpu_id >= self.num_vcpus {
            return None;
        }
        let within = offset % GICR_PER_CPU;
        if within < 0x10000 {
            Some((vcpu_id, false, within)) // RD frame
        } else {
            Some((vcpu_id, true, within - 0x10000)) // SGI frame
        }
    }

    /// Read from RD frame
    fn read_rd(&self, vcpu_id: usize, offset: u64, size: u8) -> Option<u64> {
        let st = &self.state[vcpu_id];
        match offset {
            GICR_CTLR => Some(st.ctlr as u64),
            GICR_IIDR => Some(0x0000_043B), // ARM implementer
            GICR_TYPER if size == 8 => Some(self.typer_value(vcpu_id)),
            GICR_TYPER if size == 4 => Some(self.typer_value(vcpu_id) & 0xFFFF_FFFF),
            0x000C if size == 4 => Some(self.typer_value(vcpu_id) >> 32), // TYPER high
            GICR_STATUSR => Some(0),
            GICR_WAKER => Some(st.waker as u64),
            GICR_PIDR2 => Some(0x30), // GICv3
            _ => Some(0),             // RAZ for unimplemented
        }
    }

    /// Write to RD frame
    fn write_rd(&mut self, vcpu_id: usize, offset: u64, value: u64, _size: u8) {
        let st = &mut self.state[vcpu_id];
        match offset {
            GICR_CTLR => st.ctlr = value as u32,
            GICR_WAKER => {
                // Guest can write ProcessorSleep (bit 1). ChildrenAsleep (bit 2) is RO.
                let sleep = (value as u32) & (1 << 1);
                if sleep == 0 {
                    // Guest clearing ProcessorSleep → ChildrenAsleep clears too
                    st.waker = 0;
                } else {
                    st.waker = (1 << 1) | (1 << 2);
                }
            }
            _ => {} // WI for unimplemented
        }
    }

    /// Read from SGI frame
    fn read_sgi(&self, vcpu_id: usize, offset: u64, _size: u8) -> Option<u64> {
        let st = &self.state[vcpu_id];
        match offset {
            GICR_IGROUPR0 => Some(st.igroupr0 as u64),
            GICR_ISENABLER0 => Some(st.isenabler0 as u64),
            GICR_ICENABLER0 => Some(st.isenabler0 as u64), // reads return enable state
            GICR_ISPENDR0 => Some(st.ispendr0 as u64),
            GICR_ICPENDR0 => Some(st.ispendr0 as u64),
            GICR_ISACTIVER0 => Some(st.isactiver0 as u64),
            GICR_ICACTIVER0 => Some(st.isactiver0 as u64),
            GICR_IPRIORITYR_BASE..=GICR_IPRIORITYR_END => {
                let idx = ((offset - GICR_IPRIORITYR_BASE) / 4) as usize;
                if idx < 8 {
                    Some(st.ipriorityr[idx] as u64)
                } else {
                    Some(0)
                }
            }
            GICR_ICFGR0 => Some(st.icfgr[0] as u64),
            GICR_ICFGR1 => Some(st.icfgr[1] as u64),
            _ => Some(0),
        }
    }

    /// Write to SGI frame
    fn write_sgi(&mut self, vcpu_id: usize, offset: u64, value: u64, _size: u8) {
        let val = value as u32;
        let st = &mut self.state[vcpu_id];
        match offset {
            GICR_IGROUPR0 => st.igroupr0 = val,
            GICR_ISENABLER0 => st.isenabler0 |= val, // write-1-to-set
            GICR_ICENABLER0 => st.isenabler0 &= !val, // write-1-to-clear
            GICR_ISPENDR0 => st.ispendr0 |= val,
            GICR_ICPENDR0 => st.ispendr0 &= !val,
            GICR_ISACTIVER0 => st.isactiver0 |= val,
            GICR_ICACTIVER0 => st.isactiver0 &= !val,
            GICR_IPRIORITYR_BASE..=GICR_IPRIORITYR_END => {
                let idx = ((offset - GICR_IPRIORITYR_BASE) / 4) as usize;
                if idx < 8 {
                    st.ipriorityr[idx] = val;
                }
            }
            GICR_ICFGR0 => {} // SGI config is RO (always edge-triggered)
            GICR_ICFGR1 => st.icfgr[1] = val,
            _ => {} // WI
        }
    }
}

impl MmioDevice for VirtualGicr {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        let (vcpu_id, is_sgi, frame_off) = self.decode_offset(offset)?;
        if is_sgi {
            self.read_sgi(vcpu_id, frame_off, size)
        } else {
            self.read_rd(vcpu_id, frame_off, size)
        }
    }

    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
        if let Some((vcpu_id, is_sgi, frame_off)) = self.decode_offset(offset) {
            if is_sgi {
                self.write_sgi(vcpu_id, frame_off, value, size);
            } else {
                self.write_rd(vcpu_id, frame_off, value, size);
            }
            true
        } else {
            true // WI for out-of-range
        }
    }

    fn base_address(&self) -> u64 {
        crate::dtb::platform_info().gicr_base
    }

    fn size(&self) -> u64 {
        GICR_PER_CPU * self.num_vcpus as u64
    }
}
