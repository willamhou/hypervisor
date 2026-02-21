//! SPMC Event Loop — FF-A request dispatch for S-EL2 SPMC role.
//!
//! When booted as BL32 at S-EL2, the hypervisor acts as the SPMC (Secure
//! Partition Manager Core). After initialization, it sends FFA_MSG_WAIT to
//! SPMD (EL3), which returns the first Normal World FF-A request. The SPMC
//! then enters an event loop: dispatch the request, send the response via
//! SMC, and receive the next request.

use crate::ffa;
use crate::ffa::smc_forward::SmcResult8;

/// SPMC event loop — dispatches FF-A requests from SPMD (EL3) forever.
///
/// `first_request` is the SmcResult8 returned by the initial FFA_MSG_WAIT
/// SMC (sent during SPMC boot). Each iteration dispatches the request,
/// sends the response back to SPMD via forward_smc8(), and receives the
/// next request in the return value.
#[cfg(feature = "sel2")]
pub fn run_event_loop(first_request: SmcResult8) -> ! {
    let mut request = first_request;
    loop {
        let response = dispatch_ffa(&request);
        // Send response to SPMD and receive the next request
        request = crate::ffa::smc_forward::forward_smc8(
            response.x0,
            response.x1,
            response.x2,
            response.x3,
            response.x4,
            response.x5,
            response.x6,
            response.x7,
        );
    }
}

/// Dispatch an FF-A request and return the appropriate response.
///
/// Pure function: matches on the FF-A function ID in req.x0 and builds
/// a response SmcResult8. Not gated by feature flags so it can be unit
/// tested on the host.
pub fn dispatch_ffa(req: &SmcResult8) -> SmcResult8 {
    match req.x0 {
        ffa::FFA_VERSION => {
            // Return FF-A v1.1
            SmcResult8 {
                x0: ffa::FFA_VERSION_1_1 as u64,
                x1: 0,
                x2: 0,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_ID_GET => {
            // SPMC partition ID = 0x8000
            SmcResult8 {
                x0: ffa::FFA_SUCCESS_32,
                x1: 0,
                x2: ffa::FFA_SPMC_ID as u64,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_SPM_ID_GET => {
            // SPMC partition ID = 0x8000
            SmcResult8 {
                x0: ffa::FFA_SUCCESS_32,
                x1: 0,
                x2: ffa::FFA_SPMC_ID as u64,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_FEATURES => {
            // Check if the queried function ID (in x1) is supported
            let queried_fid = req.x1;
            let supported = matches!(
                queried_fid,
                ffa::FFA_VERSION
                    | ffa::FFA_ID_GET
                    | ffa::FFA_FEATURES
                    | ffa::FFA_SPM_ID_GET
                    | ffa::FFA_PARTITION_INFO_GET
                    | ffa::FFA_MSG_SEND_DIRECT_REQ_32
                    | ffa::FFA_MSG_SEND_DIRECT_REQ_64
            );
            if supported {
                SmcResult8 {
                    x0: ffa::FFA_SUCCESS_32,
                    x1: 0,
                    x2: 0,
                    x3: 0,
                    x4: 0,
                    x5: 0,
                    x6: 0,
                    x7: 0,
                }
            } else {
                make_error(ffa::FFA_NOT_SUPPORTED as u64)
            }
        }

        ffa::FFA_PARTITION_INFO_GET => {
            // No SPs registered yet — return count=0
            SmcResult8 {
                x0: ffa::FFA_SUCCESS_32,
                x1: 0,
                x2: 0, // partition count = 0
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            }
        }

        ffa::FFA_MSG_SEND_DIRECT_REQ_32 => {
            // Echo x3-x7 back, swap source/dest in x1
            let source = (req.x1 >> 16) & 0xFFFF;
            let dest = req.x1 & 0xFFFF;
            SmcResult8 {
                x0: ffa::FFA_MSG_SEND_DIRECT_RESP_32,
                x1: (dest << 16) | source,
                x2: 0,
                x3: req.x3,
                x4: req.x4,
                x5: req.x5,
                x6: req.x6,
                x7: req.x7,
            }
        }

        ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
            // Echo x3-x7 back, swap source/dest in x1
            let source = (req.x1 >> 16) & 0xFFFF;
            let dest = req.x1 & 0xFFFF;
            SmcResult8 {
                x0: ffa::FFA_MSG_SEND_DIRECT_RESP_64,
                x1: (dest << 16) | source,
                x2: 0,
                x3: req.x3,
                x4: req.x4,
                x5: req.x5,
                x6: req.x6,
                x7: req.x7,
            }
        }

        _ => make_error(ffa::FFA_NOT_SUPPORTED as u64),
    }
}

/// Build an FFA_ERROR response with the given error code in x2.
fn make_error(error_code: u64) -> SmcResult8 {
    SmcResult8 {
        x0: ffa::FFA_ERROR,
        x1: 0,
        x2: error_code,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}
