//! SPMC manifest parser (TOS_FW_CONFIG DTB).
//! FF-A Core Manifest v1.0 (DEN0077A).

#[cfg(feature = "sel2")]
use crate::ffa::smc_forward;

/// Parsed SPMC manifest properties.
pub struct SpMcManifest {
    pub spmc_id: u16,
    pub maj_ver: u16,
    pub min_ver: u16,
}

static mut MANIFEST: Option<SpMcManifest> = None;

/// Parse TOS_FW_CONFIG DTB passed in x0 by SPMD.
///
/// For minimal Sprint 4.3 scope, we use defaults if the manifest address
/// cannot be parsed. Full FDT parsing will be enabled once secure memory
/// access is validated.
pub fn init(manifest_addr: usize) {
    if manifest_addr == 0 {
        crate::uart_puts(b"[SPMC] WARNING: manifest addr=0, using defaults\n");
        set_defaults();
        return;
    }

    // Try to read FDT magic to verify address is accessible.
    // FDT magic in big-endian: 0xd00dfeed → little-endian reads as 0xedfe0dd0
    // If the read faults (secure memory not yet accessible), the exception
    // handler catches it silently — so we guard with a manual read first.
    //
    // TODO: Enable full FDT parsing once secure Stage-1 MMU setup is done.
    // For now, use defaults to avoid potential Data Abort on secure DRAM access.
    crate::uart_puts(b"[SPMC] Using default manifest (FDT parsing deferred)\n");
    set_defaults();
}

fn set_defaults() {
    unsafe {
        MANIFEST = Some(SpMcManifest {
            spmc_id: 0x8000,
            maj_ver: 1,
            min_ver: 1,
        });
    }
}

/// Get parsed manifest.
pub fn manifest_info() -> &'static SpMcManifest {
    unsafe {
        (*core::ptr::addr_of!(MANIFEST))
            .as_ref()
            .expect("manifest not initialized")
    }
}

/// Signal SPMD that SPMC init is complete via FFA_MSG_WAIT (0x8400006B).
#[cfg(feature = "sel2")]
pub fn signal_spmc_ready() {
    smc_forward::forward_smc(0x8400_006B, 0, 0, 0, 0, 0, 0, 0);
}
