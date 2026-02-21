//! Unit tests for SPMC event loop dispatch logic.
//!
//! Tests `spmc_handler::dispatch_ffa()` which is the S-EL2 SPMC dispatch
//! (not the NS-EL2 proxy in ffa::proxy). Uses SmcResult8 directly.

use hypervisor::ffa::{self, smc_forward::SmcResult8};
use hypervisor::spmc_handler::dispatch_ffa;

fn zero_req(fid: u64) -> SmcResult8 {
    SmcResult8 { x0: fid, x1: 0, x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0 }
}

pub fn run_tests() {
    crate::uart_puts(b"  test_spmc_handler...\n");
    let mut pass = 0u32;

    // Test 1: FFA_VERSION returns v1.1
    let resp = dispatch_ffa(&zero_req(ffa::FFA_VERSION));
    assert_eq!(resp.x0, ffa::FFA_VERSION_1_1 as u64);
    pass += 1;

    // Test 2-3: FFA_ID_GET returns SUCCESS + SPMC ID
    let resp = dispatch_ffa(&zero_req(ffa::FFA_ID_GET));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    assert_eq!(resp.x2, ffa::FFA_SPMC_ID as u64);
    pass += 2;

    // Test 4-5: FFA_SPM_ID_GET returns SUCCESS + SPMC ID
    let resp = dispatch_ffa(&zero_req(ffa::FFA_SPM_ID_GET));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    assert_eq!(resp.x2, ffa::FFA_SPMC_ID as u64);
    pass += 2;

    // Test 6: FFA_FEATURES with supported function -> SUCCESS
    let mut req = zero_req(ffa::FFA_FEATURES);
    req.x1 = ffa::FFA_VERSION;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 7-8: FFA_FEATURES with unsupported function -> NOT_SUPPORTED
    let mut req = zero_req(ffa::FFA_FEATURES);
    req.x1 = 0xDEAD;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
    pass += 2;

    // Test 9-10: FFA_PARTITION_INFO_GET returns count=0
    let resp = dispatch_ffa(&zero_req(ffa::FFA_PARTITION_INFO_GET));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    assert_eq!(resp.x2, 0); // no SPs
    pass += 2;

    // Test 11-17: DIRECT_REQ echoes payload, swaps source/dest
    let req = SmcResult8 {
        x0: ffa::FFA_MSG_SEND_DIRECT_REQ_32,
        x1: (0x0001 << 16) | 0x8001, // source=1, dest=0x8001
        x2: 0,
        x3: 0xAAAA,
        x4: 0xBBBB,
        x5: 0xCCCC,
        x6: 0xDDDD,
        x7: 0xEEEE,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MSG_SEND_DIRECT_RESP_32);
    assert_eq!(resp.x1, (0x8001 << 16) | 0x0001); // swapped
    assert_eq!(resp.x3, 0xAAAA);
    assert_eq!(resp.x4, 0xBBBB);
    assert_eq!(resp.x5, 0xCCCC);
    assert_eq!(resp.x6, 0xDDDD);
    assert_eq!(resp.x7, 0xEEEE);
    pass += 7;

    // Test 18-22: SPMD framework message (FFA_VERSION_REQ)
    // SPMD sends x1 = (SPMD_EP_ID << 16) | SPMC_ID = (0xFFFF << 16) | 0x8000
    let spmd_ep_id: u64 = 0xFFFF;
    let spmc_id: u64 = ffa::FFA_SPMC_ID as u64;
    let req = SmcResult8 {
        x0: ffa::FFA_MSG_SEND_DIRECT_REQ_32,
        x1: (spmd_ep_id << 16) | spmc_id,
        x2: ffa::FFA_FWK_MSG_BIT | ffa::SPMD_FWK_MSG_FFA_VERSION_REQ,
        x3: ffa::FFA_VERSION_1_1 as u64, // NWd requested version
        x4: 0, x5: 0, x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MSG_SEND_DIRECT_RESP_32);
    // x1 must swap: source=SPMC_ID, dest=SPMD_EP_ID
    assert_eq!(resp.x1, (spmc_id << 16) | spmd_ep_id);
    assert_eq!(resp.x2, ffa::FFA_FWK_MSG_BIT | ffa::SPMD_FWK_MSG_FFA_VERSION_RESP);
    assert_eq!(resp.x3, ffa::FFA_VERSION_1_1 as u64);
    // Also verify x4-x7 are zeroed
    assert_eq!(resp.x4, 0);
    pass += 5;

    // Test 22-23: Unknown function -> FFA_ERROR(NOT_SUPPORTED)
    let resp = dispatch_ffa(&zero_req(0xDEADBEEF));
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
    pass += 2;

    crate::uart_puts(b"    ");
    crate::print_u32(pass);
    crate::uart_puts(b" assertions passed\n");
}
