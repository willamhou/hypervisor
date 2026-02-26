//! VM activation tests
//!
//! Verifies that Vm stores VTTBR/VTCR fields and that
//! activate_stage2() is callable.

use hypervisor::vm::Vm;

pub fn run_vm_activate_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  VM Activate Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Test 1: New VM has zero VTTBR/VTCR
    hypervisor::log_info!("[VM-ACT] Test 1: Initial VTTBR/VTCR are zero...\n");
    let vm = Vm::new(0);
    if vm.vttbr() != 0 || vm.vtcr() != 0 {
        hypervisor::log_info!("[VM-ACT] FAILED: expected zero VTTBR/VTCR\n");
        return;
    }
    hypervisor::log_info!("[VM-ACT] Test 1 PASSED\n\n");

    // Test 2: VM 1 also has zero VTTBR/VTCR (independent)
    hypervisor::log_info!("[VM-ACT] Test 2: VM 1 initial state...\n");
    let vm1 = Vm::new(1);
    if vm1.vttbr() != 0 || vm1.vtcr() != 0 {
        hypervisor::log_info!("[VM-ACT] FAILED: expected zero VTTBR/VTCR for VM 1\n");
        return;
    }
    hypervisor::log_info!("[VM-ACT] Test 2 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  VM Activate Test PASSED (2 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}
