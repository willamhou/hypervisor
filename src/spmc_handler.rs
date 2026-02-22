//! SPMC Event Loop — FF-A request dispatch for S-EL2 SPMC role.
//!
//! When booted as BL32 at S-EL2, the hypervisor acts as the SPMC (Secure
//! Partition Manager Core). After initialization, it sends FFA_MSG_WAIT to
//! SPMD (EL3), which returns the first Normal World FF-A request. The SPMC
//! then enters an event loop: dispatch the request, send the response via
//! SMC, and receive the next request.

use crate::ffa;
use crate::ffa::smc_forward::SmcResult8;

// ── SPMC RXTX buffers (registered with SPMD for PARTITION_INFO relay) ──

/// 4KB-aligned page for SPMC TX/RX buffers.
#[cfg(feature = "sel2")]
#[repr(C, align(4096))]
struct AlignedPage([u8; 4096]);

#[cfg(feature = "sel2")]
static mut SPMC_TX_BUF: AlignedPage = AlignedPage([0u8; 4096]);
#[cfg(feature = "sel2")]
static mut SPMC_RX_BUF: AlignedPage = AlignedPage([0u8; 4096]);

/// Register SPMC's TX/RX buffers with SPMD via FFA_RXTX_MAP.
///
/// Must be called after SP boot completes, before `signal_spmc_ready()`.
#[cfg(feature = "sel2")]
pub fn spmc_register_rxtx() {
    let tx_pa = &raw const SPMC_TX_BUF as u64;
    let rx_pa = &raw const SPMC_RX_BUF as u64;
    let result = crate::ffa::smc_forward::forward_smc8(
        ffa::FFA_RXTX_MAP,
        tx_pa,
        rx_pa,
        1, // 1 page
        0,
        0,
        0,
        0,
    );
    if result.x0 == ffa::FFA_SUCCESS_32 {
        crate::uart_puts(b"[SPMC] RXTX registered with SPMD\n");
    } else {
        crate::uart_puts(b"[SPMC] WARNING: RXTX registration failed, x0=0x");
        crate::uart_put_hex(result.x0);
        crate::uart_puts(b"\n");
    }
}

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
        let response = dispatch_request(&request);
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

/// Dispatch an FF-A request. Routes to SP or local SPMC handling.
#[cfg(feature = "sel2")]
fn dispatch_request(req: &SmcResult8) -> SmcResult8 {
    if req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_32
        || req.x0 == ffa::FFA_MSG_SEND_DIRECT_REQ_64
    {
        let dest = (req.x1 & 0xFFFF) as u16;
        if crate::sp_context::is_registered_sp(dest) {
            return dispatch_to_sp(req, dest);
        }
    }
    dispatch_ffa(req)
}

