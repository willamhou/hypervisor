//! Secure Stage-2 page tables for SP isolation at S-EL2.
//!
//! Uses VSTTBR_EL2/VSTCR_EL2 (not VTTBR/VTCR) to provide
//! address translation for Secure Partitions at S-EL1.

use crate::arch::aarch64::defs::*;

/// Secure Stage-2 configuration (VSTTBR_EL2 + VSTCR_EL2).
pub struct SecureStage2Config {
    pub vsttbr: u64,
    pub vstcr: u64,
}

impl SecureStage2Config {
    /// Create configuration from a page table base address.
    pub fn new(page_table_addr: u64) -> Self {
        let vstcr = VTCR_T0SZ_48BIT
            | VTCR_SL0_LEVEL0
            | VTCR_IRGN0_WB
            | VTCR_ORGN0_WB
            | VTCR_SH0_INNER
            | VTCR_TG0_4KB
            | VTCR_PS_48BIT;

        let vsttbr = page_table_addr & 0x0000_FFFF_FFFF_FFFE;

        Self { vsttbr, vstcr }
    }

    /// Create from a previously stored VSTTBR value (for reinstalling).
    pub fn new_from_vsttbr(vsttbr: u64) -> Self {
        let vstcr = VTCR_T0SZ_48BIT
            | VTCR_SL0_LEVEL0
            | VTCR_IRGN0_WB
            | VTCR_ORGN0_WB
            | VTCR_SH0_INNER
            | VTCR_TG0_4KB
            | VTCR_PS_48BIT;
        Self { vsttbr, vstcr }
    }

    /// Install Secure Stage-2 to hardware registers.
    #[cfg(feature = "sel2")]
    pub fn install(&self) {
        unsafe {
            core::arch::asm!(
                "msr s3_4_c2_c6_2, {vstcr}", // VSTCR_EL2
                "isb",
                vstcr = in(reg) self.vstcr,
                options(nostack, nomem),
            );
            core::arch::asm!(
                "msr s3_4_c2_c6_0, {vsttbr}", // VSTTBR_EL2
                "isb",
                vsttbr = in(reg) self.vsttbr,
                options(nostack, nomem),
            );
        }
    }
}

/// Build Secure Stage-2 page tables for an SP.
///
/// Identity-maps the SP's code/data region and UART for debug output.
/// Returns a `DynamicIdentityMapper` (caller reads `l0_addr()` for VSTTBR).
#[cfg(feature = "sel2")]
pub fn build_sp_stage2(
    sp_base: u64,
    sp_size: u64,
) -> Result<crate::arch::aarch64::mm::mmu::DynamicIdentityMapper, &'static str> {
    use crate::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute};

    let mut mapper = DynamicIdentityMapper::new();

    // Map SP code/data region (Normal memory, identity-mapped)
    mapper.map_region(sp_base, sp_size, MemoryAttribute::Normal)?;

    // Map UART for SP debug output (Device memory, 2MB block containing 0x09000000)
    mapper.map_region(
        crate::platform::SP_UART_BASE,
        BLOCK_SIZE_2MB,
        MemoryAttribute::Device,
    )?;

    Ok(mapper)
}
