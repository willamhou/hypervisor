//! Multi-vCPU support tests

use hypervisor::uart_puts;
use hypervisor::vm::Vm;

pub fn run_multi_vcpu_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Multi-vCPU Support Test\n");
    uart_puts(b"========================================\n\n");

    // Test 1: Create multiple vCPUs
    uart_puts(b"[MULTI] Test 1: Create multiple vCPUs...\n");
    let mut vm = Vm::new(0);

    let vcpu0 = vm.create_vcpu(0);
    if vcpu0.is_err() {
        uart_puts(b"[MULTI] ERROR: Failed to create vCPU 0\n");
        return;
    }
    uart_puts(b"[MULTI] vCPU 0 created\n");

    let vcpu1 = vm.create_vcpu(1);
    if vcpu1.is_err() {
        uart_puts(b"[MULTI] ERROR: Failed to create vCPU 1\n");
        return;
    }
    uart_puts(b"[MULTI] vCPU 1 created\n");
    uart_puts(b"[MULTI] Test 1 PASSED\n\n");

    // Test 2: Each vCPU has independent state
    uart_puts(b"[MULTI] Test 2: vCPU state independence...\n");
    {
        let v0 = vm.vcpu_mut(0).unwrap();
        v0.context_mut().set_gpr(0, 0x1111);
    }
    {
        let v1 = vm.vcpu_mut(1).unwrap();
        v1.context_mut().set_gpr(0, 0x2222);
    }

    let x0_v0 = vm.vcpu(0).unwrap().context().get_gpr(0);
    let x0_v1 = vm.vcpu(1).unwrap().context().get_gpr(0);

    if x0_v0 != 0x1111 {
        uart_puts(b"[MULTI] ERROR: vCPU 0 x0 != 0x1111\n");
        return;
    }
    if x0_v1 != 0x2222 {
        uart_puts(b"[MULTI] ERROR: vCPU 1 x0 != 0x2222\n");
        return;
    }
    uart_puts(b"[MULTI] vCPU state independence verified\n");
    uart_puts(b"[MULTI] Test 2 PASSED\n\n");

    // Test 3: vCPU count tracking
    uart_puts(b"[MULTI] Test 3: vCPU count...\n");
    if vm.vcpu_count() != 2 {
        uart_puts(b"[MULTI] ERROR: vcpu_count != 2\n");
        return;
    }
    uart_puts(b"[MULTI] Test 3 PASSED\n\n");

    // Test 4: Cannot create duplicate vCPU
    uart_puts(b"[MULTI] Test 4: Duplicate vCPU rejection...\n");
    let duplicate = vm.create_vcpu(0);
    if duplicate.is_ok() {
        uart_puts(b"[MULTI] ERROR: Should reject duplicate vCPU ID\n");
        return;
    }
    uart_puts(b"[MULTI] Duplicate vCPU correctly rejected\n");
    uart_puts(b"[MULTI] Test 4 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Multi-vCPU Support Test PASSED\n");
    uart_puts(b"========================================\n\n");
}
