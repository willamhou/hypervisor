//! Per-vCPU architectural state that must be saved/restored on context switch.
//!
//! This includes GICv3 virtual interface registers, virtual timer state,
//! CPU identity (VMPIDR), and EL1 system registers not saved by exception.S.

use core::arch::asm;

/// Number of GICv3 list registers to save/restore
const NUM_LRS: usize = 4;

/// Per-vCPU architectural state
pub struct VcpuArchState {
    // GICv3 virtual interface
    pub ich_lr: [u64; NUM_LRS],
    pub ich_vmcr: u64,
    pub ich_hcr: u64,

    // Virtual timer
    pub cntv_ctl: u64,
    pub cntv_cval: u64,

    // CPU identity
    pub vmpidr: u64,

    // EL1 system registers (not saved/restored by exception.S context switch)
    pub sctlr_el1: u64,
    pub ttbr0_el1: u64,
    pub ttbr1_el1: u64,
    pub tcr_el1: u64,
    pub mair_el1: u64,
    pub vbar_el1: u64,
    pub cpacr_el1: u64,
    pub contextidr_el1: u64,
    pub tpidr_el1: u64,
    pub tpidrro_el0: u64,
    pub tpidr_el0: u64,
    pub par_el1: u64,
    pub cntkctl_el1: u64,
    pub sp_el1: u64,
    pub elr_el1: u64,
    pub spsr_el1: u64,
    pub afsr0_el1: u64,
    pub afsr1_el1: u64,
    pub esr_el1: u64,
    pub far_el1: u64,
    pub amair_el1: u64,
    pub mdscr_el1: u64,
    pub sp_el0: u64,

    // Pointer Authentication keys (PAC)
    pub apia_key_lo: u64,
    pub apia_key_hi: u64,
    pub apib_key_lo: u64,
    pub apib_key_hi: u64,
    pub apda_key_lo: u64,
    pub apda_key_hi: u64,
    pub apdb_key_lo: u64,
    pub apdb_key_hi: u64,
    pub apga_key_lo: u64,
    pub apga_key_hi: u64,
}

impl VcpuArchState {
    /// Create a new zeroed state
    pub const fn new() -> Self {
        Self {
            ich_lr: [0; NUM_LRS],
            ich_vmcr: 0,
            ich_hcr: 0,
            cntv_ctl: 0,
            cntv_cval: 0,
            vmpidr: 0,
            sctlr_el1: 0,
            ttbr0_el1: 0,
            ttbr1_el1: 0,
            tcr_el1: 0,
            mair_el1: 0,
            vbar_el1: 0,
            cpacr_el1: 0,
            contextidr_el1: 0,
            tpidr_el1: 0,
            tpidrro_el0: 0,
            tpidr_el0: 0,
            par_el1: 0,
            cntkctl_el1: 0,
            sp_el1: 0,
            elr_el1: 0,
            spsr_el1: 0,
            afsr0_el1: 0,
            afsr1_el1: 0,
            esr_el1: 0,
            far_el1: 0,
            amair_el1: 0,
            mdscr_el1: 0,
            sp_el0: 0,
            apia_key_lo: 0,
            apia_key_hi: 0,
            apib_key_lo: 0,
            apib_key_hi: 0,
            apda_key_lo: 0,
            apda_key_hi: 0,
            apdb_key_lo: 0,
            apdb_key_hi: 0,
            apga_key_lo: 0,
            apga_key_hi: 0,
        }
    }