/// Route DIRECT_REQ to an SP: ERET, wait for response, return it.
#[cfg(feature = "sel2")]
fn dispatch_to_sp(req: &SmcResult8, sp_id: u16) -> SmcResult8 {
    let sp = match crate::sp_context::get_sp_mut(sp_id) {
        Some(sp) => sp,
        None => return make_error(ffa::FFA_INVALID_PARAMETERS as u64),
    };

    if sp.state() != crate::sp_context::SpState::Idle {
        return make_error(ffa::FFA_BUSY as u64);
    }

    // Set up SP registers with the DIRECT_REQ args
    sp.set_args(req.x0, req.x1, req.x2, req.x3, req.x4, req.x5, req.x6, req.x7);
    sp.transition_to(crate::sp_context::SpState::Running)
        .expect("SP Running transition failed");

    // Reinstall SP's Secure Stage-2 and ERET
    let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
    s2.install();

    let _exit = unsafe {
        crate::arch::aarch64::enter_guest(
            sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
        )
    };

    // SP trapped back
    sp.transition_to(crate::sp_context::SpState::Idle)
        .expect("SP Idle transition failed");

    let (x0, x1, x2, x3, x4, x5, x6, x7) = sp.get_args();
    SmcResult8 {
        x0,
        x1,
        x2,
        x3,
        x4,
        x5,
        x6,
        x7,
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
            // Note: FFA_RXTX_MAP is NOT listed here because SPMD handles
            // NWd RXTX registration directly — it is never forwarded to SPMC.
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
            handle_partition_info_get()
        }

        ffa::FFA_MSG_SEND_DIRECT_REQ_32 => {
            handle_direct_req_32(req)
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

/// Handle PARTITION_INFO_GET — writes 24-byte descriptors to SPMC TX buffer.
///
/// FF-A v1.1 partition info descriptor (DEN0077A Table 5.37):
///   Offset 0:  partition_id    (u16 LE)
///   Offset 2:  exec_ctx_count  (u16 LE)
///   Offset 4:  properties      (u32 LE)
///   Offset 8:  uuid[16]        (128-bit UUID)
///
/// Returns SUCCESS + count in x2. SPMD copies TX→NWd RX.
fn handle_partition_info_get() -> SmcResult8 {
    let mut count = 0u64;

    // Write descriptors for each registered SP into the TX buffer.
    // In unit-test mode (no sel2 feature), we can't write to the TX buffer
    // because it doesn't exist, so we just return the count.
    #[cfg(feature = "sel2")]
    {
        let tx_ptr = &raw mut SPMC_TX_BUF as *mut u8;
        crate::sp_context::for_each_sp(|sp| {
            let offset = count as usize * 24;
            if offset + 24 > 4096 {
                return; // TX buffer full
            }
            unsafe {
                let ptr = tx_ptr.add(offset);
                // partition_id (u16 LE)
                core::ptr::write_unaligned(ptr as *mut u16, sp.sp_id());
                // exec_ctx_count (u16 LE)
                core::ptr::write_unaligned(ptr.add(2) as *mut u16, 1);
                // properties (u32 LE) — bit 0: supports DIRECT_REQ
                core::ptr::write_unaligned(ptr.add(4) as *mut u32, 0x1);
                // UUID (16 bytes) — read from SpContext (parsed from SP manifest)
                core::ptr::copy_nonoverlapping(
                    sp.uuid().as_ptr() as *const u8,
                    ptr.add(8),
                    16,
                );
            }
            count += 1;
        });
    }

    // In non-sel2 mode (unit tests), just count registered SPs
    #[cfg(not(feature = "sel2"))]
    {
        crate::sp_context::for_each_sp(|_| {
            count += 1;
        });
    }

    SmcResult8 {
        x0: ffa::FFA_SUCCESS_32,
        x1: 0,
        x2: count,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
}

/// Handle DIRECT_REQ_32 — checks for SPMD framework messages first.
///
/// SPMD wraps certain FF-A calls (e.g. FFA_VERSION) as framework messages
/// inside DIRECT_REQ with FFA_FWK_MSG_BIT set in x2. We must detect and
/// respond to these before falling through to the normal echo handler.
fn handle_direct_req_32(req: &SmcResult8) -> SmcResult8 {
    // Check for SPMD framework message (FFA_FWK_MSG_BIT set in x2)
    if (req.x2 & ffa::FFA_FWK_MSG_BIT) != 0 {
        let fwk_func = req.x2 & !ffa::FFA_FWK_MSG_BIT;
        // Swap source/dest from the request so SPMD recognizes us.
        // SPMD sends x1 = (SPMD_EP_ID << 16) | SPMC_ID.
        // We must respond with x1 = (SPMC_ID << 16) | SPMD_EP_ID.
        let source = (req.x1 >> 16) & 0xFFFF;
        let dest = req.x1 & 0xFFFF;
        let swapped_x1 = (dest << 16) | source;
        if fwk_func == ffa::SPMD_FWK_MSG_FFA_VERSION_REQ {
            // SPMD forwarding NWd's FFA_VERSION. x3 = requested version.
            return SmcResult8 {
                x0: ffa::FFA_MSG_SEND_DIRECT_RESP_32,
                x1: swapped_x1,
                x2: ffa::FFA_FWK_MSG_BIT | ffa::SPMD_FWK_MSG_FFA_VERSION_RESP,
                x3: ffa::FFA_VERSION_1_1 as u64,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            };
        }
        // Unknown framework message
        return make_error(ffa::FFA_NOT_SUPPORTED as u64);
    }

    // Normal direct request: echo x3-x7, swap source/dest in x1
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
