//! FF-A proxy unit tests
//!
//! Tests FF-A function dispatching using direct function calls
//! (not actual SMC — we test the proxy logic, not the trap path).

use hypervisor::arch::aarch64::regs::VcpuContext;
use hypervisor::ffa;

pub fn run_ffa_test() {
    hypervisor::log_info!("\n=== Test: FF-A Proxy ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    // Clear VTTBR_EL2 to avoid stale page tables from earlier VM tests.
    // Earlier tests (test_mmio, test_simple_guest) create VMs that set VTTBR
    // to their own Stage-2 tables. The MEM_SHARE handler checks has_stage2()
    // and would attempt ownership validation against those incomplete tables.
    // SAFETY: Test setup resets VTTBR_EL2 to avoid cross-test Stage-2 state leakage.
    unsafe {
        core::arch::asm!("msr vttbr_el2, xzr", "isb", options(nomem, nostack));
    }

    // Test 1: FFA_VERSION returns v1.1
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_VERSION;
        ctx.gp_regs.x1 = ffa::FFA_VERSION_1_1 as u64;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_VERSION_1_1 as u64 {
            hypervisor::log_info!("  [PASS] FFA_VERSION returns 0x00010001\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_VERSION\n");
            fail += 1;
        }
    }

    // Test 2: FFA_ID_GET returns partition ID
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_ID_GET;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && ctx.gp_regs.x2 == 1 {
            hypervisor::log_info!("  [PASS] FFA_ID_GET returns partition ID 1\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_ID_GET\n");
            fail += 1;
        }
    }

    // Test 3: FFA_FEATURES — supported function
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx.gp_regs.x1 = ffa::FFA_VERSION;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] FFA_FEATURES(FFA_VERSION) = supported\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_FEATURES(FFA_VERSION)\n");
            fail += 1;
        }
    }

    // Test 4: FFA_FEATURES — unsupported function
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx.gp_regs.x1 = 0x84000099;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] FFA_FEATURES(unknown) = NOT_SUPPORTED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_FEATURES(unknown)\n");
            fail += 1;
        }
    }

    // Test 5: FFA_MEM_DONATE blocked
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_DONATE_32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] FFA_MEM_DONATE blocked\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_MEM_DONATE not blocked\n");
            fail += 1;
        }
    }

    // Test 6: FFA_RXTX_MAP
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_RXTX_MAP;
        ctx.gp_regs.x1 = 0x5000_0000; // TX buffer IPA (page-aligned)
        ctx.gp_regs.x2 = 0x5000_1000; // RX buffer IPA
        ctx.gp_regs.x3 = 1; // 1 page
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] FFA_RXTX_MAP success\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_RXTX_MAP\n");
            fail += 1;
        }
    }

    // Test 7: FFA_RXTX_MAP duplicate → DENIED
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_RXTX_MAP;
        ctx.gp_regs.x1 = 0x5000_2000;
        ctx.gp_regs.x2 = 0x5000_3000;
        ctx.gp_regs.x3 = 1;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] FFA_RXTX_MAP duplicate denied\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_RXTX_MAP duplicate\n");
            fail += 1;
        }
    }

    // Test 8: FFA_RXTX_UNMAP
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_RXTX_UNMAP;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] FFA_RXTX_UNMAP success\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_RXTX_UNMAP\n");
            fail += 1;
        }
    }

    // Test 9: FFA_MSG_SEND_DIRECT_REQ echo (stub SPMC only)
    // Under tfa_boot, SPMC_PRESENT=true → proxy forwards to real SPMC which
    // modifies x4 differently (SP1 adds 0x1000), so stub echo doesn't apply.
    if !cfg!(feature = "tfa_boot") {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MSG_SEND_DIRECT_REQ_32;
        // x1: sender=1 (VM0 partition ID), receiver=0x8001 (SP1)
        ctx.gp_regs.x1 = (1u64 << 16) | 0x8001;
        ctx.gp_regs.x3 = 0;
        ctx.gp_regs.x4 = 0xDEAD_BEEF;
        ctx.gp_regs.x5 = 0xCAFE_BABE;
        ctx.gp_regs.x6 = 0x1234_5678;
        ctx.gp_regs.x7 = 0x9ABC_DEF0;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont
            && ctx.gp_regs.x0 == ffa::FFA_MSG_SEND_DIRECT_RESP_32
            && ctx.gp_regs.x4 == 0xDEAD_BEEF
            && ctx.gp_regs.x5 == 0xCAFE_BABE
        {
            hypervisor::log_info!("  [PASS] FFA_MSG_SEND_DIRECT_REQ echo\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_MSG_SEND_DIRECT_REQ\n");
            fail += 1;
        }
    }

    // Test 10: FFA_MSG_SEND_DIRECT_REQ to invalid SP (stub SPMC only)
    // Under tfa_boot, forwarded to real SPMC — error path differs.
    if !cfg!(feature = "tfa_boot") {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MSG_SEND_DIRECT_REQ_32;
        ctx.gp_regs.x1 = (1u64 << 16) | 0x9999; // Invalid SP
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] Direct req to invalid SP rejected\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Direct req to invalid SP\n");
            fail += 1;
        }
    }

    // Test 11-13: MEM_SHARE/RECLAIM with SP receiver (stub SPMC only).
    // Under tfa_boot, these forward to real SPMC via SPMD — stub BL32 can't handle them.
    if !cfg!(feature = "tfa_boot") {
        // Test 11: FFA_MEM_SHARE → success with handle (register-based, no mailbox)
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
            ctx.gp_regs.x3 = 0x5000_0000; // IPA
            ctx.gp_regs.x4 = 1; // 1 page
            ctx.gp_regs.x5 = 0x8001; // SP1
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            let handle = ctx.gp_regs.x2;
            if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && handle > 0 {
                hypervisor::log_info!("  [PASS] FFA_MEM_SHARE returns handle\n");
                pass += 1;

                // Test 12: FFA_MEM_RECLAIM with valid handle
                let mut ctx2 = VcpuContext::default();
                ctx2.gp_regs.x0 = ffa::FFA_MEM_RECLAIM;
                ctx2.gp_regs.x1 = handle; // handle low
                ctx2.gp_regs.x2 = 0; // handle high
                let cont2 = ffa::proxy::handle_ffa_call(&mut ctx2);
                if cont2 && ctx2.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                    hypervisor::log_info!("  [PASS] FFA_MEM_RECLAIM success\n");
                    pass += 1;
                } else {
                    hypervisor::log_info!("  [FAIL] FFA_MEM_RECLAIM\n");
                    fail += 1;
                }
            } else {
                hypervisor::log_info!("  [FAIL] FFA_MEM_SHARE\n");
                fail += 2; // Skip reclaim test too
            }
        }

        // Test 13: FFA_MEM_RECLAIM with invalid handle
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MEM_RECLAIM;
            ctx.gp_regs.x1 = 0xDEAD; // Invalid handle
            ctx.gp_regs.x2 = 0;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
                hypervisor::log_info!("  [PASS] FFA_MEM_RECLAIM invalid handle rejected\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] FFA_MEM_RECLAIM invalid\n");
                fail += 1;
            }
        }
    }

    // ── Phase 2 tests: Descriptor parsing ─────────────────────────────

    // Test 14: Parse valid FfaMemRegion descriptor
    {
        let mut buf = [0u8; 128];
        let ranges = [(0x5000_0000u64, 2u32)];
        // SAFETY: Buffer is local and sized for descriptor build helper contract.
        let total_len = unsafe {
            ffa::descriptors::build_test_descriptor(buf.as_mut_ptr(), 1, 0x8001, &ranges)
        };
        // SAFETY: Parser reads from the same initialized descriptor bytes and bounded length.
        let parsed = unsafe { ffa::descriptors::parse_mem_region(buf.as_ptr(), total_len) };
        if let Ok(p) = parsed {
            if p.sender_id == 1
                && p.receiver_id == 0x8001
                && p.range_count == 1
                && p.ranges[0] == (0x5000_0000, 2)
                && p.total_page_count == 2
            {
                hypervisor::log_info!("  [PASS] Parse valid FfaMemRegion\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] Parse valid FfaMemRegion: wrong fields\n");
                fail += 1;
            }
        } else {
            hypervisor::log_info!("  [FAIL] Parse valid FfaMemRegion: error\n");
            fail += 1;
        }
    }

    // Test 15: Parse descriptor with multiple ranges
    {
        let mut buf = [0u8; 160];
        let ranges = [(0x5000_0000u64, 1u32), (0x6000_0000u64, 3u32)];
        // SAFETY: Buffer is local and sized for multi-range descriptor build helper.
        let total_len = unsafe {
            ffa::descriptors::build_test_descriptor(buf.as_mut_ptr(), 2, 0x8002, &ranges)
        };
        // SAFETY: Parser reads from initialized buffer with exact descriptor length.
        let parsed = unsafe { ffa::descriptors::parse_mem_region(buf.as_ptr(), total_len) };
        if let Ok(p) = parsed {
            if p.range_count == 2
                && p.ranges[0] == (0x5000_0000, 1)
                && p.ranges[1] == (0x6000_0000, 3)
                && p.total_page_count == 4
            {
                hypervisor::log_info!("  [PASS] Parse multi-range descriptor\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] Parse multi-range: wrong fields\n");
                fail += 1;
            }
        } else {
            hypervisor::log_info!("  [FAIL] Parse multi-range: error\n");
            fail += 1;
        }
    }

    // Test 16: Parse undersized descriptor → INVALID_PARAMETERS
    {
        let buf = [0u8; 16]; // Too small for FfaMemRegion (48 bytes)
                             // SAFETY: Intentionally passes undersized buffer to validate parser error handling path.
        let parsed = unsafe { ffa::descriptors::parse_mem_region(buf.as_ptr(), 16) };
        if let Err(code) = parsed {
            if code == ffa::FFA_INVALID_PARAMETERS {
                hypervisor::log_info!("  [PASS] Parse undersized -> INVALID_PARAMS\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] Parse undersized: wrong error code\n");
                fail += 1;
            }
        } else {
            hypervisor::log_info!("  [FAIL] Parse undersized: should fail\n");
            fail += 1;
        }
    }

    // ── Phase 3 tests: SMC forwarding ─────────────────────────────────

    // Test 17: forward_smc to EL3 with PSCI_VERSION returns valid response
    {
        let result = ffa::smc_forward::forward_smc(
            0x84000000, // PSCI_VERSION
            0, 0, 0, 0, 0, 0, 0,
        );
        // QEMU firmware always implements PSCI — should return version (not -1)
        if result.x0 != 0xFFFF_FFFF_FFFF_FFFF && result.x0 != 0 {
            hypervisor::log_info!(
                "  [PASS] SMC forward PSCI_VERSION returns {:#018x}\n",
                result.x0
            );
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] SMC forward PSCI_VERSION: {:#018x}\n", result.x0);
            fail += 1;
        }
    }

    // Test 18: probe_spmc — skipped in unit test mode.
    // QEMU's EL3 firmware doesn't handle FFA_VERSION SMC gracefully (crashes).
    // probe_spmc() is tested implicitly by ffa::proxy::init() at boot in linux_guest mode.

    // Test 18: Unknown FF-A call returns NOT_SUPPORTED when no SPMC
    // Under tfa_boot, unknown calls are forwarded to EL3 instead.
    if !cfg!(feature = "tfa_boot") {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = 0x8400009F; // Unknown FF-A function ID
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] Unknown FFA -> NOT_SUPPORTED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Unknown FFA call\n");
            fail += 1;
        }
    }

    // ── Phase 4 tests: VM-to-VM memory sharing ────────────────────

    // Test 19: is_valid_receiver accepts VMs and SPs
    {
        let ok_vm = ffa::is_valid_receiver(1); // VM 0 partition ID
        let ok_vm2 = ffa::is_valid_receiver(2); // VM 1 partition ID
        let ok_sp = ffa::is_valid_receiver(0x8001); // SP1
        let bad = ffa::is_valid_receiver(0x9999); // Invalid
        if ok_vm && ok_vm2 && ok_sp && !bad {
            hypervisor::log_info!("  [PASS] is_valid_receiver accepts VMs and SPs\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] is_valid_receiver\n");
            fail += 1;
        }
    }

    // Test 20: MEM_SHARE to VM1 returns handle (register-based)
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5800_0000; // IPA
        ctx.gp_regs.x4 = 1; // 1 page
        ctx.gp_regs.x5 = 2; // receiver = VM1 (partition ID 2)
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && handle > 0 {
            hypervisor::log_info!("  [PASS] MEM_SHARE to VM1 returns handle\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] MEM_SHARE to VM1\n");
            fail += 1;
        }
    }

    // MEM_LEND basic — register-based returns handle
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_LEND_32;
        ctx.gp_regs.x3 = 0x5C00_0000; // IPA (different from MEM_SHARE tests)
        ctx.gp_regs.x4 = 1; // 1 page
        ctx.gp_regs.x5 = 2; // receiver = VM1 (partition ID 2)
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && handle > 0 {
            hypervisor::log_info!("  [PASS] MEM_LEND returns handle\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] MEM_LEND\n");
            fail += 1;
        }
    }

    // Test 21: MEM_RETRIEVE_REQ by VM1 succeeds
    {
        // Share from VM0 to VM1
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5900_0000; // IPA (different from test 20)
        ctx.gp_regs.x4 = 1; // 1 page
        ctx.gp_regs.x5 = 2; // receiver = VM1
        ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

        // Switch to VM1 context
        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);

        // Retrieve as VM1
        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx2.gp_regs.x2 = handle >> 32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx2);

        // Restore VM0 context
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        if cont && ctx2.gp_regs.x0 == ffa::FFA_MEM_RETRIEVE_RESP {
            hypervisor::log_info!("  [PASS] MEM_RETRIEVE_REQ by VM1 succeeds\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] MEM_RETRIEVE_REQ by VM1\n");
            fail += 1;
        }
    }

    // Test 22: Double RETRIEVE denied
    {
        // Share from VM0 to VM1
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5A00_0000;
        ctx.gp_regs.x4 = 1;
        ctx.gp_regs.x5 = 2;
        ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

        // First retrieve as VM1
        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx2.gp_regs.x2 = handle >> 32;
        ffa::proxy::handle_ffa_call(&mut ctx2);

        // Second retrieve should fail
        let mut ctx3 = VcpuContext::default();
        ctx3.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx3.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx3.gp_regs.x2 = handle >> 32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx3);
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        if cont && ctx3.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] Double RETRIEVE denied\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Double RETRIEVE\n");
            fail += 1;
        }
    }

    // Test 23: MEM_RELINQUISH by VM1 succeeds
    {
        // Share and retrieve
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5B00_0000;
        ctx.gp_regs.x4 = 1;
        ctx.gp_regs.x5 = 2;
        ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx2.gp_regs.x2 = handle >> 32;
        ffa::proxy::handle_ffa_call(&mut ctx2);

        // Relinquish as VM1
        let mut ctx3 = VcpuContext::default();
        ctx3.gp_regs.x0 = ffa::FFA_MEM_RELINQUISH;
        ctx3.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx3.gp_regs.x2 = handle >> 32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx3);
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        if cont && ctx3.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] MEM_RELINQUISH by VM1 succeeds\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] MEM_RELINQUISH by VM1\n");
            fail += 1;
        }
    }

    // Test 24: MEM_RECLAIM after RELINQUISH succeeds
    {
        // Share, retrieve, relinquish, then reclaim
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5C00_0000;
        ctx.gp_regs.x4 = 1;
        ctx.gp_regs.x5 = 2;
        ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

        // Retrieve as VM1
        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx2.gp_regs.x2 = handle >> 32;
        ffa::proxy::handle_ffa_call(&mut ctx2);

        // Relinquish as VM1
        let mut ctx3 = VcpuContext::default();
        ctx3.gp_regs.x0 = ffa::FFA_MEM_RELINQUISH;
        ctx3.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx3.gp_regs.x2 = handle >> 32;
        ffa::proxy::handle_ffa_call(&mut ctx3);
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        // Reclaim as VM0
        let mut ctx4 = VcpuContext::default();
        ctx4.gp_regs.x0 = ffa::FFA_MEM_RECLAIM;
        ctx4.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx4.gp_regs.x2 = handle >> 32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx4);

        if cont && ctx4.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] MEM_RECLAIM after RELINQUISH succeeds\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] MEM_RECLAIM after RELINQUISH\n");
            fail += 1;
        }
    }

    // Test 25: RECLAIM while retrieved -> DENIED
    {
        // Share and retrieve (don't relinquish)
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5D00_0000;
        ctx.gp_regs.x4 = 1;
        ctx.gp_regs.x5 = 2;
        ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

        // Retrieve as VM1
        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx2.gp_regs.x2 = handle >> 32;
        ffa::proxy::handle_ffa_call(&mut ctx2);
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        // Try reclaim as VM0 while still retrieved — should fail
        let mut ctx3 = VcpuContext::default();
        ctx3.gp_regs.x0 = ffa::FFA_MEM_RECLAIM;
        ctx3.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx3.gp_regs.x2 = handle >> 32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx3);

        if cont && ctx3.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] RECLAIM while retrieved denied\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] RECLAIM while retrieved\n");
            fail += 1;
        }
    }

    // Test 26: RETRIEVE by wrong VM -> DENIED
    {
        // Share from VM0 to VM1 (receiver=2)
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5E00_0000;
        ctx.gp_regs.x4 = 1;
        ctx.gp_regs.x5 = 2; // receiver = VM1
        ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

        // Try retrieve as VM0 (caller_id=1, but receiver_id=2) — should fail
        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
        ctx2.gp_regs.x2 = handle >> 32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx2);

        if cont && ctx2.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] RETRIEVE by wrong VM denied\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] RETRIEVE by wrong VM\n");
            fail += 1;
        }
    }

    // Test 27: FEATURES reports RETRIEVE/RELINQUISH supported
    {
        let mut ok = true;

        let mut ctx1 = VcpuContext::default();
        ctx1.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx1.gp_regs.x1 = ffa::FFA_MEM_RETRIEVE_REQ_32;
        ffa::proxy::handle_ffa_call(&mut ctx1);
        if ctx1.gp_regs.x0 != ffa::FFA_SUCCESS_32 {
            ok = false;
        }

        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx2.gp_regs.x1 = ffa::FFA_MEM_RELINQUISH;
        ffa::proxy::handle_ffa_call(&mut ctx2);
        if ctx2.gp_regs.x0 != ffa::FFA_SUCCESS_32 {
            ok = false;
        }

        if ok {
            hypervisor::log_info!("  [PASS] FEATURES: RETRIEVE/RELINQUISH supported\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FEATURES: RETRIEVE/RELINQUISH\n");
            fail += 1;
        }
    }

    // ── Phase 5 tests: Supplemental calls ──────────────────────────

    // Test 28: FFA_SPM_ID_GET returns 0x8000
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_SPM_ID_GET;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && ctx.gp_regs.x2 == 0x8000 {
            hypervisor::log_info!("  [PASS] FFA_SPM_ID_GET returns 0x8000\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_SPM_ID_GET\n");
            fail += 1;
        }
    }

    // Test 29: FFA_RUN returns NOT_SUPPORTED (no real SPMC)
    // Under tfa_boot, SPMC_PRESENT=true so FFA_RUN is forwarded to real SPMC
    // which returns a scheduling result, not FFA_ERROR. Skip in that case.
    if !cfg!(feature = "tfa_boot") {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_RUN;
        ctx.gp_regs.x1 = 0x8001u64 << 16; // SP1, vCPU 0
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] FFA_RUN returns NOT_SUPPORTED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FFA_RUN\n");
            fail += 1;
        }
    }

    // Test 30: FFA_FEATURES(FFA_SPM_ID_GET) = supported
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx.gp_regs.x1 = ffa::FFA_SPM_ID_GET;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] FEATURES(SPM_ID_GET) supported\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FEATURES(SPM_ID_GET)\n");
            fail += 1;
        }
    }

    // ── Phase 6 tests: Notifications ────────────────────────────────

    // Test 31: NOTIFICATION_BITMAP_CREATE for VM0
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_BITMAP_CREATE;
        ctx.gp_regs.x1 = 1; // VM0 partition ID
        ctx.gp_regs.x2 = 1; // 1 vCPU
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] BITMAP_CREATE for VM0\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] BITMAP_CREATE\n");
            fail += 1;
        }
    }

    // Test 32: NOTIFICATION_BIND (SP1→VM0, bitmap=0x1)
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_BIND;
        ctx.gp_regs.x1 = (0x8001u64 << 16) | 1; // sender=SP1, receiver=VM0
        ctx.gp_regs.x2 = 0; // flags: global
        ctx.gp_regs.x3 = 0x1; // bitmap bit 0
        ctx.gp_regs.x4 = 0;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] NOTIFICATION_BIND SP1->VM0\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] NOTIFICATION_BIND\n");
            fail += 1;
        }
    }

    // Test 33: NOTIFICATION_SET (SP1→VM0, bitmap=0x1)
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_SET;
        ctx.gp_regs.x1 = (0x8001u64 << 16) | 1; // sender=SP1, receiver=VM0
        ctx.gp_regs.x2 = 0; // flags
        ctx.gp_regs.x3 = 0x1; // bitmap bit 0
        ctx.gp_regs.x4 = 0;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] NOTIFICATION_SET SP1->VM0\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] NOTIFICATION_SET\n");
            fail += 1;
        }
    }

    // Test 34: NOTIFICATION_GET returns pending bit
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_GET;
        ctx.gp_regs.x1 = 1; // VM0 partition ID
        ctx.gp_regs.x2 = 0x3; // flags: SP + VM
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && ctx.gp_regs.x2 == 0x1 {
            hypervisor::log_info!("  [PASS] NOTIFICATION_GET returns 0x1\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] NOTIFICATION_GET\n");
            fail += 1;
        }
    }

    // Test 35: NOTIFICATION_GET again returns 0 (cleared)
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_GET;
        ctx.gp_regs.x1 = 1; // VM0
        ctx.gp_regs.x2 = 0x3;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && ctx.gp_regs.x2 == 0 {
            hypervisor::log_info!("  [PASS] NOTIFICATION_GET cleared\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] NOTIFICATION_GET cleared\n");
            fail += 1;
        }
    }

    // Test 36: NOTIFICATION_UNBIND
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_UNBIND;
        ctx.gp_regs.x1 = (0x8001u64 << 16) | 1; // sender=SP1, receiver=VM0
        ctx.gp_regs.x3 = 0x1; // bitmap
        ctx.gp_regs.x4 = 0;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] NOTIFICATION_UNBIND\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] NOTIFICATION_UNBIND\n");
            fail += 1;
        }
    }

    // Test 37: SET after UNBIND → DENIED
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_NOTIFICATION_SET;
        ctx.gp_regs.x1 = (0x8001u64 << 16) | 1;
        ctx.gp_regs.x2 = 0;
        ctx.gp_regs.x3 = 0x1;
        ctx.gp_regs.x4 = 0;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] SET after UNBIND denied\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] SET after UNBIND\n");
            fail += 1;
        }
    }

    // Test 38: FEATURES(NOTIFICATION_BIND) = supported
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx.gp_regs.x1 = ffa::FFA_NOTIFICATION_BIND;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] FEATURES(NOTIFICATION_BIND)\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FEATURES(NOTIFICATION_BIND)\n");
            fail += 1;
        }
    }

    // ── Phase 7 tests: Indirect messaging ───────────────────────────

    // Test 39: MSG_SEND2 without mailbox → DENIED
    {
        // Ensure mailbox is unmapped (test 8 already unmapped it)
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MSG_SEND2;
        ctx.gp_regs.x1 = 0; // flags
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::log_info!("  [PASS] MSG_SEND2 no mailbox denied\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] MSG_SEND2 no mailbox\n");
            fail += 1;
        }
    }

    // Test 40-42: MSG_SEND2 from VM0→VM1, MSG_WAIT, MSG_WAIT no msg
    {
        // Set up TX/RX buffers using page-aligned arrays.
        // FFA_RXTX_MAP requires page-aligned buffers.
        #[repr(C, align(4096))]
        struct PageBuf([u8; 4096]);
        let mut tx_buf = PageBuf([0u8; 4096]);
        let mut rx_buf_vm0 = PageBuf([0u8; 4096]);
        let tx_buf_vm1 = PageBuf([0u8; 4096]);
        let mut rx_buf_vm1 = PageBuf([0u8; 4096]);

        // Map VM0 mailbox
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_RXTX_MAP;
            ctx.gp_regs.x1 = tx_buf.0.as_ptr() as u64;
            ctx.gp_regs.x2 = rx_buf_vm0.0.as_mut_ptr() as u64;
            ctx.gp_regs.x3 = 1;
            ffa::proxy::handle_ffa_call(&mut ctx);
        }

        // Map VM1 mailbox
        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_RXTX_MAP;
            ctx.gp_regs.x1 = tx_buf_vm1.0.as_ptr() as u64;
            ctx.gp_regs.x2 = rx_buf_vm1.0.as_mut_ptr() as u64;
            ctx.gp_regs.x3 = 1;
            ffa::proxy::handle_ffa_call(&mut ctx);
        }
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        // Write indirect message header in VM0's TX buffer
        // Header: sender_id(u16=1) + receiver_id(u16=2) + size(u32=4) + payload
        // SAFETY: Writes test message header/payload into local 4KB TX mailbox buffer.
        unsafe {
            core::ptr::write_unaligned(tx_buf.0.as_mut_ptr() as *mut u16, 1u16); // sender VM0
            core::ptr::write_unaligned(tx_buf.0.as_mut_ptr().add(2) as *mut u16, 2u16); // receiver VM1
            core::ptr::write_unaligned(tx_buf.0.as_mut_ptr().add(4) as *mut u32, 4u32); // payload size
            core::ptr::write_unaligned(tx_buf.0.as_mut_ptr().add(8) as *mut u32, 0xCAFE_BABE);
            // payload
        }

        // Test 40: MSG_SEND2 from VM0 to VM1
        // Test 41: MSG_WAIT by VM1 returns pending message
        // Under linux_guest, is_guest_ram() rejects stack-allocated RXTX buffers
        // (hypervisor memory, not guest RAM). Skip these tests.
        if !cfg!(feature = "linux_guest") {
            {
                let mut ctx = VcpuContext::default();
                ctx.gp_regs.x0 = ffa::FFA_MSG_SEND2;
                ctx.gp_regs.x1 = 0;
                let cont = ffa::proxy::handle_ffa_call(&mut ctx);
                if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                    hypervisor::log_info!("  [PASS] MSG_SEND2 VM0->VM1\n");
                    pass += 1;
                } else {
                    hypervisor::log_info!("  [FAIL] MSG_SEND2 VM0->VM1\n");
                    fail += 1;
                }
            }

            hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
            {
                let mut ctx = VcpuContext::default();
                ctx.gp_regs.x0 = ffa::FFA_MSG_WAIT;
                let cont = ffa::proxy::handle_ffa_call(&mut ctx);
                if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && ctx.gp_regs.x1 == 1 {
                    hypervisor::log_info!("  [PASS] MSG_WAIT returns sender=VM0\n");
                    pass += 1;
                } else {
                    hypervisor::log_info!("  [FAIL] MSG_WAIT\n");
                    fail += 1;
                }
            }

            // Release RX buffer so msg_pending clears
            {
                let mut ctx = VcpuContext::default();
                ctx.gp_regs.x0 = ffa::FFA_RX_RELEASE;
                ffa::proxy::handle_ffa_call(&mut ctx);
            }
        }

        // Test 42: MSG_WAIT with no message → NO_DATA
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MSG_WAIT;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
                hypervisor::log_info!("  [PASS] MSG_WAIT no msg -> NO_DATA\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] MSG_WAIT no msg\n");
                fail += 1;
            }
        }
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

        // Test 43: MSG_SEND2 when receiver RX busy → BUSY
        {
            // Send first message (RX now owned by VM1)
            // SAFETY: Reinitializes local TX mailbox header for busy-RX negative test.
            unsafe {
                core::ptr::write_unaligned(tx_buf.0.as_mut_ptr() as *mut u16, 1u16);
                core::ptr::write_unaligned(tx_buf.0.as_mut_ptr().add(2) as *mut u16, 2u16);
                core::ptr::write_unaligned(tx_buf.0.as_mut_ptr().add(4) as *mut u32, 4u32);
            }
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MSG_SEND2;
            ffa::proxy::handle_ffa_call(&mut ctx);

            // Second send should fail (RX busy)
            let mut ctx2 = VcpuContext::default();
            ctx2.gp_regs.x0 = ffa::FFA_MSG_SEND2;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx2);
            if cont && ctx2.gp_regs.x0 == ffa::FFA_ERROR {
                hypervisor::log_info!("  [PASS] MSG_SEND2 RX busy\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] MSG_SEND2 RX busy\n");
                fail += 1;
            }
        }

        // Test 44: FEATURES(MSG_SEND2) = supported
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_FEATURES;
            ctx.gp_regs.x1 = ffa::FFA_MSG_SEND2;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                hypervisor::log_info!("  [PASS] FEATURES(MSG_SEND2)\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] FEATURES(MSG_SEND2)\n");
                fail += 1;
            }
        }

        // Test 45-46: Fragmented MEM_SHARE + MEM_FRAG_TX
        // Under linux_guest, is_guest_ram() rejects stack-allocated RXTX buffers
        // (hypervisor memory, not guest RAM). Skip these tests.
        if !cfg!(feature = "linux_guest") {
            // Build a descriptor in a local buffer
            let mut desc_buf = [0u8; 128];
            let ranges = [(0x7000_0000u64, 1u32)];
            let total_len = unsafe {
                ffa::descriptors::build_test_descriptor(desc_buf.as_mut_ptr(), 1, 0x8001, &ranges)
            };
            let split = total_len / 2; // Split descriptor in half

            // Write first half to TX buffer
            unsafe {
                core::ptr::copy_nonoverlapping(
                    desc_buf.as_ptr(),
                    tx_buf.0.as_mut_ptr(),
                    split as usize,
                );
            }

            // MEM_SHARE with total > fragment → should return MEM_FRAG_RX
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
            ctx.gp_regs.x1 = total_len as u64; // total_length
            ctx.gp_regs.x2 = split as u64; // fragment_length (first half)
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont && ctx.gp_regs.x0 == ffa::FFA_MEM_FRAG_RX {
                let frag_handle_lo = ctx.gp_regs.x1;
                let frag_handle_hi = ctx.gp_regs.x2;
                let offset = ctx.gp_regs.x3;
                if offset == split as u64 {
                    hypervisor::log_info!("  [PASS] Fragmented MEM_SHARE returns MEM_FRAG_RX\n");
                    pass += 1;

                    // Test 46: MEM_FRAG_TX with second half → FFA_SUCCESS
                    let remaining = total_len - split;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            desc_buf.as_ptr().add(split as usize),
                            tx_buf.0.as_mut_ptr(),
                            remaining as usize,
                        );
                    }
                    let mut ctx2 = VcpuContext::default();
                    ctx2.gp_regs.x0 = ffa::FFA_MEM_FRAG_TX;
                    ctx2.gp_regs.x1 = frag_handle_lo;
                    ctx2.gp_regs.x2 = frag_handle_hi;
                    ctx2.gp_regs.x3 = remaining as u64;
                    let cont2 = ffa::proxy::handle_ffa_call(&mut ctx2);
                    if cont2 && ctx2.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                        hypervisor::log_info!("  [PASS] MEM_FRAG_TX completes share\n");
                        pass += 1;

                        // Reclaim the fragmented share
                        let mut ctx3 = VcpuContext::default();
                        ctx3.gp_regs.x0 = ffa::FFA_MEM_RECLAIM;
                        ctx3.gp_regs.x1 = ctx2.gp_regs.x2; // handle lo from SUCCESS
                        ctx3.gp_regs.x2 = ctx2.gp_regs.x3; // handle hi from SUCCESS
                        ffa::proxy::handle_ffa_call(&mut ctx3);
                    } else {
                        hypervisor::log_info!("  [FAIL] MEM_FRAG_TX: x0={:#x}\n", ctx2.gp_regs.x0);
                        fail += 1;
                    }
                } else {
                    hypervisor::log_info!("  [FAIL] MEM_FRAG_RX offset: {} != {}\n", offset, split);
                    fail += 2;
                }
            } else {
                hypervisor::log_info!("  [FAIL] Fragmented MEM_SHARE: x0={:#x}\n", ctx.gp_regs.x0);
                fail += 2;
            }
        }

        // Test 47: MEM_FRAG_TX with wrong handle → INVALID_PARAMETERS
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MEM_FRAG_TX;
            ctx.gp_regs.x1 = 0xDEAD; // invalid handle
            ctx.gp_regs.x2 = 0;
            ctx.gp_regs.x3 = 64; // fragment length
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont
                && ctx.gp_regs.x0 == ffa::FFA_ERROR
                && ctx.gp_regs.x2 == (ffa::FFA_INVALID_PARAMETERS as u32) as u64
            {
                hypervisor::log_info!("  [PASS] MEM_FRAG_TX wrong handle rejected\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] MEM_FRAG_TX wrong handle\n");
                fail += 1;
            }
        }

        // Test 48: FEATURES(MEM_FRAG_TX) = supported
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_FEATURES;
            ctx.gp_regs.x1 = ffa::FFA_MEM_FRAG_TX;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                hypervisor::log_info!("  [PASS] FEATURES(MEM_FRAG_TX)\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] FEATURES(MEM_FRAG_TX)\n");
                fail += 1;
            }
        }

        // Test 49: RETRIEVE_RESP writes descriptor to RX when mailbox mapped
        // Skip: map_page in retrieve path requires valid VTTBR_EL2, but
        // test_dynamic_pagetable leaves stale VTTBR causing map_page failure.
        // Validated via BL33 Test 13 (E2E) and pKVM ffa_test.ko instead.
        if false {
            // Temporarily unmap VM0 mailbox so MEM_SHARE uses register-based protocol
            {
                let mut ctx = VcpuContext::default();
                ctx.gp_regs.x0 = ffa::FFA_RXTX_UNMAP;
                ffa::proxy::handle_ffa_call(&mut ctx);
            }

            // Share from VM0 to VM1 (register-based, VM0 mailbox unmapped)
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
            ctx.gp_regs.x3 = 0x5D00_0000;
            ctx.gp_regs.x4 = 1;
            ctx.gp_regs.x5 = 2; // receiver = VM1 (partition_id 2)
            ffa::proxy::handle_ffa_call(&mut ctx);
            let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);

            // Re-map VM0 mailbox
            {
                let mut ctx = VcpuContext::default();
                ctx.gp_regs.x0 = ffa::FFA_RXTX_MAP;
                ctx.gp_regs.x1 = tx_buf.0.as_ptr() as u64;
                ctx.gp_regs.x2 = rx_buf_vm0.0.as_mut_ptr() as u64;
                ctx.gp_regs.x3 = 1;
                ffa::proxy::handle_ffa_call(&mut ctx);
            }

            // Retrieve as VM1 (VM1 mailbox is mapped → descriptor written to RX)
            hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
            let mut ctx2 = VcpuContext::default();
            ctx2.gp_regs.x0 = ffa::FFA_MEM_RETRIEVE_REQ_32;
            ctx2.gp_regs.x1 = handle & 0xFFFF_FFFF;
            ctx2.gp_regs.x2 = handle >> 32;
            ffa::proxy::handle_ffa_call(&mut ctx2);
            hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);

            if ctx2.gp_regs.x0 == ffa::FFA_MEM_RETRIEVE_RESP && ctx2.gp_regs.x1 > 0 {
                hypervisor::log_info!(
                    "  [PASS] RETRIEVE_RESP descriptor in RX (total_length={})\n",
                    ctx2.gp_regs.x1
                );
                pass += 1;
            } else {
                hypervisor::log_info!(
                    "  [FAIL] RETRIEVE_RESP descriptor: x0={:#x} x1={}\n",
                    ctx2.gp_regs.x0,
                    ctx2.gp_regs.x1
                );
                fail += 1;
            }
        }

        // Test 50: MEM_FRAG_RX with no active state → INVALID_PARAMETERS
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_MEM_FRAG_RX;
            ctx.gp_regs.x1 = 0xBEEF;
            ctx.gp_regs.x2 = 0;
            ctx.gp_regs.x3 = 0;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont
                && ctx.gp_regs.x0 == ffa::FFA_ERROR
                && ctx.gp_regs.x2 == (ffa::FFA_INVALID_PARAMETERS as u32) as u64
            {
                hypervisor::log_info!("  [PASS] MEM_FRAG_RX no active state rejected\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] MEM_FRAG_RX no active state\n");
                fail += 1;
            }
        }

        // Test 51: FEATURES(MEM_FRAG_RX) = supported
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_FEATURES;
            ctx.gp_regs.x1 = ffa::FFA_MEM_FRAG_RX;
            let cont = ffa::proxy::handle_ffa_call(&mut ctx);
            if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                hypervisor::log_info!("  [PASS] FEATURES(MEM_FRAG_RX)\n");
                pass += 1;
            } else {
                hypervisor::log_info!("  [FAIL] FEATURES(MEM_FRAG_RX)\n");
                fail += 1;
            }
        }

        // Cleanup: unmap both mailboxes
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_RXTX_UNMAP;
            ffa::proxy::handle_ffa_call(&mut ctx);
        }
        hypervisor::global::CURRENT_VM_ID.store(1, core::sync::atomic::Ordering::Relaxed);
        {
            let mut ctx = VcpuContext::default();
            ctx.gp_regs.x0 = ffa::FFA_RXTX_UNMAP;
            ffa::proxy::handle_ffa_call(&mut ctx);
        }
        hypervisor::global::CURRENT_VM_ID.store(0, core::sync::atomic::Ordering::Relaxed);
    }

    // Test 52: CONSOLE_LOG_32 with "Hi" → SUCCESS
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_CONSOLE_LOG_32;
        ctx.gp_regs.x1 = 2; // 2 chars
        ctx.gp_regs.x2 = 0x6948; // 'H' (0x48) + 'i' (0x69) in LE
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] CONSOLE_LOG_32 success\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] CONSOLE_LOG_32\n");
            fail += 1;
        }
    }

    // Test 53: CONSOLE_LOG with char_count=0 → INVALID_PARAMETERS
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_CONSOLE_LOG_32;
        ctx.gp_regs.x1 = 0; // invalid
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont
            && ctx.gp_regs.x0 == ffa::FFA_ERROR
            && ctx.gp_regs.x2 == (ffa::FFA_INVALID_PARAMETERS as u32) as u64
        {
            hypervisor::log_info!("  [PASS] CONSOLE_LOG count=0 rejected\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] CONSOLE_LOG count=0\n");
            fail += 1;
        }
    }

    // Test 54: FEATURES(CONSOLE_LOG_32) = supported
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx.gp_regs.x1 = ffa::FFA_CONSOLE_LOG_32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::log_info!("  [PASS] FEATURES(CONSOLE_LOG_32)\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] FEATURES(CONSOLE_LOG_32)\n");
            fail += 1;
        }
    }

    hypervisor::log_info!("  Results: {} passed, {} failed\n", pass, fail);
    assert!(fail == 0, "FF-A proxy tests failed");
}
