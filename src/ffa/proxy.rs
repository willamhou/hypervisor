//! FF-A Proxy — main dispatch for FF-A SMC calls.
//!
//! Routes FF-A function IDs to local handlers or stub SPMC.

use crate::arch::aarch64::regs::VcpuContext;
use crate::ffa::*;

/// Handle an FF-A SMC call from guest.
///
/// Called from handle_smc() when function_id is in FF-A range.
/// Returns true to continue guest, false to exit.
pub fn handle_ffa_call(context: &mut VcpuContext) -> bool {
    let function_id = context.gp_regs.x0;

    match function_id {
        FFA_VERSION => handle_version(context),
        FFA_ID_GET => handle_id_get(context),
        FFA_FEATURES => handle_features(context),

        // Blocked: FFA_MEM_DONATE
        FFA_MEM_DONATE_32 | FFA_MEM_DONATE_64 => {
            ffa_error(context, FFA_NOT_SUPPORTED);
            true
        }

        // Not yet implemented — return NOT_SUPPORTED
        _ => {
            ffa_error(context, FFA_NOT_SUPPORTED);
            true
        }
    }
}

/// FFA_VERSION: Return supported FF-A version.
///
/// Input:  x1 = caller's version (ignored for now)
/// Output: x0 = FFA_VERSION_1_1 (0x00010001)
fn handle_version(context: &mut VcpuContext) -> bool {
    context.gp_regs.x0 = FFA_VERSION_1_1 as u64;
    true
}

/// FFA_ID_GET: Return the calling VM's FF-A partition ID.
///
/// Output: x0 = FFA_SUCCESS_32, x2 = partition ID
fn handle_id_get(context: &mut VcpuContext) -> bool {
    let vm_id = crate::global::current_vm_id();
    let part_id = vm_id_to_partition_id(vm_id);
    context.gp_regs.x0 = FFA_SUCCESS_32;
    context.gp_regs.x2 = part_id as u64;
    true
}

/// FFA_FEATURES: Query if a specific FF-A function is supported.
///
/// Input:  x1 = function ID to query
/// Output: x0 = FFA_SUCCESS_32 if supported, FFA_ERROR + NOT_SUPPORTED if not
fn handle_features(context: &mut VcpuContext) -> bool {
    let queried_fid = context.gp_regs.x1;
    let supported = matches!(queried_fid,
        FFA_VERSION | FFA_ID_GET | FFA_FEATURES |
        FFA_RXTX_MAP | FFA_RXTX_UNMAP | FFA_RX_RELEASE |
        FFA_PARTITION_INFO_GET |
        FFA_MSG_SEND_DIRECT_REQ_32 | FFA_MSG_SEND_DIRECT_REQ_64 |
        FFA_MEM_SHARE_32 | FFA_MEM_SHARE_64 |
        FFA_MEM_LEND_32 | FFA_MEM_LEND_64 |
        FFA_MEM_RECLAIM
    );

    if supported {
        context.gp_regs.x0 = FFA_SUCCESS_32;
        context.gp_regs.x2 = 0; // No additional feature properties
    } else {
        ffa_error(context, FFA_NOT_SUPPORTED);
    }
    true
}

/// Set FFA_ERROR return with error code.
/// FF-A error codes are 32-bit signed values in w2 (not sign-extended to 64-bit x2).
pub(crate) fn ffa_error(context: &mut VcpuContext, error_code: i32) {
    context.gp_regs.x0 = FFA_ERROR;
    context.gp_regs.x2 = (error_code as u32) as u64; // Mask to 32 bits, no sign extension
}
