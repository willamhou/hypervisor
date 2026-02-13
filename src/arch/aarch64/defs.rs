//! ARM64 Architecture Constants
//!
//! Named constants for ARM64 system register fields, exception classes,
//! GICv3 list register encoding, page table bits, and other architectural
//! definitions. Eliminates magic numbers throughout the codebase.

// ── HCR_EL2 (Hypervisor Configuration Register) ─────────────────────
pub const HCR_VM: u64 = 1 << 0;
pub const HCR_SWIO: u64 = 1 << 1;
pub const HCR_FMO: u64 = 1 << 3;
pub const HCR_IMO: u64 = 1 << 4;
pub const HCR_AMO: u64 = 1 << 5;
pub const HCR_FB: u64 = 1 << 9;
pub const HCR_BSU_INNER: u64 = 1 << 10;
pub const HCR_TWI: u64 = 1 << 13;
pub const HCR_TWE: u64 = 1 << 14;
pub const HCR_RW: u64 = 1 << 31;
pub const HCR_TEA: u64 = 1 << 37;  // Trap External Aborts to EL2
pub const HCR_APK: u64 = 1 << 40;
pub const HCR_API: u64 = 1 << 41;

// ── ESR_EL2 (Exception Syndrome Register) ────────────────────────────
pub const ESR_EC_SHIFT: u32 = 26;
pub const ESR_EC_MASK: u64 = 0x3F;
pub const ESR_ISS_MASK: u64 = 0x1FFFFFF;
pub const ESR_HVC_IMM_MASK: u64 = 0xFFFF;

// ── Exception Class (EC) values ──────────────────────────────────────
pub const EC_UNKNOWN: u64 = 0x00;
pub const EC_WFI_WFE: u64 = 0x01;
pub const EC_TRAPPED_SIMD_FP: u64 = 0x07;
pub const EC_TRAPPED_SVE: u64 = 0x09;
pub const EC_HVC64: u64 = 0x16;
pub const EC_MSR_MRS: u64 = 0x18;
pub const EC_SVE_TRAP: u64 = 0x19;
pub const EC_IABT_LOWER: u64 = 0x20;
pub const EC_IABT_SAME: u64 = 0x21;
pub const EC_DABT_LOWER: u64 = 0x24;
pub const EC_DABT_SAME: u64 = 0x25;

// ── SPSR_EL2 defaults ────────────────────────────────────────────────
pub const SPSR_EL1H_DAIF_MASKED: u64 = 0x3C5;
pub const SPSR_EL1H: u64 = 0b0101;

// ── CPTR_EL2 bits ────────────────────────────────────────────────────
pub const CPTR_TZ: u64 = 1 << 8;
pub const CPTR_TFP: u64 = 1 << 10;
pub const CPTR_TSM: u64 = 1 << 12;
pub const CPTR_TCPAC: u64 = 1 << 20;

// ── ICH_HCR_EL2 (Hypervisor Control Register for Virtual GIC) ───────
pub const ICH_HCR_EN: u64 = 1 << 0;
pub const ICH_HCR_TALL1: u64 = 1 << 13;

// ── ICC register bits ────────────────────────────────────────────────
pub const ICC_SRE_SRE: u32 = 1 << 0;
pub const ICC_SRE_ENABLE: u32 = 1 << 3;
pub const ICC_CTLR_EOIMODE: u32 = 1 << 1;
pub const ICC_PMR_ALLOW_ALL: u32 = 0xFF;

// ── GICv3 List Register field positions ──────────────────────────────
pub const LR_STATE_SHIFT: u32 = 62;
pub const LR_STATE_MASK: u64 = 0x3;
pub const LR_HW_BIT: u64 = 1 << 61;
pub const LR_GROUP1_BIT: u64 = 1 << 60;
pub const LR_PRIORITY_SHIFT: u32 = 48;
pub const LR_PINTID_SHIFT: u32 = 32;
pub const LR_PINTID_MASK: u64 = 0x3FF;
pub const LR_VINTID_MASK: u64 = 0xFFFF_FFFF;
pub const VTR_LISTREGS_MASK: u32 = 0x1F;
pub const GIC_SPURIOUS_INTID: u32 = 1020;

// ── Interrupt priority ───────────────────────────────────────────────
pub const IRQ_DEFAULT_PRIORITY: u8 = 0xA0;

// ── VTCR_EL2 fields ─────────────────────────────────────────────────
pub const VTCR_T0SZ_48BIT: u64 = 16;
pub const VTCR_SL0_LEVEL0: u64 = 2 << 6;
pub const VTCR_IRGN0_WB: u64 = 0b01 << 8;
pub const VTCR_ORGN0_WB: u64 = 0b01 << 10;
pub const VTCR_SH0_INNER: u64 = 0b11 << 12;
pub const VTCR_TG0_4KB: u64 = 0b00 << 14;
pub const VTCR_PS_48BIT: u64 = 0b101 << 16;

// ── CNTHCTL_EL2 bits ─────────────────────────────────────────────────
pub const CNTHCTL_EL1PCTEN: u64 = 1 << 0;
pub const CNTHCTL_EL1PCEN: u64 = 1 << 1;

// ── Page table constants ─────────────────────────────────────────────
pub const PTE_VALID: u64 = 1 << 0;
pub const PTE_TABLE: u64 = 1 << 1;
pub const PTE_ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
pub const PAGE_OFFSET_MASK: u64 = 0xFFF;
pub const PT_INDEX_MASK: u64 = 0x1FF;
pub const BLOCK_SIZE_2MB: u64 = 2 * 1024 * 1024;
pub const BLOCK_MASK_2MB: u64 = BLOCK_SIZE_2MB - 1;
pub const PAGE_SIZE_4KB: u64 = 4096;
pub const PAGE_MASK_4KB: u64 = PAGE_SIZE_4KB - 1;

// ── Preemptive scheduling ────────────────────────────────────────────
// Preemption is now handled by CNTHP timer (INTID 26) armed before each
// vcpu.run(). See timer::arm_preemption_timer(). This ensures preemption
// works even when the guest virtual timer (INTID 27) is masked.

// ── ARM64 instruction width ──────────────────────────────────────────
pub const AARCH64_INSN_SIZE: u64 = 4;
