//! Scheduler tests

use hypervisor::scheduler::Scheduler;
use hypervisor::uart_puts;

pub fn run_scheduler_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Scheduler Test\n");
    uart_puts(b"========================================\n\n");

    let mut sched = Scheduler::new();

    // Test 1: Add vCPUs and pick first
    uart_puts(b"[SCHED] Test 1: Add vCPUs and pick first...\n");
    sched.add_vcpu(0);
    sched.add_vcpu(1);
    sched.add_vcpu(2);

    let first = sched.pick_next();
    if first != Some(0) {
        uart_puts(b"[SCHED] ERROR: First pick should be vCPU 0\n");
        return;
    }
    uart_puts(b"[SCHED] Test 1 PASSED\n\n");

    // Test 2: Round-robin after yield
    uart_puts(b"[SCHED] Test 2: Round-robin...\n");
    sched.yield_current();
    let second = sched.pick_next();
    if second != Some(1) {
        uart_puts(b"[SCHED] ERROR: After yield should be vCPU 1\n");
        return;
    }
    uart_puts(b"[SCHED] Test 2 PASSED\n\n");

    // Test 3: Wrap-around
    uart_puts(b"[SCHED] Test 3: Wrap-around...\n");
    sched.yield_current();
    let third = sched.pick_next();
    if third != Some(2) {
        uart_puts(b"[SCHED] ERROR: Should be vCPU 2\n");
        return;
    }
    sched.yield_current();
    let wrapped = sched.pick_next();
    if wrapped != Some(0) {
        uart_puts(b"[SCHED] ERROR: Should wrap to vCPU 0\n");
        return;
    }
    uart_puts(b"[SCHED] Test 3 PASSED\n\n");

    // Test 4: Block and unblock
    uart_puts(b"[SCHED] Test 4: Block/unblock...\n");
    sched.block_current(); // Block vCPU 0
    let after_block = sched.pick_next();
    if after_block != Some(1) {
        uart_puts(b"[SCHED] ERROR: After block should be vCPU 1\n");
        return;
    }

    // Unblock vCPU 0
    sched.unblock(0);
    sched.yield_current(); // Yield vCPU 1
    sched.pick_next(); // Should get vCPU 2
    sched.yield_current();
    let unblocked = sched.pick_next();
    if unblocked != Some(0) {
        uart_puts(b"[SCHED] ERROR: Unblocked vCPU 0 should be picked\n");
        return;
    }
    uart_puts(b"[SCHED] Test 4 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Scheduler Test PASSED\n");
    uart_puts(b"========================================\n\n");
}
