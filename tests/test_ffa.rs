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

    hypervisor::uart_puts(b"  Results: ");
    hypervisor::uart_put_u64(pass);
    hypervisor::uart_puts(b" passed, ");
    hypervisor::uart_put_u64(fail);
    hypervisor::uart_puts(b" failed\n");
    assert!(fail == 0, "FF-A proxy tests failed");
}
