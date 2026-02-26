//! VM state isolation tests
//!
//! Verifies that VmGlobalState instances for different VMs
//! are independent (no cross-contamination of SGIs/SPIs).

use core::sync::atomic::Ordering;
use hypervisor::global::{vm_state, MAX_VCPUS};

pub fn run_vm_state_isolation_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  VM State Isolation Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Clean up both VMs' state
    let vs0 = vm_state(0);
    let vs1 = vm_state(1);
    for i in 0..MAX_VCPUS {
        vs0.pending_sgis[i].store(0, Ordering::Relaxed);
        vs0.pending_spis[i].store(0, Ordering::Relaxed);
        vs1.pending_sgis[i].store(0, Ordering::Relaxed);
        vs1.pending_spis[i].store(0, Ordering::Relaxed);
    }

    // Test 1: Set SGI on VM 0, verify VM 1 unaffected
    hypervisor::log_info!("[VM-ISO] Test 1: SGI isolation...\n");
    vs0.pending_sgis[0].store(0xFF, Ordering::Release);
    let vm1_sgi = vs1.pending_sgis[0].load(Ordering::Acquire);
    if vm1_sgi != 0 {
        hypervisor::log_info!("[VM-ISO] FAILED: VM 1 SGI contaminated\n");
        return;
    }
    hypervisor::log_info!("[VM-ISO] Test 1 PASSED\n\n");

    // Test 2: Set SPI on VM 1, verify VM 0 unaffected
    hypervisor::log_info!("[VM-ISO] Test 2: SPI isolation...\n");
    vs1.pending_spis[0].store(0xABCD, Ordering::Release);
    let vm0_spi = vs0.pending_spis[0].load(Ordering::Acquire);
    if vm0_spi != 0 {
        hypervisor::log_info!("[VM-ISO] FAILED: VM 0 SPI contaminated\n");
        return;
    }
    hypervisor::log_info!("[VM-ISO] Test 2 PASSED\n\n");

    // Test 3: vcpu_online_mask independent
    hypervisor::log_info!("[VM-ISO] Test 3: online mask isolation...\n");
    vs0.vcpu_online_mask.store(0b1111, Ordering::Release);
    vs1.vcpu_online_mask.store(0b0001, Ordering::Release);
    let mask0 = vs0.vcpu_online_mask.load(Ordering::Acquire);
    let mask1 = vs1.vcpu_online_mask.load(Ordering::Acquire);
    if mask0 != 0b1111 || mask1 != 0b0001 {
        hypervisor::log_info!("[VM-ISO] FAILED: online masks not independent\n");
        return;
    }
    hypervisor::log_info!("[VM-ISO] Test 3 PASSED\n\n");

    // Test 4: current_vcpu_id independent
    hypervisor::log_info!("[VM-ISO] Test 4: current_vcpu_id isolation...\n");
    vs0.current_vcpu_id.store(3, Ordering::Release);
    vs1.current_vcpu_id.store(0, Ordering::Release);
    let id0 = vs0.current_vcpu_id.load(Ordering::Acquire);
    let id1 = vs1.current_vcpu_id.load(Ordering::Acquire);
    if id0 != 3 || id1 != 0 {
        hypervisor::log_info!("[VM-ISO] FAILED: current_vcpu_id not independent\n");
        return;
    }
    hypervisor::log_info!("[VM-ISO] Test 4 PASSED\n\n");

    // Clean up
    for i in 0..MAX_VCPUS {
        vs0.pending_sgis[i].store(0, Ordering::Relaxed);
        vs0.pending_spis[i].store(0, Ordering::Relaxed);
        vs1.pending_sgis[i].store(0, Ordering::Relaxed);
        vs1.pending_spis[i].store(0, Ordering::Relaxed);
    }
    vs0.vcpu_online_mask.store(0, Ordering::Relaxed);
    vs1.vcpu_online_mask.store(0, Ordering::Relaxed);
    vs0.current_vcpu_id.store(0, Ordering::Relaxed);
    vs1.current_vcpu_id.store(0, Ordering::Relaxed);

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  VM State Isolation Test PASSED (4 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}
