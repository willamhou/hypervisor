//! SMC forwarding to EL3 (Secure World).
//!
//! From EL2, executing `smc #0` goes directly to EL3 — `HCR_EL2.TSC` only
//! traps EL1 SMC to EL2, not EL2 SMC. This enables transparent forwarding
//! of validated FF-A calls (and other SMCCC calls) to the Secure World.

/// Result of forwarding an SMC to EL3.
pub struct SmcResult {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
}

/// Forward an SMC call to EL3 (Secure World) from EL2.
///
/// Passes x0-x7 as arguments per SMCCC calling convention,
/// returns x0-x3 as results.
///
/// # Safety
///
/// This executes a real SMC instruction at EL2. The caller must ensure
/// the arguments are valid for the target SMC function.
#[inline(never)]
pub fn forward_smc(
    x0: u64,
    x1: u64,
    x2: u64,
    x3: u64,
    x4: u64,
    x5: u64,
    x6: u64,
    x7: u64,
) -> SmcResult {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    let r3: u64;
    unsafe {
        core::arch::asm!(
            "smc #0",
            inout("x0") x0 => r0,
            inout("x1") x1 => r1,
            inout("x2") x2 => r2,
            inout("x3") x3 => r3,
            in("x4") x4,
            in("x5") x5,
            in("x6") x6,
            in("x7") x7,
            // x4-x17 may be clobbered by the SMC call per SMCCC
            lateout("x4") _,
            lateout("x5") _,
            lateout("x6") _,
            lateout("x7") _,
            lateout("x8") _,
            lateout("x9") _,
            lateout("x10") _,
            lateout("x11") _,
            lateout("x12") _,
            lateout("x13") _,
            lateout("x14") _,
            lateout("x15") _,
            lateout("x16") _,
            lateout("x17") _,
            options(nomem, nostack),
        );
    }
    SmcResult {
        x0: r0,
        x1: r1,
        x2: r2,
        x3: r3,
    }
}

/// Check if a real SPMC is present by sending FFA_VERSION to EL3.
///
/// Returns true if EL3 responds with a valid FF-A version (not -1 / SMC_UNKNOWN).
pub fn probe_spmc() -> bool {
    let result = forward_smc(
        crate::ffa::FFA_VERSION,
        crate::ffa::FFA_VERSION_1_1 as u64,
        0,
        0,
        0,
        0,
        0,
        0,
    );
    // SMC_UNKNOWN returns 0xFFFFFFFF_FFFFFFFF (u64) or 0xFFFFFFFF (u32 sign-extended)
    // A valid FFA_VERSION response is a small positive number like 0x00010001
    result.x0 != 0xFFFF_FFFF_FFFF_FFFF && result.x0 != 0xFFFF_FFFF
}
