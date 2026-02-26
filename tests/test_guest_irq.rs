//! Interrupt queueing tests
//!
//! Tests PENDING_SGIS and PENDING_SPIS atomic bitmask operations
//! used for cross-vCPU interrupt delivery.

use core::sync::atomic::Ordering;
use hypervisor::global::{vm_state, MAX_VCPUS};

pub fn run_irq_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Interrupt Queue Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Reset state
    for i in 0..MAX_VCPUS {
        vm_state(0).pending_sgis[i].store(0, Ordering::Relaxed);
        vm_state(0).pending_spis[i].store(0, Ordering::Relaxed);
    }

    // Test 1: Queue SGI 1 to vCPU 2
    hypervisor::log_info!("[IRQ Q] Test 1: Queue SGI...\n");
    vm_state(0).pending_sgis[2].fetch_or(1 << 1, Ordering::Release);
    let pending = vm_state(0).pending_sgis[2].load(Ordering::Acquire);
    if pending != 0x02 {
        hypervisor::log_info!("[IRQ Q] FAILED: SGI bit not set\n");
        return;
    }
    hypervisor::log_info!("[IRQ Q] Test 1 PASSED\n\n");

    // Test 2: Queue multiple SGIs
    hypervisor::log_info!("[IRQ Q] Test 2: Multiple SGIs...\n");
    vm_state(0).pending_sgis[2].fetch_or(1 << 0, Ordering::Release); // SGI 0
    vm_state(0).pending_sgis[2].fetch_or(1 << 7, Ordering::Release); // SGI 7
    let pending = vm_state(0).pending_sgis[2].load(Ordering::Acquire);
    if pending != 0x83 {
        // bits 0,1,7
        hypervisor::log_info!("[IRQ Q] FAILED: expected 0x83\n");
        return;
    }
    hypervisor::log_info!("[IRQ Q] Test 2 PASSED\n\n");

    // Test 3: Consume SGIs via swap
    hypervisor::log_info!("[IRQ Q] Test 3: Consume SGIs...\n");
    let consumed = vm_state(0).pending_sgis[2].swap(0, Ordering::AcqRel);
    if consumed != 0x83 {
        hypervisor::log_info!("[IRQ Q] FAILED: swap should return 0x83\n");
        return;
    }
    let after = vm_state(0).pending_sgis[2].load(Ordering::Acquire);
    if after != 0 {
        hypervisor::log_info!("[IRQ Q] FAILED: should be 0 after swap\n");
        return;
    }
    hypervisor::log_info!("[IRQ Q] Test 3 PASSED\n\n");

    // Test 4: Queue SPI — bit encoding (INTID 48 = bit 16)
    hypervisor::log_info!("[IRQ Q] Test 4: Queue SPI...\n");
    let spi_bit = 48u32 - 32; // bit 16
    vm_state(0).pending_spis[0].fetch_or(1 << spi_bit, Ordering::Release);
    let pending = vm_state(0).pending_spis[0].load(Ordering::Acquire);
    if pending != (1 << 16) {
        hypervisor::log_info!("[IRQ Q] FAILED: SPI bit not set\n");
        return;
    }
    hypervisor::log_info!("[IRQ Q] Test 4 PASSED\n\n");

    // Test 5: vCPU isolation — vCPU 1 unaffected
    hypervisor::log_info!("[IRQ Q] Test 5: vCPU isolation...\n");
    let vcpu1_sgis = vm_state(0).pending_sgis[1].load(Ordering::Acquire);
    let vcpu1_spis = vm_state(0).pending_spis[1].load(Ordering::Acquire);
    if vcpu1_sgis != 0 || vcpu1_spis != 0 {
        hypervisor::log_info!("[IRQ Q] FAILED: vCPU 1 should have no pending\n");
        return;
    }
    hypervisor::log_info!("[IRQ Q] Test 5 PASSED\n\n");

    // Clean up
    for i in 0..MAX_VCPUS {
        vm_state(0).pending_sgis[i].store(0, Ordering::Relaxed);
        vm_state(0).pending_spis[i].store(0, Ordering::Relaxed);
    }

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Interrupt Queue Test PASSED (5 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}