    /// Initialize state for a specific vCPU ID
    ///
    /// Sets VMPIDR based on MPIDR layout (Aff0 = vcpu_id),
    /// and default GIC/timer values.
    pub fn init_for_vcpu(&mut self, vcpu_id: usize) {
        // VMPIDR: use real MPIDR as template, override Aff0 with vcpu_id
        let mpidr: u64;
        unsafe {
            asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nostack, nomem));
        }
        // Clear Aff0 (bits [7:0]) and set to vcpu_id
        self.vmpidr = (mpidr & !0xFF) | (vcpu_id as u64 & 0xFF);

        // Default GIC virtual interface: enable virtual interrupts + TALL1
        // TALL1 traps ICC_SGI1R_EL1 writes (SGI generation) to EL2 for emulation.
        // With En=1, other ICC registers are redirected to virtual ICV_* (not trapped).
        self.ich_hcr = (1 << 13) | 1; // TALL1 | En
                                      // VMCR: VPMR=0xFF (allow all priorities), VENG1=1 (enable Group 1)
        self.ich_vmcr = (0xFF << 24) | (1 << 1);
        self.ich_lr = [0; NUM_LRS];

        // Timer: disabled by default
        self.cntv_ctl = 0;
        self.cntv_cval = 0;
    }

    /// Save all per-vCPU registers from hardware
    pub fn save(&mut self) {
        unsafe {
            // GICv3 List Registers
            asm!("mrs {}, ICH_LR0_EL2", out(reg) self.ich_lr[0], options(nostack, nomem));
            asm!("mrs {}, ICH_LR1_EL2", out(reg) self.ich_lr[1], options(nostack, nomem));
            asm!("mrs {}, ICH_LR2_EL2", out(reg) self.ich_lr[2], options(nostack, nomem));
            asm!("mrs {}, ICH_LR3_EL2", out(reg) self.ich_lr[3], options(nostack, nomem));

            // GICv3 virtual interface control
            let vmcr: u64;
            asm!("mrs {}, ICH_VMCR_EL2", out(reg) vmcr, options(nostack, nomem));
            self.ich_vmcr = vmcr;
            let hcr: u64;
            asm!("mrs {}, ICH_HCR_EL2", out(reg) hcr, options(nostack, nomem));
            self.ich_hcr = hcr;

            // Virtual timer
            asm!("mrs {}, cntv_ctl_el0", out(reg) self.cntv_ctl, options(nostack, nomem));
            asm!("mrs {}, cntv_cval_el0", out(reg) self.cntv_cval, options(nostack, nomem));

            // EL1 system registers
            asm!("mrs {}, sctlr_el1", out(reg) self.sctlr_el1, options(nostack, nomem));
            asm!("mrs {}, ttbr0_el1", out(reg) self.ttbr0_el1, options(nostack, nomem));
            asm!("mrs {}, ttbr1_el1", out(reg) self.ttbr1_el1, options(nostack, nomem));
            asm!("mrs {}, tcr_el1", out(reg) self.tcr_el1, options(nostack, nomem));
            asm!("mrs {}, mair_el1", out(reg) self.mair_el1, options(nostack, nomem));
            asm!("mrs {}, vbar_el1", out(reg) self.vbar_el1, options(nostack, nomem));
            asm!("mrs {}, cpacr_el1", out(reg) self.cpacr_el1, options(nostack, nomem));
            asm!("mrs {}, contextidr_el1", out(reg) self.contextidr_el1, options(nostack, nomem));
            asm!("mrs {}, tpidr_el1", out(reg) self.tpidr_el1, options(nostack, nomem));
            asm!("mrs {}, tpidrro_el0", out(reg) self.tpidrro_el0, options(nostack, nomem));
            asm!("mrs {}, tpidr_el0", out(reg) self.tpidr_el0, options(nostack, nomem));
            asm!("mrs {}, par_el1", out(reg) self.par_el1, options(nostack, nomem));
            asm!("mrs {}, cntkctl_el1", out(reg) self.cntkctl_el1, options(nostack, nomem));
            asm!("mrs {}, sp_el1", out(reg) self.sp_el1, options(nostack, nomem));
            asm!("mrs {}, elr_el1", out(reg) self.elr_el1, options(nostack, nomem));
            asm!("mrs {}, spsr_el1", out(reg) self.spsr_el1, options(nostack, nomem));
            asm!("mrs {}, afsr0_el1", out(reg) self.afsr0_el1, options(nostack, nomem));
            asm!("mrs {}, afsr1_el1", out(reg) self.afsr1_el1, options(nostack, nomem));
            asm!("mrs {}, esr_el1", out(reg) self.esr_el1, options(nostack, nomem));
            asm!("mrs {}, far_el1", out(reg) self.far_el1, options(nostack, nomem));
            asm!("mrs {}, amair_el1", out(reg) self.amair_el1, options(nostack, nomem));
            asm!("mrs {}, mdscr_el1", out(reg) self.mdscr_el1, options(nostack, nomem));
            asm!("mrs {}, sp_el0", out(reg) self.sp_el0, options(nostack, nomem));

            // PAC keys (using system register encodings)
            // APIAKey: S3_0_C2_C1_0/1, APIBKey: S3_0_C2_C1_2/3
            // APDAKey: S3_0_C2_C2_0/1, APDBKey: S3_0_C2_C2_2/3
            // APGAKey: S3_0_C2_C3_0/1
            asm!("mrs {}, S3_0_C2_C1_0", out(reg) self.apia_key_lo, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C1_1", out(reg) self.apia_key_hi, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C1_2", out(reg) self.apib_key_lo, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C1_3", out(reg) self.apib_key_hi, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C2_0", out(reg) self.apda_key_lo, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C2_1", out(reg) self.apda_key_hi, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C2_2", out(reg) self.apdb_key_lo, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C2_3", out(reg) self.apdb_key_hi, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C3_0", out(reg) self.apga_key_lo, options(nostack, nomem));
            asm!("mrs {}, S3_0_C2_C3_1", out(reg) self.apga_key_hi, options(nostack, nomem));
        }
    }

    /// Restore all per-vCPU registers to hardware
    pub fn restore(&self) {
        unsafe {
            // CPU identity - must be set before guest runs
            asm!("msr vmpidr_el2, {}", in(reg) self.vmpidr, options(nostack, nomem));

            // GICv3 List Registers
            asm!("msr ICH_LR0_EL2, {}", in(reg) self.ich_lr[0], options(nostack, nomem));
            asm!("msr ICH_LR1_EL2, {}", in(reg) self.ich_lr[1], options(nostack, nomem));
            asm!("msr ICH_LR2_EL2, {}", in(reg) self.ich_lr[2], options(nostack, nomem));
            asm!("msr ICH_LR3_EL2, {}", in(reg) self.ich_lr[3], options(nostack, nomem));

            // GICv3 virtual interface control
            asm!("msr ICH_VMCR_EL2, {}", in(reg) self.ich_vmcr, options(nostack, nomem));
            asm!("msr ICH_HCR_EL2, {}", in(reg) self.ich_hcr, options(nostack, nomem));

            // Virtual timer
            asm!("msr cntv_ctl_el0, {}", in(reg) self.cntv_ctl, options(nostack, nomem));
            asm!("msr cntv_cval_el0, {}", in(reg) self.cntv_cval, options(nostack, nomem));

            // EL1 system registers
            asm!("msr sctlr_el1, {}", in(reg) self.sctlr_el1, options(nostack, nomem));
            asm!("msr ttbr0_el1, {}", in(reg) self.ttbr0_el1, options(nostack, nomem));
            asm!("msr ttbr1_el1, {}", in(reg) self.ttbr1_el1, options(nostack, nomem));
            asm!("msr tcr_el1, {}", in(reg) self.tcr_el1, options(nostack, nomem));
            asm!("msr mair_el1, {}", in(reg) self.mair_el1, options(nostack, nomem));
            asm!("msr vbar_el1, {}", in(reg) self.vbar_el1, options(nostack, nomem));
            asm!("msr cpacr_el1, {}", in(reg) self.cpacr_el1, options(nostack, nomem));
            asm!("msr contextidr_el1, {}", in(reg) self.contextidr_el1, options(nostack, nomem));
            asm!("msr tpidr_el1, {}", in(reg) self.tpidr_el1, options(nostack, nomem));
            asm!("msr tpidrro_el0, {}", in(reg) self.tpidrro_el0, options(nostack, nomem));
            asm!("msr tpidr_el0, {}", in(reg) self.tpidr_el0, options(nostack, nomem));
            asm!("msr par_el1, {}", in(reg) self.par_el1, options(nostack, nomem));
            asm!("msr cntkctl_el1, {}", in(reg) self.cntkctl_el1, options(nostack, nomem));
            asm!("msr sp_el1, {}", in(reg) self.sp_el1, options(nostack, nomem));
            asm!("msr elr_el1, {}", in(reg) self.elr_el1, options(nostack, nomem));
            asm!("msr spsr_el1, {}", in(reg) self.spsr_el1, options(nostack, nomem));
            asm!("msr afsr0_el1, {}", in(reg) self.afsr0_el1, options(nostack, nomem));
            asm!("msr afsr1_el1, {}", in(reg) self.afsr1_el1, options(nostack, nomem));
            asm!("msr esr_el1, {}", in(reg) self.esr_el1, options(nostack, nomem));
            asm!("msr far_el1, {}", in(reg) self.far_el1, options(nostack, nomem));
            asm!("msr amair_el1, {}", in(reg) self.amair_el1, options(nostack, nomem));
            asm!("msr mdscr_el1, {}", in(reg) self.mdscr_el1, options(nostack, nomem));
            asm!("msr sp_el0, {}", in(reg) self.sp_el0, options(nostack, nomem));

            // PAC keys
            asm!("msr S3_0_C2_C1_0, {}", in(reg) self.apia_key_lo, options(nostack, nomem));
            asm!("msr S3_0_C2_C1_1, {}", in(reg) self.apia_key_hi, options(nostack, nomem));
            asm!("msr S3_0_C2_C1_2, {}", in(reg) self.apib_key_lo, options(nostack, nomem));
            asm!("msr S3_0_C2_C1_3, {}", in(reg) self.apib_key_hi, options(nostack, nomem));
            asm!("msr S3_0_C2_C2_0, {}", in(reg) self.apda_key_lo, options(nostack, nomem));
            asm!("msr S3_0_C2_C2_1, {}", in(reg) self.apda_key_hi, options(nostack, nomem));
            asm!("msr S3_0_C2_C2_2, {}", in(reg) self.apdb_key_lo, options(nostack, nomem));
            asm!("msr S3_0_C2_C2_3, {}", in(reg) self.apdb_key_hi, options(nostack, nomem));
            asm!("msr S3_0_C2_C3_0, {}", in(reg) self.apga_key_lo, options(nostack, nomem));
            asm!("msr S3_0_C2_C3_1, {}", in(reg) self.apga_key_hi, options(nostack, nomem));

            // ISB to ensure all register writes take effect
            asm!("isb", options(nostack, nomem));
        }
    }
}
