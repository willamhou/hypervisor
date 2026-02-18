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

    // Test 1: FFA_VERSION returns v1.1
    {
        let mut ctx = VcpuContext::default();
        ctx.gp_regs.x0 = ffa::FFA_VERSION;
        ctx.gp_regs.x1 = ffa::FFA_VERSION_1_1 as u64; // caller version
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
        // In test mode (no VM running), current_vm_id() returns 0
        // So partition ID = vm_id_to_partition_id(0) = 1
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
        ctx.gp_regs.x1 = ffa::FFA_VERSION; // Query FFA_VERSION support
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
        ctx.gp_regs.x1 = 0x84000099; // Unknown function
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

    hypervisor::uart_puts(b"  Results: ");
    hypervisor::uart_put_u64(pass);
    hypervisor::uart_puts(b" passed, ");
    hypervisor::uart_put_u64(fail);
    hypervisor::uart_puts(b" failed\n");
    assert!(fail == 0, "FF-A proxy tests failed");
}
