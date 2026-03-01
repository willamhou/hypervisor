//! SPMC manifest parser (TOS_FW_CONFIG DTB).
//! FF-A Core Manifest v1.0 (DEN0077A).

#[cfg(feature = "sel2")]
use crate::ffa::smc_forward;
use core::cell::UnsafeCell;

/// Parsed SPMC manifest properties.
pub struct SpMcManifest {
    pub spmc_id: u16,
    pub maj_ver: u16,
    pub min_ver: u16,
}

struct ManifestCell(UnsafeCell<Option<SpMcManifest>>);
unsafe impl Sync for ManifestCell {}

static MANIFEST: ManifestCell = ManifestCell(UnsafeCell::new(None));
static DEFAULT_MANIFEST: SpMcManifest = SpMcManifest {
    spmc_id: 0x8000,
    maj_ver: 1,
    min_ver: 1,
};

/// Parse TOS_FW_CONFIG DTB passed in x0 by SPMD.
///
/// Extracts SPMC properties from the `/attribute` node per FF-A Core Manifest
/// v1.0 (DEN0077A). Falls back to defaults on parse failure.
pub fn init(manifest_addr: usize) {
    if manifest_addr == 0 {
        crate::log_warn!("[SPMC] WARNING: manifest addr=0, using defaults\n");
        set_defaults();
        return;
    }

    // Parse manifest DTB using fdt crate (zero-copy, no_std).
    // SAFETY: `manifest_addr` is provided by secure boot flow as pointer to TOS_FW_CONFIG DTB blob.
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
            // SAFETY: Single global manifest cell updated during init before concurrent readers.
            unsafe {
                *MANIFEST.0.get() = Some(SpMcManifest {
                    spmc_id,
                    maj_ver: maj,
                    min_ver: min,
                });
            }
        }
        Err(_) => {
            crate::log_warn!("[SPMC] WARNING: manifest FDT parse failed, using defaults\n");
            set_defaults();
        }
    }
}

fn set_defaults() {
    // SAFETY: Default manifest write targets global init cell before concurrent use.
    unsafe {
        *MANIFEST.0.get() = Some(SpMcManifest {
            spmc_id: 0x8000,
            maj_ver: 1,
            min_ver: 1,
        });
    }
}

/// Get parsed manifest.
pub fn manifest_info() -> &'static SpMcManifest {
    // SAFETY: Returns shared ref from initialized static cell; fallback uses immutable default.
    unsafe { (*MANIFEST.0.get()).as_ref().unwrap_or(&DEFAULT_MANIFEST) }
}

/// Signal SPMD that SPMC init is complete via FFA_MSG_WAIT.
/// Returns the first FF-A request from Normal World.
#[cfg(feature = "sel2")]
pub fn signal_spmc_ready() -> smc_forward::SmcResult8 {
    smc_forward::forward_smc8(0x8400_006B, 0, 0, 0, 0, 0, 0, 0)
}
