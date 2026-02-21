//! SMC forwarding to EL3 (Secure World).
//!
//! From EL2, executing `smc #0` goes directly to EL3 — `HCR_EL2.TSC` only
//! traps EL1 SMC to EL2, not EL2 SMC. This enables transparent forwarding
//! of validated FF-A calls (and other SMCCC calls) to the Secure World.

/// Result of forwarding an SMC to EL3 (x0-x3).
pub struct SmcResult {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
}

/// Result of forwarding an SMC to EL3 with all 8 return registers (x0-x7).
///
/// Used by the SPMC event loop where the full SMCCC return state is needed
/// (e.g., FFA_MSG_SEND_DIRECT_REQ passes data in x4-x7).
#[derive(Debug, Clone, Copy)]
pub struct SmcResult8 {
    pub x0: u64,
    pub x1: u64,
    pub x2: u64,
    pub x3: u64,
    pub x4: u64,
    pub x5: u64,
    pub x6: u64,
    pub x7: u64,
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

/// Forward an SMC call to EL3 (Secure World) from EL2, capturing all 8 return registers.
///
/// Passes x0-x7 as arguments per SMCCC calling convention,
/// returns x0-x7 as results. Used by the SPMC event loop where
/// FF-A calls carry data in x4-x7 (e.g., direct messaging).
///
/// # Safety
///
/// This executes a real SMC instruction at EL2. The caller must ensure
/// the arguments are valid for the target SMC function.
#[inline(never)]
pub fn forward_smc8(
    x0: u64,
    x1: u64,
    x2: u64,
    x3: u64,
    x4: u64,
    x5: u64,
    x6: u64,
    x7: u64,
) -> SmcResult8 {
    let r0: u64;
    let r1: u64;
    let r2: u64;
    let r3: u64;
    let r4: u64;
    let r5: u64;
    let r6: u64;
    let r7: u64;
    unsafe {
        core::arch::asm!(
            "smc #0",
            inout("x0") x0 => r0,
            inout("x1") x1 => r1,
            inout("x2") x2 => r2,
            inout("x3") x3 => r3,
            inout("x4") x4 => r4,
            inout("x5") x5 => r5,
            inout("x6") x6 => r6,
            inout("x7") x7 => r7,
            // x8-x17 may be clobbered by the SMC call per SMCCC
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
    SmcResult8 {
        x0: r0,
        x1: r1,
        x2: r2,
        x3: r3,
        x4: r4,
        x5: r5,
        x6: r6,
        x7: r7,
    }
}

/// Check if a real SPMC is present at EL3.
///
/// Uses PSCI_VERSION as a safe probe first (always handled by QEMU firmware),
/// then sends FFA_VERSION only if EL3 is known to be responsive.
///
/// NOTE: QEMU's simple EL3 firmware crashes on unknown SMCs like FFA_VERSION
/// (it doesn't return SMC_UNKNOWN, it faults). So we only probe FFA_VERSION
/// on platforms where EL3 is known to handle it (e.g., with Hafnium/OP-TEE).
/// For now, we conservatively return false on QEMU.
pub fn probe_spmc() -> bool {
    // First, verify EL3 is alive via PSCI_VERSION (0x84000000).
    // QEMU firmware always handles this, returning 0x00010001 (PSCI 1.1).
    let psci_result = forward_smc(0x8400_0000, 0, 0, 0, 0, 0, 0, 0);
    if psci_result.x0 == 0xFFFF_FFFF_FFFF_FFFF || psci_result.x0 == 0xFFFF_FFFF {
        // EL3 doesn't even handle PSCI — no point probing FFA
        return false;
    }

    // EL3 is alive, but QEMU's simple firmware will crash on FFA_VERSION.
    // Only send FFA_VERSION if we have evidence of a real SPMC (e.g., TF-A + Hafnium).
    // For QEMU virt with default firmware, PSCI_VERSION returns 0x10001 — no SPMC.
    // TODO: Enable FFA_VERSION probe when running on real hardware or with OP-TEE/Hafnium.
    false
}
