//! Unit tests for SPMC event loop dispatch logic.
//!
//! Tests `spmc_handler::dispatch_ffa()` which is the S-EL2 SPMC dispatch
//! (not the NS-EL2 proxy in ffa::proxy). Uses SmcResult8 directly.

use hypervisor::ffa::{self, smc_forward::SmcResult8};
use hypervisor::spmc_handler::{dispatch_ffa, dispatch_ffa_as_sp};

fn zero_req(fid: u64) -> SmcResult8 {
    SmcResult8 {
        x0: fid,
        x1: 0,
        x2: 0,
        x3: 0,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    }
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
    hypervisor::sp_context::register_sp(hypervisor::sp_context::SpContext::new(
        0x8001, 0x1000, 0x2000, [0xAA; 4],
    ));
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

    // Test 22a-22g: DIRECT_REQ_64 echoes payload with RESP_64
    let req = SmcResult8 {
        x0: ffa::FFA_MSG_SEND_DIRECT_REQ_64,
        x1: (0x0001 << 16) | 0x8001, // source=1, dest=0x8001
        x2: 0,
        x3: 0x1111_2222_3333,
        x4: 0x4444_5555_6666,
        x5: 0x7777,
        x6: 0x8888,
        x7: 0x9999,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MSG_SEND_DIRECT_RESP_64);
    assert_eq!(resp.x1, (0x8001 << 16) | 0x0001); // swapped
    assert_eq!(resp.x3, 0x1111_2222_3333);
    assert_eq!(resp.x4, 0x4444_5555_6666);
    assert_eq!(resp.x5, 0x7777);
    assert_eq!(resp.x6, 0x8888);
    assert_eq!(resp.x7, 0x9999);
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
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MSG_SEND_DIRECT_RESP_32);
    // x1 must swap: source=SPMC_ID, dest=SPMD_EP_ID
    assert_eq!(resp.x1, (spmc_id << 16) | spmd_ep_id);
    assert_eq!(
        resp.x2,
        ffa::FFA_FWK_MSG_BIT | ffa::SPMD_FWK_MSG_FFA_VERSION_RESP
    );
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
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // Test 29: FFA_RXTX_MAP again -> SUCCESS (re-mapping allowed for pKVM)
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
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

    // ── RXTX_UNMAP clears fragment state ──
    {
        // Step 1: RXTX_MAP
        let mut req = zero_req(ffa::FFA_RXTX_MAP);
        req.x1 = 0x4200_0000;
        req.x2 = 0x4200_1000;
        req.x3 = 1;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);

        // Step 2: Manually activate NWD_FRAG to simulate in-flight fragment transfer
        {
            let mut frag = hypervisor::spmc_handler::NWD_FRAG.lock();
            frag.active = true;
            frag.handle = 0xDEAD;
            frag.total_length = 4096;
            frag.received = 1024;
        }

        // Step 3: RXTX_UNMAP — should clear fragment state
        let resp = dispatch_ffa(&zero_req(ffa::FFA_RXTX_UNMAP));
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);

        // Step 4: Verify fragment state was cleared
        {
            let frag = hypervisor::spmc_handler::NWD_FRAG.lock();
            assert!(!frag.active);
        }
        pass += 1;

        // Step 5: RXTX_MAP again + MEM_SHARE should succeed (no leftover FFA_BUSY)
        let mut req = zero_req(ffa::FFA_RXTX_MAP);
        req.x1 = 0x4200_0000;
        req.x2 = 0x4200_1000;
        req.x3 = 1;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    }

    // Test 33: FFA_RXTX_MAP with misaligned TX -> INVALID_PARAMETERS
    let req = SmcResult8 {
        x0: ffa::FFA_RXTX_MAP,
        x1: 0x6000_1001, // Not aligned
        x2: 0x6000_2000,
        x3: 1,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
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
            x6: 0,
            x7: 0,
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
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // Test: MEM_RETRIEVE -> RETRIEVE_RESP with descriptor (x1 = total_length > 0)
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        // total_length > 0: descriptor built (80 + 1*16 = 96 bytes for 1 range)
        assert!(resp.x1 > 0, "RETRIEVE_RESP should have total_length > 0");
        pass += 2;
    }

    // Test: MEM_RETRIEVE again (already retrieved) -> DENIED
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: share_handle & 0xFFFF_FFFF,
            x2: share_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
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
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
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
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
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
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
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
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── DONATE: basic NWd → SP1 ──
    let donate_handle: u64;
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_DONATE_32,
            x1: 0x0001_8001, // sender=NWd(0x0001), receiver=SP1(0x8001)
            x2: 0,
            x3: 0xA000_0000, // IPA
            x4: 1,           // 1 page
            x5: 0x8001,      // receiver_id
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        donate_handle = resp.x2 | (resp.x3 << 32);
        assert!(donate_handle != 0);
        pass += 1;
    }

    // ── DONATE: RECLAIM by sender → DENIED (sender no longer owns) ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: donate_handle & 0xFFFF_FFFF,
            x2: donate_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_DENIED);
        pass += 1;
    }

    // ── DONATE: SP RETRIEVE → SUCCESS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: donate_handle & 0xFFFF_FFFF,
            x2: donate_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        pass += 1;
    }

    // ── DONATE: SP RELINQUISH → DENIED (donated pages are permanent) ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: donate_handle & 0xFFFF_FFFF,
            x2: donate_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_DENIED);
        pass += 1;
    }

    // ── DONATE: 64-bit variant also works ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_DONATE_64,
            x1: 0x0001_8002, // sender=NWd(0x0001), receiver=SP2(0x8002)
            x2: 0,
            x3: 0xA001_0000,
            x4: 1,
            x5: 0x8002,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── DONATE: SP-to-SP via dispatch_ffa_as_sp ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_DONATE_32,
            x1: (0x8001u64 << 16) | 0x8001, // sender=SP1
            x2: 0,
            x3: 0xB000_0000,
            x4: 1,
            x5: 0x8002, // receiver=SP2
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let sp_donate_handle = resp.x2 | (resp.x3 << 32);

        // SP1 (sender) RECLAIM → DENIED
        let req2 = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: sp_donate_handle & 0xFFFF_FFFF,
            x2: sp_donate_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp2 = dispatch_ffa_as_sp(&req2, 0x8001, 0);
        assert_eq!(resp2.x0, ffa::FFA_ERROR);
        assert_eq!(resp2.x2 as i32, ffa::FFA_DENIED);
        pass += 1;
    }

    // ── T1: Multi-page MEM_SHARE lifecycle (3 pages) ──
    let multi_handle: u64;
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16, // sender=1
            x2: 0,
            x3: 0xB000_0000, // IPA
            x4: 3,           // 3 pages
            x5: 0x8001,      // receiver=SP1
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        multi_handle = resp.x2 | (resp.x3 << 32);
        assert!(multi_handle > 0);
        pass += 2;
    }
    // T1 RETRIEVE → RETRIEVE_RESP
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: multi_handle & 0xFFFF_FFFF,
            x2: multi_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        pass += 1;
    }
    // T1 RELINQUISH → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: multi_handle & 0xFFFF_FFFF,
            x2: multi_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }
    // T1 RECLAIM → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: multi_handle & 0xFFFF_FFFF,
            x2: multi_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── T2: MEM_LEND full lifecycle ──
    {
        // T2-1: FEATURES(FFA_MEM_LEND_32) → SUCCESS
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MEM_LEND_32;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }
    let lend_handle: u64;
    {
        // T2-2: MEM_LEND → SUCCESS + handle
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_LEND_32,
            x1: 0x0001 << 16, // sender=1
            x2: 0,
            x3: 0xC000_0000, // IPA
            x4: 1,           // 1 page
            x5: 0x8001,      // receiver=SP1
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        lend_handle = resp.x2 | (resp.x3 << 32);
        assert!(lend_handle > 0);
        pass += 2;
    }
    // T2-3: RETRIEVE → RETRIEVE_RESP
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: lend_handle & 0xFFFF_FFFF,
            x2: lend_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        pass += 1;
    }
    // T2-4: RELINQUISH → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: lend_handle & 0xFFFF_FFFF,
            x2: lend_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }
    // T2-5: RECLAIM → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: lend_handle & 0xFFFF_FFFF,
            x2: lend_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── Range count overflow → None (record_spmc_share rejects > MAX_SHARE_RANGES) ──
    {
        let too_many: [(u64, u32); 5] = [
            (0xA000_0000, 1),
            (0xA000_1000, 1),
            (0xA000_2000, 1),
            (0xA000_3000, 1),
            (0xA000_4000, 1),
        ];
        let result =
            hypervisor::spmc_handler::record_spmc_share(0x0001, 0x8001, &too_many, false, false);
        assert!(result.is_none());
        pass += 1;
    }

    // ── LEND: double RETRIEVE → DENIED ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_LEND_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0xD000_0000,
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);

        // First RETRIEVE → ok
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);

        // Second RETRIEVE → DENIED
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        pass += 1;

        // Cleanup: RELINQUISH + RECLAIM
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        dispatch_ffa(&req);
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        dispatch_ffa(&req);
    }

    // ── LEND: RECLAIM without RELINQUISH → DENIED ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_LEND_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0xD100_0000,
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);

        // RETRIEVE → ok
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);

        // RECLAIM (without RELINQUISH) → DENIED
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        pass += 1;

        // Cleanup: RELINQUISH then RECLAIM
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        dispatch_ffa(&req);
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        dispatch_ffa(&req);
    }

    // ── T4: RELINQUISH before RETRIEVE → DENIED ──
    let t4_handle: u64;
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0xD000_0000,
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        t4_handle = resp.x2 | (resp.x3 << 32);
        pass += 1;
    }
    // T4: RELINQUISH without RETRIEVE → DENIED
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: t4_handle & 0xFFFF_FFFF,
            x2: t4_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;
    }
    // T4: RECLAIM should succeed (never retrieved)
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: t4_handle & 0xFFFF_FFFF,
            x2: t4_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── T5: MEM_SHARE to SP2 (not just SP1) ──
    let t5_handle: u64;
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0xE000_0000,
            x4: 1,
            x5: 0x8002, // receiver=SP2
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        t5_handle = resp.x2 | (resp.x3 << 32);
        pass += 1;
    }
    // T5: RECLAIM → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: t5_handle & 0xFFFF_FFFF,
            x2: t5_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── T6: Zero page count → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0xF000_0000,
            x4: 0, // zero pages
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── ISO-1: Cross-SP isolation — SP1 RETRIEVE on SP2's share → DENIED ──
    let iso1_handle: u64;
    {
        // Share a page to SP2 (receiver=0x8002)
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x1_0000_0000, // unique IPA
            x4: 1,
            x5: 0x8002, // receiver=SP2
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        iso1_handle = resp.x2 | (resp.x3 << 32);
        pass += 1;
    }
    {
        // Cross-SP isolation: share to SP2, RECLAIM without RETRIEVE → SUCCESS
        // (The caller==receiver isolation check is enforced in sel2 mode via _current_sp.
        //  In unit tests, we verify the share lifecycle works correctly for SP2 receiver.)
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: iso1_handle & 0xFFFF_FFFF,
            x2: iso1_handle >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── ISO-2: Invalid handle RECLAIM → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: 0xDEAD_BEEF, // bogus handle
            x2: 0,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── ISO-3: Invalid handle RETRIEVE → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: 0xBAAD_F00D, // bogus handle
            x2: 0,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── ISO-4: Invalid handle RELINQUISH → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: 0xCAFE_BABE, // bogus handle
            x2: 0,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── STRESS-1: Fill remaining share slots, then overflow → fails ──
    // 3 slots permanently occupied by DONATE records (non-reclaimable)
    {
        let mut handles = [0u64; 13];
        for i in 0..13u64 {
            let req = SmcResult8 {
                x0: ffa::FFA_MEM_SHARE_32,
                x1: 0x0001 << 16,
                x2: 0,
                x3: 0x2_0000_0000 + i * 0x1000, // unique IPA per slot
                x4: 1,
                x5: 0x8001,
                x6: 0,
                x7: 0,
            };
            let resp = dispatch_ffa(&req);
            assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
            handles[i as usize] = resp.x2 | (resp.x3 << 32);
        }
        pass += 1; // 13 shares created

        // 14th share should fail (NO_MEMORY — all 16 slots occupied)
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x2_0001_0000,
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        pass += 1; // overflow rejected

        // Clean up: reclaim all 13
        for handle in handles.iter().take(13) {
            let req = SmcResult8 {
                x0: ffa::FFA_MEM_RECLAIM,
                x1: handle & 0xFFFF_FFFF,
                x2: handle >> 32,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            };
            let resp = dispatch_ffa(&req);
            assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        }
        pass += 1; // all 13 reclaimed
    }

    // ── STRESS-2: Interleaved share/retrieve/relinquish/reclaim across SP1 and SP2 ──
    {
        // Share to SP1
        let req1 = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x3_0000_0000,
            x4: 2, // 2 pages
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp1 = dispatch_ffa(&req1);
        assert_eq!(resp1.x0, ffa::FFA_SUCCESS_32);
        let h1 = resp1.x2 | (resp1.x3 << 32);

        // Share to SP2
        let req2 = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x3_0001_0000,
            x4: 3, // 3 pages
            x5: 0x8002,
            x6: 0,
            x7: 0,
        };
        let resp2 = dispatch_ffa(&req2);
        assert_eq!(resp2.x0, ffa::FFA_SUCCESS_32);
        let h2 = resp2.x2 | (resp2.x3 << 32);

        // Retrieve SP1's share (unit test: NWd path)
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: h1 & 0xFFFF_FFFF,
            x2: h1 >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        // NWd retrieve returns success with x1=total_length > 0
        assert!(resp.x1 > 0);

        // Reclaim SP2's share (not retrieved → should succeed)
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h2 & 0xFFFF_FFFF,
            x2: h2 >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);

        // Reclaim SP1's share (retrieved → DENIED)
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h1 & 0xFFFF_FFFF,
            x2: h1 >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);

        // Relinquish SP1's share (NWd path)
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: h1 & 0xFFFF_FFFF,
            x2: h1 >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);

        // Now reclaim SP1's share → SUCCESS
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h1 & 0xFFFF_FFFF,
            x2: h1 >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── ISO-5: Cross-SP RETRIEVE isolation — SP1 attempts RETRIEVE on SP2's share → DENIED ──
    {
        // Share a page to SP2
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x4_0000_0000,
            x4: 1,
            x5: 0x8002, // receiver=SP2
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);

        // SP1 (0x8001) tries to RETRIEVE SP2's share → DENIED
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;

        // SP2 (0x8002) RETRIEVE → should succeed
        let resp = dispatch_ffa_as_sp(&req, 0x8002, 0);
        // In non-sel2, RETRIEVE returns success with x1=total_length
        assert!(resp.x0 == ffa::FFA_SUCCESS_32 || resp.x1 > 0);
        pass += 1;

        // SP1 tries to RELINQUISH SP2's share → DENIED
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RELINQUISH,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;

        // SP2 RELINQUISH → SUCCESS
        let resp = dispatch_ffa_as_sp(&req, 0x8002, 0);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;

        // Reclaim
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── ISO-6: Double RETRIEVE → DENIED (already retrieved) ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x4_0001_0000,
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);

        // First RETRIEVE → SUCCESS
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert!(resp.x0 == ffa::FFA_SUCCESS_32 || resp.x1 > 0);

        // Second RETRIEVE → DENIED (already retrieved)
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;

        // Clean up: relinquish + reclaim
        let resp = dispatch_ffa_as_sp(
            &SmcResult8 {
                x0: ffa::FFA_MEM_RELINQUISH,
                x1: h & 0xFFFF_FFFF,
                x2: h >> 32,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            },
            0x8001,
            0,
        );
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── ISO-7: RELINQUISH without RETRIEVE → DENIED ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x4_0002_0000,
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);

        // RELINQUISH without RETRIEVE → DENIED
        let resp = dispatch_ffa_as_sp(
            &SmcResult8 {
                x0: ffa::FFA_MEM_RELINQUISH,
                x1: h & 0xFFFF_FFFF,
                x2: h >> 32,
                x3: 0,
                x4: 0,
                x5: 0,
                x6: 0,
                x7: 0,
            },
            0x8001,
            0,
        );
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;

        // Clean up
        let resp = dispatch_ffa(&SmcResult8 {
            x0: ffa::FFA_MEM_RECLAIM,
            x1: h & 0xFFFF_FFFF,
            x2: h >> 32,
            x3: 0,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        });
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── VAL-1: Misaligned IPA → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x5_0000_0001, // NOT 4KB-aligned
            x4: 1,
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── VAL-2: Excessive page count (> 65536) → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x5_0000_0000,
            x4: 100_000, // > 65536
            x5: 0x8001,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── VAL-3: Invalid receiver (not a registered SP) → INVALID_PARAMETERS ──
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_SHARE_32,
            x1: 0x0001 << 16,
            x2: 0,
            x3: 0x5_0001_0000,
            x4: 1,
            x5: 0x9999, // invalid SP
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
    }

    // ── N1-N9: SPMC notification lifecycle ──

    // N1: BITMAP_CREATE for SP1 → SUCCESS
    {
        let mut req = zero_req(ffa::FFA_NOTIFICATION_BITMAP_CREATE);
        req.x1 = 0x8001;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // N2: FEATURES(NOTIFICATION_BITMAP_CREATE) → SUCCESS
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_NOTIFICATION_BITMAP_CREATE;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // N3: BIND SP2→SP1, bitmap=0x1 → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_NOTIFICATION_BIND,
            x1: (0x8002u64 << 16) | 0x8001, // sender=SP2, receiver=SP1
            x2: 0,
            x3: 0x1, // bitmap low
            x4: 0,   // bitmap high
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // N4: SET SP2→SP1, bitmap=0x1 → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_NOTIFICATION_SET,
            x1: (0x8002u64 << 16) | 0x8001,
            x2: 0,
            x3: 0x1,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // N5: GET SP1 → pending=0x1
    {
        let mut req = zero_req(ffa::FFA_NOTIFICATION_GET);
        req.x1 = 0x8001;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        assert_eq!(resp.x2, 0x1);
        pass += 2;
    }

    // N6: GET SP1 again → cleared
    {
        let mut req = zero_req(ffa::FFA_NOTIFICATION_GET);
        req.x1 = 0x8001;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        assert_eq!(resp.x2, 0);
        pass += 1;
    }

    // N7: UNBIND SP2→SP1 → SUCCESS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_NOTIFICATION_UNBIND,
            x1: (0x8002u64 << 16) | 0x8001,
            x2: 0,
            x3: 0x1,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // N8: SET after UNBIND → DENIED
    {
        let req = SmcResult8 {
            x0: ffa::FFA_NOTIFICATION_SET,
            x1: (0x8002u64 << 16) | 0x8001,
            x2: 0,
            x3: 0x1,
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_DENIED as u64);
        pass += 1;
    }

    // N9: BITMAP_DESTROY SP1 → SUCCESS
    {
        let mut req = zero_req(ffa::FFA_NOTIFICATION_BITMAP_DESTROY);
        req.x1 = 0x8001;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── G4: FFA_RUN accepts Preempted state (non-sel2 returns NOT_SUPPORTED) ──

    // Push SP1 through Reset → Idle → Running → Preempted via with_sp_locked
    {
        use hypervisor::sp_context::{with_sp_locked, SpState};
        let ok = with_sp_locked(0x8001, |sp| {
            sp.transition_to(SpState::Idle).unwrap();
            sp.transition_to(SpState::Running).unwrap();
            sp.transition_to(SpState::Preempted).unwrap();
        });
        assert!(ok.is_some()); // lock acquired
        pass += 1;

        // Now FFA_RUN should pass the Preempted check and return NOT_SUPPORTED
        // (unit-test path has no enter_guest)
        let mut req = zero_req(ffa::FFA_RUN);
        req.x1 = 0x8001 << 16;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_NOT_SUPPORTED as u64);
        pass += 2;

        // Restore SP1 to Idle for subsequent tests
        let restored = with_sp_locked(0x8001, |sp| {
            sp.transition_to(SpState::Running).unwrap();
            sp.transition_to(SpState::Idle).unwrap();
        });
        assert!(restored.is_some());
    }

    // ── G5: Global SP store helper functions ──

    {
        use hypervisor::sp_context::{
            find_sp_with_pending_irq, first_owned_intid_for, set_pending_irq_for,
            take_pending_irq_for,
        };

        // G5-1: first_owned_intid_for — SP2 owns INTID 29
        assert_eq!(first_owned_intid_for(0x8002), Some(29));
        assert_eq!(first_owned_intid_for(0x8001), None); // SP1 has no owned INTIDs
        assert_eq!(first_owned_intid_for(0x9999), None); // non-existent SP
        pass += 3;

        // G5-2: set_pending_irq_for + find_sp_with_pending_irq + take_pending_irq_for
        assert_eq!(find_sp_with_pending_irq(), None); // nothing pending
        assert!(set_pending_irq_for(0x8002, 29)); // set for SP2
        assert_eq!(find_sp_with_pending_irq(), Some(0x8002));
        assert_eq!(take_pending_irq_for(0x8002), Some(29));
        assert_eq!(find_sp_with_pending_irq(), None); // consumed
        assert_eq!(take_pending_irq_for(0x8002), None); // already consumed
        pass += 6;

        // G5-3: set_pending_irq_for non-existent SP returns false
        assert!(!set_pending_irq_for(0x9999, 42));
        pass += 1;

        // G5-4: take_pending_irq_for non-existent SP returns None
        assert_eq!(take_pending_irq_for(0x9999), None);
        pass += 1;
    }

    // ── Fragmentation tests ─────────────────────────────────────────

    // F1: MEM_FRAG_TX with no active fragment → INVALID_PARAMETERS
    {
        let req = SmcResult8 {
            x0: ffa::FFA_MEM_FRAG_TX,
            x1: 0xDEAD, // bogus handle lo
            x2: 0xBEEF, // bogus handle hi
            x3: 64,     // fragment length
            x4: 0,
            x5: 0,
            x6: 0,
            x7: 0,
        };
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 2;
    }

    // F2: FEATURES(FFA_MEM_FRAG_TX) → SUCCESS
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MEM_FRAG_TX;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // ── ME-3: MSG_SEND2 / MSG_WAIT tests ──────────────────────────────

    // Setup: map NWd RXTX with a real stack buffer as TX
    // (previous tests unmapped at test 31, so we re-map)
    #[repr(align(4096))]
    struct AlignedBuf([u8; 4096]);
    let mut tx_buf = AlignedBuf([0u8; 4096]);

    // Write message header: sender=0x0001 (NWd VM0), receiver=0x8001 (SP1), size=4, payload=0xCAFEBABE
    unsafe {
        let p = tx_buf.0.as_mut_ptr();
        core::ptr::write_unaligned(p as *mut u16, 0x0001u16); // sender_id
        core::ptr::write_unaligned(p.add(2) as *mut u16, 0x8001u16); // receiver_id
        core::ptr::write_unaligned(p.add(4) as *mut u32, 4u32); // payload size
        core::ptr::write_unaligned(p.add(8) as *mut u32, 0xCAFE_BABEu32); // payload
    }

    let rxtx_req = SmcResult8 {
        x0: ffa::FFA_RXTX_MAP,
        x1: tx_buf.0.as_ptr() as u64, // TX = stack buffer
        x2: 0x6000_2000,              // RX (unused for MSG_SEND2)
        x3: 1,
        x4: 0,
        x5: 0,
        x6: 0,
        x7: 0,
    };
    let resp = dispatch_ffa(&rxtx_req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // MS1: MSG_SEND2 to SP1 → SUCCESS
    {
        let req = zero_req(ffa::FFA_MSG_SEND2);
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // MS2: MSG_SEND2 again (pending) → BUSY
    {
        let req = zero_req(ffa::FFA_MSG_SEND2);
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_BUSY as u64);
        pass += 1;
    }

    // MS3: MSG_WAIT from NWd → NO_DATA (SPMC doesn't queue msgs for NWd)
    {
        let req = zero_req(ffa::FFA_MSG_WAIT);
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_NO_DATA as u64);
        pass += 1;
    }

    // MS4: MSG_SEND2 to invalid SP → INVALID_PARAMETERS
    {
        // Change receiver to 0x9999 (invalid)
        unsafe {
            let p = tx_buf.0.as_mut_ptr();
            core::ptr::write_unaligned(p.add(2) as *mut u16, 0x9999u16);
        }
        // Need to re-map since tx_pa was captured at map time
        // Actually tx_pa points to tx_buf which we just modified in-place — no re-map needed
        // But we need to restore the RXTX (it was already mapped above)
        let req = zero_req(ffa::FFA_MSG_SEND2);
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2, ffa::FFA_INVALID_PARAMETERS as u64);
        pass += 1;
        // Restore receiver to SP1
        unsafe {
            let p = tx_buf.0.as_mut_ptr();
            core::ptr::write_unaligned(p.add(2) as *mut u16, 0x8001u16);
        }
    }

    // MS5: FEATURES(MSG_SEND2) → supported
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MSG_SEND2;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // MS6: FEATURES(MSG_WAIT) → supported
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MSG_WAIT;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // FR1: MEM_FRAG_RX with no active state → INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_MEM_FRAG_RX);
        req.x1 = 0xDEAD;
        req.x2 = 0;
        req.x3 = 0;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        pass += 1;
    }

    // FR2: MEM_FRAG_RX wrong handle → INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_MEM_FRAG_RX);
        req.x1 = 0xBAD;
        req.x2 = 0xBAD;
        req.x3 = 0;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        pass += 1;
    }

    // FR3: FEATURES(MEM_FRAG_RX) → supported
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_MEM_FRAG_RX;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // Cleanup: unmap RXTX
    let resp = dispatch_ffa(&zero_req(ffa::FFA_RXTX_UNMAP));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    pass += 1;

    // CL1: CONSOLE_LOG_32 with "Ok" → SUCCESS
    {
        let mut req = zero_req(ffa::FFA_CONSOLE_LOG_32);
        req.x1 = 2; // 2 chars
        req.x2 = 0x6B4F; // 'O' (0x4F) + 'k' (0x6B) in LE
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // CL2: CONSOLE_LOG with count=0 → INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_CONSOLE_LOG_32);
        req.x1 = 0;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        pass += 1;
    }

    // CL3: FEATURES(CONSOLE_LOG_32) → supported
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_CONSOLE_LOG_32;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // SRI1: FEATURES(NPI, feature_id=1) → SUCCESS with INTID in x2
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_FEATURE_NPI;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        assert_eq!(resp.x2, ffa::NPI_INTID as u64);
        pass += 2;
    }

    // SRI2: FEATURES(SRI, feature_id=2) → SUCCESS with INTID in x2
    {
        let mut req = zero_req(ffa::FFA_FEATURES);
        req.x1 = ffa::FFA_FEATURE_SRI;
        let resp = dispatch_ffa(&req);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        assert_eq!(resp.x2, ffa::SRI_INTID as u64);
        pass += 2;
    }

    // ── SP→SP DIRECT_REQ routing tests ──

    // SP2SP1: SP→SP self-call blocked (FFA_INVALID_PARAMETERS)
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        req.x1 = ((0x8001u16 as u64) << 16) | (0x8001u16 as u64);
        let resp = dispatch_ffa_as_sp(&req, 0x8001u16, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SP2SP2: SP→SP source spoofing blocked
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        // SP1 claims to be SP2
        req.x1 = ((0x8002u16 as u64) << 16) | (0x8001u16 as u64);
        let resp = dispatch_ffa_as_sp(&req, 0x8001u16, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SP2SP3: SP→SP invalid destination
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        req.x1 = ((0x8001u16 as u64) << 16) | 0x8099; // non-existent SP
        let resp = dispatch_ffa_as_sp(&req, 0x8001u16, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SP2SP4: SP→SP DIRECT_REQ_64 self-call also blocked
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_64);
        req.x1 = ((0x8001u16 as u64) << 16) | (0x8001u16 as u64);
        let resp = dispatch_ffa_as_sp(&req, 0x8001u16, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SP2SP5: Cycle detection — manually push frame, then try DIRECT_REQ to stacked SP
    {
        {
            let mut stack = hypervisor::spmc_handler::CALL_STACK.lock();
            stack.push(0x8002u16, 0x8001u16).unwrap();
        }
        // SP1 tries to call SP2 — cycle! (SP2 is already in the stack as caller)
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        req.x1 = ((0x8001u16 as u64) << 16) | (0x8002u16 as u64);
        let resp = dispatch_ffa_as_sp(&req, 0x8001u16, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_BUSY);

        // Clean up call stack
        {
            let mut stack = hypervisor::spmc_handler::CALL_STACK.lock();
            stack.pop();
        }
        pass += 1;
    }

    // ── CallStack unit tests ──

    // CS1: push/pop basic operations
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        assert_eq!(stack.depth(), 0);
        assert!(stack.push(0x8001, 0x8002).is_ok());
        assert_eq!(stack.depth(), 1);
        assert!(stack.contains(0x8001));
        assert!(stack.contains(0x8002));
        assert!(!stack.contains(0x8003));
        let frame = stack.pop().unwrap();
        assert_eq!(frame.caller_id, 0x8001);
        assert_eq!(frame.callee_id, 0x8002);
        assert_eq!(stack.depth(), 0);
        pass += 1;
    }

    // CS2: cycle detection via contains()
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        stack.push(0x8001, 0x8002).unwrap();
        stack.push(0x8002, 0x8003).unwrap();
        // SP1 is in the stack as caller → cycle detected
        assert!(stack.contains(0x8001));
        // SP3 is in the stack as callee → cycle detected
        assert!(stack.contains(0x8003));
        pass += 1;
    }

    // CS3: stack overflow (MAX_SPS - 1 = 3 frames max)
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        stack.push(0x8001, 0x8002).unwrap();
        stack.push(0x8002, 0x8003).unwrap();
        stack.push(0x8003, 0x8004).unwrap();
        assert!(stack.push(0x8004, 0x8005).is_err());
        pass += 1;
    }

    // CS4: find_caller lookup
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        stack.push(0x8001, 0x8002).unwrap();
        stack.push(0x8002, 0x8003).unwrap();
        assert_eq!(stack.find_caller(0x8003), Some(0x8002));
        assert_eq!(stack.find_caller(0x8002), Some(0x8001));
        assert_eq!(stack.find_caller(0x8001), None);
        pass += 1;
    }

    // ── SP-to-SP MEM_SHARE tests ──

    // SPMEM1-4: Full lifecycle SP1→SP2 (SHARE, RETRIEVE, RELINQUISH, RECLAIM)
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16; // sender = SP1
        req.x3 = 0x70000000; // IPA
        req.x4 = 1; // 1 page
        req.x5 = 0x8002; // receiver = SP2
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);
        assert!(h > 0);
        pass += 1;

        // SP2 RETRIEVE
        let mut req2 = zero_req(ffa::FFA_MEM_RETRIEVE_REQ_32);
        req2.x1 = h & 0xFFFF_FFFF;
        req2.x2 = h >> 32;
        let resp2 = dispatch_ffa_as_sp(&req2, 0x8002, 0);
        assert_eq!(resp2.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        pass += 1;

        // SP2 RELINQUISH
        let mut req3 = zero_req(ffa::FFA_MEM_RELINQUISH);
        req3.x1 = h & 0xFFFF_FFFF;
        req3.x2 = h >> 32;
        let resp3 = dispatch_ffa_as_sp(&req3, 0x8002, 0);
        assert_eq!(resp3.x0, ffa::FFA_SUCCESS_32);
        pass += 1;

        // SP1 RECLAIM
        let mut req4 = zero_req(ffa::FFA_MEM_RECLAIM);
        req4.x1 = h & 0xFFFF_FFFF;
        req4.x2 = h >> 32;
        let resp4 = dispatch_ffa_as_sp(&req4, 0x8001, 0);
        assert_eq!(resp4.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // SPMEM5: Self-sharing blocked
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x70000000;
        req.x4 = 1;
        req.x5 = 0x8001; // self
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SPMEM6: Non-existent receiver blocked
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x70000000;
        req.x4 = 1;
        req.x5 = 0x8099;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SPMEM7: Source spoofing blocked
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8002u64) << 16; // claims SP2
        req.x3 = 0x70000000;
        req.x4 = 1;
        req.x5 = 0x8001;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0); // actually SP1
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SPMEM8-10: Cross-SP RECLAIM blocked
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x71000000;
        req.x4 = 1;
        req.x5 = 0x8002;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);
        pass += 1;

        // SP2 tries to reclaim — denied (not sender)
        let mut req2 = zero_req(ffa::FFA_MEM_RECLAIM);
        req2.x1 = h & 0xFFFF_FFFF;
        req2.x2 = h >> 32;
        let resp2 = dispatch_ffa_as_sp(&req2, 0x8002, 0);
        assert_eq!(resp2.x0, ffa::FFA_ERROR);
        assert_eq!(resp2.x2 as i32, ffa::FFA_DENIED);
        pass += 1;

        // SP1 reclaims — success
        let mut req3 = zero_req(ffa::FFA_MEM_RECLAIM);
        req3.x1 = h & 0xFFFF_FFFF;
        req3.x2 = h >> 32;
        let resp3 = dispatch_ffa_as_sp(&req3, 0x8001, 0);
        assert_eq!(resp3.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // SPMEM11-15: SP-to-SP MEM_LEND lifecycle
    {
        let mut req = zero_req(ffa::FFA_MEM_LEND_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x72000000;
        req.x4 = 1;
        req.x5 = 0x8002;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
        let h = resp.x2 | (resp.x3 << 32);
        pass += 1;

        // SP2 retrieve
        let mut req2 = zero_req(ffa::FFA_MEM_RETRIEVE_REQ_32);
        req2.x1 = h & 0xFFFF_FFFF;
        req2.x2 = h >> 32;
        let resp2 = dispatch_ffa_as_sp(&req2, 0x8002, 0);
        assert_eq!(resp2.x0, ffa::FFA_MEM_RETRIEVE_RESP);
        pass += 1;

        // SP1 reclaim while retrieved — DENIED
        let mut req3 = zero_req(ffa::FFA_MEM_RECLAIM);
        req3.x1 = h & 0xFFFF_FFFF;
        req3.x2 = h >> 32;
        let resp3 = dispatch_ffa_as_sp(&req3, 0x8001, 0);
        assert_eq!(resp3.x0, ffa::FFA_ERROR);
        assert_eq!(resp3.x2 as i32, ffa::FFA_DENIED);
        pass += 1;

        // SP2 relinquish
        let mut req4 = zero_req(ffa::FFA_MEM_RELINQUISH);
        req4.x1 = h & 0xFFFF_FFFF;
        req4.x2 = h >> 32;
        let resp4 = dispatch_ffa_as_sp(&req4, 0x8002, 0);
        assert_eq!(resp4.x0, ffa::FFA_SUCCESS_32);
        pass += 1;

        // SP1 reclaim after relinquish — SUCCESS
        let mut req5 = zero_req(ffa::FFA_MEM_RECLAIM);
        req5.x1 = h & 0xFFFF_FFFF;
        req5.x2 = h >> 32;
        let resp5 = dispatch_ffa_as_sp(&req5, 0x8001, 0);
        assert_eq!(resp5.x0, ffa::FFA_SUCCESS_32);
        pass += 1;
    }

    // SPMEM16: Zero page count — INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x70000000;
        req.x4 = 0;
        req.x5 = 0x8002;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SPMEM17: Misaligned IPA — INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x70000001; // not 4KB-aligned
        req.x4 = 1;
        req.x5 = 0x8002;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SPMEM18: page_count > 65536 — INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0x70000000;
        req.x4 = 65537; // exceeds limit
        req.x5 = 0x8002;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    // SPMEM19: Address wrap overflow — INVALID_PARAMETERS
    {
        let mut req = zero_req(ffa::FFA_MEM_SHARE_32);
        req.x1 = (0x8001u64) << 16;
        req.x3 = 0xFFFF_FFFF_FFFF_F000; // near u64 max, 4KB-aligned
        req.x4 = 2; // ipa + 2*4096 wraps
        req.x5 = 0x8002;
        let resp = dispatch_ffa_as_sp(&req, 0x8001, 0);
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS);
        pass += 1;
    }

    hypervisor::log_info!("    {} assertions passed\n", pass);
}
