//! VM with Scheduler integration tests

use hypervisor::vm::Vm;

pub fn run_vm_scheduler_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  VM Scheduler Integration Test\n");
    hypervisor::log_info!("========================================\n\n");

    let mut vm = Vm::new(0);

    // Test 1: Create vCPUs and schedule
    hypervisor::log_info!("[VM SCHED] Test 1: Create and schedule...\n");
    vm.create_vcpu(0).unwrap();
    vm.create_vcpu(1).unwrap();

    let next = vm.schedule();
    if next != Some(0) {
        hypervisor::log_info!("[VM SCHED] ERROR: First schedule should be vCPU 0\n");
        return;
    }
    hypervisor::log_info!("[VM SCHED] Test 1 PASSED\n\n");

    // Test 2: Yield and reschedule
    hypervisor::log_info!("[VM SCHED] Test 2: Yield and reschedule...\n");
    vm.yield_current();
    let next = vm.schedule();
    if next != Some(1) {
        hypervisor::log_info!("[VM SCHED] ERROR: After yield should be vCPU 1\n");
        return;
    }
    hypervisor::log_info!("[VM SCHED] Test 2 PASSED\n\n");

    // Test 3: Block current and reschedule
    hypervisor::log_info!("[VM SCHED] Test 3: Block and reschedule...\n");
    vm.block_current();
    let next = vm.schedule();
    if next != Some(0) {
        hypervisor::log_info!("[VM SCHED] ERROR: After block should be vCPU 0\n");
        return;
    }
    hypervisor::log_info!("[VM SCHED] Test 3 PASSED\n\n");

    // Test 4: Unblock and check availability
    hypervisor::log_info!("[VM SCHED] Test 4: Unblock...\n");
    vm.unblock(1);
    vm.yield_current();
    let next = vm.schedule();
    if next != Some(1) {
        hypervisor::log_info!("[VM SCHED] ERROR: After unblock, vCPU 1 should be available\n");
        return;
    }
    hypervisor::log_info!("[VM SCHED] Test 4 PASSED\n\n");

    // Test 5: Mark done
    hypervisor::log_info!("[VM SCHED] Test 5: Mark done...\n");
    vm.mark_current_done(); // Remove vCPU 1
    let next = vm.schedule();
    if next != Some(0) {
        hypervisor::log_info!("[VM SCHED] ERROR: After mark done, only vCPU 0 should be left\n");
        return;
    }
    hypervisor::log_info!("[VM SCHED] Test 5 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  VM Scheduler Integration Test PASSED\n");
    hypervisor::log_info!("========================================\n\n");
}
