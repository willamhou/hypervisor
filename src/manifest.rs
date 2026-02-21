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
/// Extracts SPMC properties from the `/attribute` node per FF-A Core Manifest
/// v1.0 (DEN0077A). Falls back to defaults on parse failure.
pub fn init(manifest_addr: usize) {
    if manifest_addr == 0 {
        crate::uart_puts(b"[SPMC] WARNING: manifest addr=0, using defaults\n");
        set_defaults();
        return;
    }

    // Parse manifest DTB using fdt crate (zero-copy, no_std).
    let fdt = unsafe { fdt::Fdt::from_ptr(manifest_addr as *const u8) };
    match fdt {
        Ok(fdt) => {
            // Extract /attribute node properties (spmc_id, major/minor version)
            let spmc_id = fdt
                .find_node("/attribute")
                .and_then(|n| n.property("spmc_id"))
                .and_then(|p| p.as_usize())
                .unwrap_or(0x8000) as u16;
            let maj = fdt
                .find_node("/attribute")
                .and_then(|n| n.property("maj_ver"))
                .and_then(|p| p.as_usize())
                .unwrap_or(1) as u16;
            let min = fdt
                .find_node("/attribute")
                .and_then(|n| n.property("min_ver"))
                .and_then(|p| p.as_usize())
                .unwrap_or(1) as u16;
            unsafe {
                MANIFEST = Some(SpMcManifest {
                    spmc_id,
                    maj_ver: maj,
                    min_ver: min,
                });
            }
        }
        Err(_) => {
            crate::uart_puts(b"[SPMC] WARNING: manifest FDT parse failed, using defaults\n");
            set_defaults();
        }
    }
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
