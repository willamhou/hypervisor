//! VMID/VTTBR encoding tests
//!
//! Verifies that Stage2Config::new_with_vmid correctly encodes
//! VMID in VTTBR_EL2 bits [63:48].

use hypervisor::arch::aarch64::mm::mmu::Stage2Config;

pub fn run_vmid_vttbr_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  VMID/VTTBR Encoding Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Test 1: VMID 0 — bits [63:48] should be 0
    hypervisor::log_info!("[VMID] Test 1: VMID 0 encoding...\n");
    let config0 = Stage2Config::new_with_vmid(0x4100_0000, 0);
    let vmid_bits = config0.vttbr >> 48;
    if vmid_bits != 0 {
        hypervisor::log_info!("[VMID] FAILED: expected VMID 0, got {:#018x}\n", vmid_bits);
        return;
    }
    // Verify page table base is preserved
    let base = config0.vttbr & 0x0000_FFFF_FFFF_FFFE;
    if base != 0x4100_0000 {
        hypervisor::log_info!("[VMID] FAILED: base address corrupted\n");
        return;
    }
    hypervisor::log_info!("[VMID] Test 1 PASSED\n\n");

    // Test 2: VMID 1 — bits [63:48] should be 1
    hypervisor::log_info!("[VMID] Test 2: VMID 1 encoding...\n");
    let config1 = Stage2Config::new_with_vmid(0x6100_0000, 1);
    let vmid_bits = config1.vttbr >> 48;
    if vmid_bits != 1 {
        hypervisor::log_info!("[VMID] FAILED: expected VMID 1, got {:#018x}\n", vmid_bits);
        return;
    }
    // Verify page table base is preserved
    let base = config1.vttbr & 0x0000_FFFF_FFFF_FFFE;
    if base != 0x6100_0000 {
        hypervisor::log_info!("[VMID] FAILED: base address corrupted\n");
        return;
    }
    hypervisor::log_info!("[VMID] Test 2 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  VMID/VTTBR Encoding Test PASSED (2 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}
