//! Scheduler tests

use hypervisor::scheduler::Scheduler;

pub fn run_scheduler_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Scheduler Test\n");
    hypervisor::log_info!("========================================\n\n");

    let mut sched = Scheduler::new();

    // Test 1: Add vCPUs and pick first
    hypervisor::log_info!("[SCHED] Test 1: Add vCPUs and pick first...\n");
    sched.add_vcpu(0);
    sched.add_vcpu(1);
    sched.add_vcpu(2);

    let first = sched.pick_next();
    if first != Some(0) {
        hypervisor::log_info!("[SCHED] ERROR: First pick should be vCPU 0\n");
        return;
    }
    hypervisor::log_info!("[SCHED] Test 1 PASSED\n\n");

    // Test 2: Round-robin after yield
    hypervisor::log_info!("[SCHED] Test 2: Round-robin...\n");
    sched.yield_current();
    let second = sched.pick_next();
    if second != Some(1) {
        hypervisor::log_info!("[SCHED] ERROR: After yield should be vCPU 1\n");
        return;
    }
    hypervisor::log_info!("[SCHED] Test 2 PASSED\n\n");

    // Test 3: Wrap-around
    hypervisor::log_info!("[SCHED] Test 3: Wrap-around...\n");
    sched.yield_current();
    let third = sched.pick_next();
    if third != Some(2) {
        hypervisor::log_info!("[SCHED] ERROR: Should be vCPU 2\n");
        return;
    }
    sched.yield_current();
    let wrapped = sched.pick_next();
    if wrapped != Some(0) {
        hypervisor::log_info!("[SCHED] ERROR: Should wrap to vCPU 0\n");
        return;
    }
    hypervisor::log_info!("[SCHED] Test 3 PASSED\n\n");

    // Test 4: Block and unblock
    hypervisor::log_info!("[SCHED] Test 4: Block/unblock...\n");
    sched.block_current(); // Block vCPU 0
    let after_block = sched.pick_next();
    if after_block != Some(1) {
        hypervisor::log_info!("[SCHED] ERROR: After block should be vCPU 1\n");
        return;
    }

    // Unblock vCPU 0
    sched.unblock(0);
    sched.yield_current(); // Yield vCPU 1
    sched.pick_next(); // Should get vCPU 2
    sched.yield_current();
    let unblocked = sched.pick_next();
    if unblocked != Some(0) {
        hypervisor::log_info!("[SCHED] ERROR: Unblocked vCPU 0 should be picked\n");
        return;
    }
    hypervisor::log_info!("[SCHED] Test 4 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Scheduler Test PASSED\n");
    hypervisor::log_info!("========================================\n\n");
}
