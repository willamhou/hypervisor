//! FF-A proxy unit tests
//!
//! Tests FF-A function dispatching using direct function calls
//! (not actual SMC — we test the proxy logic, not the trap path).

use hypervisor::arch::aarch64::regs::VcpuContext;
use hypervisor::ffa;

pub fn run_ffa_test() {
    hypervisor::uart_puts(b"\n=== Test: FF-A Proxy ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    // Clear VTTBR_EL2 to avoid stale page tables from earlier VM tests.
    // Earlier tests (test_mmio, test_simple_guest) create VMs that set VTTBR
    // to their own Stage-2 tables. The MEM_SHARE handler checks has_stage2()
    // and would attempt ownership validation against those incomplete tables.
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
            hypervisor::uart_puts(b"  [PASS] FFA_VERSION returns 0x00010001\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_VERSION\n");
            fail += 1;
        }
    }

    // Test 2: FFA_ID_GET returns partition ID
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_ID_GET;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && ctx.gp_regs.x2 == 1 {
            hypervisor::uart_puts(b"  [PASS] FFA_ID_GET returns partition ID 1\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_ID_GET\n");
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
            hypervisor::uart_puts(b"  [PASS] FFA_FEATURES(FFA_VERSION) = supported\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_FEATURES(FFA_VERSION)\n");
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
            hypervisor::uart_puts(b"  [PASS] FFA_FEATURES(unknown) = NOT_SUPPORTED\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_FEATURES(unknown)\n");
            fail += 1;
        }
    }

    // Test 5: FFA_MEM_DONATE blocked
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_DONATE_32;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::uart_puts(b"  [PASS] FFA_MEM_DONATE blocked\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_MEM_DONATE not blocked\n");
            fail += 1;
        }
    }

    // Test 6: FFA_RXTX_MAP
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_RXTX_MAP;
        ctx.gp_regs.x1 = 0x5000_0000; // TX buffer IPA (page-aligned)
        ctx.gp_regs.x2 = 0x5000_1000; // RX buffer IPA
        ctx.gp_regs.x3 = 1;           // 1 page
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::uart_puts(b"  [PASS] FFA_RXTX_MAP success\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_RXTX_MAP\n");
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
            hypervisor::uart_puts(b"  [PASS] FFA_RXTX_MAP duplicate denied\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_RXTX_MAP duplicate\n");
            fail += 1;
        }
    }

    // Test 8: FFA_RXTX_UNMAP
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_RXTX_UNMAP;
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
            hypervisor::uart_puts(b"  [PASS] FFA_RXTX_UNMAP success\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_RXTX_UNMAP\n");
            fail += 1;
        }
    }

    // Test 9: FFA_MSG_SEND_DIRECT_REQ echo
    {
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
            hypervisor::uart_puts(b"  [PASS] FFA_MSG_SEND_DIRECT_REQ echo\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_MSG_SEND_DIRECT_REQ\n");
            fail += 1;
        }
    }

    // Test 10: FFA_MSG_SEND_DIRECT_REQ to invalid SP
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MSG_SEND_DIRECT_REQ_32;
        ctx.gp_regs.x1 = (1u64 << 16) | 0x9999; // Invalid SP
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::uart_puts(b"  [PASS] Direct req to invalid SP rejected\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Direct req to invalid SP\n");
            fail += 1;
        }
    }

    // Test 11: FFA_MEM_SHARE → success with handle (register-based, no mailbox)
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5000_0000; // IPA
        ctx.gp_regs.x4 = 1;           // 1 page
        ctx.gp_regs.x5 = 0x8001;      // SP1
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2;
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && handle > 0 {
            hypervisor::uart_puts(b"  [PASS] FFA_MEM_SHARE returns handle\n");
            pass += 1;

            // Test 12: FFA_MEM_RECLAIM with valid handle
            let mut ctx2 = VcpuContext::default();
            ctx2.gp_regs.x0 = ffa::FFA_MEM_RECLAIM;
            ctx2.gp_regs.x1 = handle; // handle low
            ctx2.gp_regs.x2 = 0;      // handle high
            let cont2 = ffa::proxy::handle_ffa_call(&mut ctx2);
            if cont2 && ctx2.gp_regs.x0 == ffa::FFA_SUCCESS_32 {
                hypervisor::uart_puts(b"  [PASS] FFA_MEM_RECLAIM success\n");
                pass += 1;
            } else {
                hypervisor::uart_puts(b"  [FAIL] FFA_MEM_RECLAIM\n");
                fail += 1;
            }
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_MEM_SHARE\n");
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
            hypervisor::uart_puts(b"  [PASS] FFA_MEM_RECLAIM invalid handle rejected\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FFA_MEM_RECLAIM invalid\n");
            fail += 1;
        }
    }

    // ── Phase 2 tests: Descriptor parsing ─────────────────────────────

    // Test 14: Parse valid FfaMemRegion descriptor
    {
        let mut buf = [0u8; 128];
        let ranges = [(0x5000_0000u64, 2u32)];
        let total_len = unsafe {
            ffa::descriptors::build_test_descriptor(
                buf.as_mut_ptr(), 1, 0x8001, &ranges,
            )
        };
        let parsed = unsafe {
            ffa::descriptors::parse_mem_region(buf.as_ptr(), total_len)
        };
        if let Ok(p) = parsed {
            if p.sender_id == 1
                && p.receiver_id == 0x8001
                && p.range_count == 1
                && p.ranges[0] == (0x5000_0000, 2)
                && p.total_page_count == 2
            {
                hypervisor::uart_puts(b"  [PASS] Parse valid FfaMemRegion\n");
                pass += 1;
            } else {
                hypervisor::uart_puts(b"  [FAIL] Parse valid FfaMemRegion: wrong fields\n");
                fail += 1;
            }
        } else {
            hypervisor::uart_puts(b"  [FAIL] Parse valid FfaMemRegion: error\n");
            fail += 1;
        }
    }

    // Test 15: Parse descriptor with multiple ranges
    {
        let mut buf = [0u8; 160];
        let ranges = [(0x5000_0000u64, 1u32), (0x6000_0000u64, 3u32)];
        let total_len = unsafe {
            ffa::descriptors::build_test_descriptor(
                buf.as_mut_ptr(), 2, 0x8002, &ranges,
            )
        };
        let parsed = unsafe {
            ffa::descriptors::parse_mem_region(buf.as_ptr(), total_len)
        };
        if let Ok(p) = parsed {
            if p.range_count == 2
                && p.ranges[0] == (0x5000_0000, 1)
                && p.ranges[1] == (0x6000_0000, 3)
                && p.total_page_count == 4
            {
                hypervisor::uart_puts(b"  [PASS] Parse multi-range descriptor\n");
                pass += 1;
            } else {
                hypervisor::uart_puts(b"  [FAIL] Parse multi-range: wrong fields\n");
                fail += 1;
            }
        } else {
            hypervisor::uart_puts(b"  [FAIL] Parse multi-range: error\n");
            fail += 1;
        }
    }

    // Test 16: Parse undersized descriptor → INVALID_PARAMETERS
    {
        let buf = [0u8; 16]; // Too small for FfaMemRegion (48 bytes)
        let parsed = unsafe {
            ffa::descriptors::parse_mem_region(buf.as_ptr(), 16)
        };
        if let Err(code) = parsed {
            if code == ffa::FFA_INVALID_PARAMETERS {
                hypervisor::uart_puts(b"  [PASS] Parse undersized -> INVALID_PARAMS\n");
                pass += 1;
            } else {
                hypervisor::uart_puts(b"  [FAIL] Parse undersized: wrong error code\n");
                fail += 1;
            }
        } else {
            hypervisor::uart_puts(b"  [FAIL] Parse undersized: should fail\n");
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
            hypervisor::uart_puts(b"  [PASS] SMC forward PSCI_VERSION returns ");
            hypervisor::uart_put_hex(result.x0);
            hypervisor::uart_puts(b"\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] SMC forward PSCI_VERSION: ");
            hypervisor::uart_put_hex(result.x0);
            hypervisor::uart_puts(b"\n");
            fail += 1;
        }
    }

    // Test 18: probe_spmc — skipped in unit test mode.
    // QEMU's EL3 firmware doesn't handle FFA_VERSION SMC gracefully (crashes).
    // probe_spmc() is tested implicitly by ffa::proxy::init() at boot in linux_guest mode.

    // Test 18: Unknown FF-A call returns NOT_SUPPORTED when no SPMC
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = 0x8400009F; // Unknown FF-A function ID
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        if cont && ctx.gp_regs.x0 == ffa::FFA_ERROR {
            hypervisor::uart_puts(b"  [PASS] Unknown FFA -> NOT_SUPPORTED\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Unknown FFA call\n");
            fail += 1;
        }
    }

    // ── Phase 4 tests: VM-to-VM memory sharing ────────────────────

    // Test 19: is_valid_receiver accepts VMs and SPs
    {
        let ok_vm = ffa::is_valid_receiver(1);      // VM 0 partition ID
        let ok_vm2 = ffa::is_valid_receiver(2);     // VM 1 partition ID
        let ok_sp = ffa::is_valid_receiver(0x8001);  // SP1
        let bad = ffa::is_valid_receiver(0x9999);    // Invalid
        if ok_vm && ok_vm2 && ok_sp && !bad {
            hypervisor::uart_puts(b"  [PASS] is_valid_receiver accepts VMs and SPs\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] is_valid_receiver\n");
            fail += 1;
        }
    }

    // Test 20: MEM_SHARE to VM1 returns handle (register-based)
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5800_0000; // IPA
        ctx.gp_regs.x4 = 1;           // 1 page
        ctx.gp_regs.x5 = 2;           // receiver = VM1 (partition ID 2)
        let cont = ffa::proxy::handle_ffa_call(&mut ctx);
        let handle = ctx.gp_regs.x2 | (ctx.gp_regs.x3 << 32);
        if cont && ctx.gp_regs.x0 == ffa::FFA_SUCCESS_32 && handle > 0 {
            hypervisor::uart_puts(b"  [PASS] MEM_SHARE to VM1 returns handle\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] MEM_SHARE to VM1\n");
            fail += 1;
        }
    }

    // Test 21: MEM_RETRIEVE_REQ by VM1 succeeds
    {
        // Share from VM0 to VM1
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_MEM_SHARE_32;
        ctx.gp_regs.x3 = 0x5900_0000; // IPA (different from test 20)
        ctx.gp_regs.x4 = 1;           // 1 page
        ctx.gp_regs.x5 = 2;           // receiver = VM1
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
            hypervisor::uart_puts(b"  [PASS] MEM_RETRIEVE_REQ by VM1 succeeds\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] MEM_RETRIEVE_REQ by VM1\n");
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
            hypervisor::uart_puts(b"  [PASS] Double RETRIEVE denied\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Double RETRIEVE\n");
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
            hypervisor::uart_puts(b"  [PASS] MEM_RELINQUISH by VM1 succeeds\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] MEM_RELINQUISH by VM1\n");
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
            hypervisor::uart_puts(b"  [PASS] MEM_RECLAIM after RELINQUISH succeeds\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] MEM_RECLAIM after RELINQUISH\n");
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
            hypervisor::uart_puts(b"  [PASS] RECLAIM while retrieved denied\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] RECLAIM while retrieved\n");
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
            hypervisor::uart_puts(b"  [PASS] RETRIEVE by wrong VM denied\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] RETRIEVE by wrong VM\n");
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
        if ctx1.gp_regs.x0 != ffa::FFA_SUCCESS_32 { ok = false; }

        let mut ctx2 = VcpuContext::default();
        ctx2.gp_regs.x0 = ffa::FFA_FEATURES;
        ctx2.gp_regs.x1 = ffa::FFA_MEM_RELINQUISH;
        ffa::proxy::handle_ffa_call(&mut ctx2);
        if ctx2.gp_regs.x0 != ffa::FFA_SUCCESS_32 { ok = false; }

        if ok {
            hypervisor::uart_puts(b"  [PASS] FEATURES: RETRIEVE/RELINQUISH supported\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] FEATURES: RETRIEVE/RELINQUISH\n");
            fail += 1;
        }
    }

    hypervisor::uart_puts(b"  Results: ");
    hypervisor::uart_put_u64(pass);
    hypervisor::uart_puts(b" passed, ");
    hypervisor::uart_put_u64(fail);
    hypervisor::uart_puts(b" failed\n");
    assert!(fail == 0, "FF-A proxy tests failed");
}
