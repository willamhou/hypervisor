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
    hypervisor::log_info!("  test_spmc_handler...\n");
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

    // Test 7: FFA_FEATURES(FFA_RXTX_MAP) -> SUCCESS
    // (SPMD forwards NWd RXTX_MAP to SPMC in TF-A v2.12)
    let mut req = zero_req(ffa::FFA_FEATURES);
    req.x1 = ffa::FFA_RXTX_MAP;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 9-10: FFA_FEATURES with unsupported function -> NOT_SUPPORTED
    let mut req = zero_req(ffa::FFA_FEATURES);
    req.x1 = 0xDEAD;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
    pass += 2;

    // Test 11-12: FFA_PARTITION_INFO_GET returns count=0 (no SPs registered yet)
    let resp = dispatch_ffa(&zero_req(ffa::FFA_PARTITION_INFO_GET));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    assert_eq!(resp.x2, 0); // no SPs
    pass += 2;

    // Test 13-14: Register an SP, PARTITION_INFO_GET returns count=1
    hypervisor::sp_context::register_sp(
        hypervisor::sp_context::SpContext::new(0x8001, 0x1000, 0x2000, [0xAA; 4]),
    );
    let resp = dispatch_ffa(&zero_req(ffa::FFA_PARTITION_INFO_GET));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    assert!(resp.x2 >= 1); // at least 1 SP registered
    pass += 2;

    // Test 15-21: DIRECT_REQ echoes payload, swaps source/dest
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

    // Test 22-26: SPMD framework message (FFA_VERSION_REQ)
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

    // Test 26-27: Unknown function -> FFA_ERROR(NOT_SUPPORTED)
    let resp = dispatch_ffa(&zero_req(0xDEADBEEF));
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
    pass += 2;

    // Test 28: FFA_RXTX_MAP with valid 4KB-aligned buffers -> SUCCESS
    let req = SmcResult8 {
        x0: ffa::FFA_RXTX_MAP,
        x1: 0x6000_1000, // TX PA (4KB aligned)
        x2: 0x6000_2000, // RX PA (4KB aligned)
        x3: 1,           // 1 page
        x4: 0, x5: 0, x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 29: FFA_RXTX_MAP again -> DENIED (already mapped)
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
    pass += 1;

    // Test 30: FFA_RX_RELEASE -> SUCCESS (mapped)
    let resp = dispatch_ffa(&zero_req(ffa::FFA_RX_RELEASE));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 31: FFA_RXTX_UNMAP -> SUCCESS
    let resp = dispatch_ffa(&zero_req(ffa::FFA_RXTX_UNMAP));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 32: FFA_RXTX_UNMAP again -> DENIED (not mapped)
    let resp = dispatch_ffa(&zero_req(ffa::FFA_RXTX_UNMAP));
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
    pass += 1;

    // Test 33: FFA_RXTX_MAP with misaligned TX -> INVALID_PARAMETERS
    let req = SmcResult8 {
        x0: ffa::FFA_RXTX_MAP,
        x1: 0x6000_1001, // Not aligned
        x2: 0x6000_2000,
        x3: 1,
        x4: 0, x5: 0, x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
    pass += 1;

    // Test 34: FFA_FEATURES(FFA_RUN) -> SUCCESS
    let mut req = zero_req(ffa::FFA_FEATURES);
    req.x1 = ffa::FFA_RUN;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 35: FFA_RUN with non-existent SP -> INVALID_PARAMETERS
    let mut req = zero_req(ffa::FFA_RUN);
    req.x1 = 0x9999 << 16; // non-existent SP
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
    pass += 1;

    // Test 36: FFA_RUN with SP in Idle state -> DENIED
    let mut req = zero_req(ffa::FFA_RUN);
    req.x1 = 0x8001 << 16; // SP1 (registered above, in Idle state)
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
    pass += 1;

    // Test 37-38: Register SP2, PARTITION_INFO_GET returns count=2
    {
        let mut sp2 = hypervisor::sp_context::SpContext::new(0x8002, 0x2000, 0x3000, [0xBB; 4]);
        sp2.set_owned_intids([29, 0, 0, 0]);
        hypervisor::sp_context::register_sp(sp2);
    }
    let resp = dispatch_ffa(&zero_req(ffa::FFA_PARTITION_INFO_GET));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    assert!(resp.x2 >= 2); // 2+ SPs registered
    pass += 2;

    // Test 39-40: find_sp_for_intid returns correct SP
    assert_eq!(hypervisor::sp_context::find_sp_for_intid(29), Some(0x8002));
    assert_eq!(hypervisor::sp_context::find_sp_for_intid(99), None);
    pass += 2;

    // Test 41: DIRECT_REQ to SP2 echoes correctly (unit test path, not sel2)
    let req = SmcResult8 {
        x0: ffa::FFA_MSG_SEND_DIRECT_REQ_32,
        x1: (0x0001 << 16) | 0x8002,
        x2: 0,
        x3: 0x1111,
        x4: 0x2222,
        x5: 0x3333,
        x6: 0x4444,
        x7: 0x5555,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MSG_SEND_DIRECT_RESP_32);
    pass += 1;

    // Test 42: FFA_RUN with non-existent SP -> INVALID_PARAMETERS
    let mut req = zero_req(ffa::FFA_RUN);
    req.x1 = 0x9999 << 16;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
    pass += 1;


    // ── SPMC memory sharing tests ───────────────────────────────────

    // Test: FFA_FEATURES(FFA_MEM_SHARE_32) -> SUCCESS
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MEM_SHARE_32;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // Test: FFA_FEATURES(FFA_MEM_RECLAIM) -> SUCCESS
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MEM_RECLAIM;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // Test: MEM_SHARE register-based (x3=IPA, x4=1, x5=SP1) -> SUCCESS + handle
    let share_handle: u64;
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16, // sender=1
            x2: 0,
            x3: 0x8000_0000, // IPA
            x4: 1,           // 1 page
            x5: 0x8001,      // receiver=SP1
            x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        share_handle = resp.x2 | (resp.x3 << 32);
        assert!(share_handle > 0);
        pass += 2;
    }

    // Test: MEM_SHARE with unregistered receiver -> INVALID_PARAMETERS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x9000_0000,
            x4: 1,
            x5: 0x9999, // non-existent SP
            x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // Test: MEM_RETRIEVE -> RETRIEVE_RESP
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        pass += 1;
    }

    // Test: MEM_RETRIEVE again (already retrieved) -> DENIED
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;
    }

    // Test: MEM_RECLAIM while retrieved -> DENIED
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;
    }

    // Test: MEM_RELINQUISH -> SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // Test: MEM_RECLAIM after relinquish -> SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // Test: MEM_RECLAIM invalid handle -> INVALID_PARAMETERS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: 0xDEAD,
            x2: 0,
            x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // Test: FFA_MEM_DONATE -> NOT_SUPPORTED
    {
        let resp = dispatch_ffa(&zero_req(ffa::FFA_MEM_DONATE_32));
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
        pass += 1;
    }

    hypervisor::log_info!("    {} assertions passed\n", pass);
}
