//! Dynamic page table allocation tests

use hypervisor::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute};

pub fn run_dynamic_pt_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Dynamic Page Table Test\n");
    hypervisor::log_info!("========================================\n\n");

    // Test 1: Create mapper
    hypervisor::log_info!("[DYN PT] Test 1: Create mapper...\n");
    let mut mapper = DynamicIdentityMapper::new();
    let vttbr = mapper.vttbr();
    if vttbr == 0 {
        hypervisor::log_info!("[DYN PT] ERROR: VTTBR is zero\n");
        return;
    }
    hypervisor::log_info!("[DYN PT] Test 1 PASSED\n\n");

    // Test 2: Map a 2MB region
    hypervisor::log_info!("[DYN PT] Test 2: Map 2MB region...\n");
    let result = mapper.map_region(0x1000_0000, 0x20_0000, MemoryAttribute::Normal);
    if result.is_err() {
        hypervisor::log_info!("[DYN PT] ERROR: Failed to map region\n");
        return;
    }
    hypervisor::log_info!("[DYN PT] Test 2 PASSED\n\n");

    // Test 3: Map multiple regions
    hypervisor::log_info!("[DYN PT] Test 3: Map multiple regions...\n");
    let result = mapper.map_region(0x2000_0000, 0x40_0000, MemoryAttribute::Device);
    if result.is_err() {
        hypervisor::log_info!("[DYN PT] ERROR: Failed to map second region\n");
        return;
    }
    hypervisor::log_info!("[DYN PT] Test 3 PASSED\n\n");

    // Test 4: Verify VTTBR is non-zero and page-aligned
    hypervisor::log_info!("[DYN PT] Test 4: Verify VTTBR...\n");
    let final_vttbr = mapper.vttbr();
    if final_vttbr == 0 {
        hypervisor::log_info!("[DYN PT] ERROR: VTTBR is zero\n");
        return;
    }
    if !final_vttbr.is_multiple_of(4096) {
        hypervisor::log_info!("[DYN PT] ERROR: VTTBR not page-aligned\n");
        return;
    }
    hypervisor::log_info!("[DYN PT] Test 4 PASSED\n\n");

    // Test 5: Unmap a 4KB page from a 2MB block
    hypervisor::log_info!("[DYN PT] Test 5: Unmap 4KB page...\n");
    // Map a fresh 2MB region, then unmap a single 4KB page within it
    let result = mapper.map_region(0x3000_0000, 0x20_0000, MemoryAttribute::Normal);
    if result.is_err() {
        hypervisor::log_info!("[DYN PT] ERROR: Failed to map region for 4KB test\n");
        return;
    }
    let result = mapper.unmap_4kb_page(0x3000_1000); // Unmap second page
    if result.is_err() {
        hypervisor::log_info!("[DYN PT] ERROR: Failed to unmap 4KB page\n");
        return;
    }
    hypervisor::log_info!("[DYN PT] Test 5 PASSED\n\n");

    // Test 6: Unmap multiple 4KB pages in same 2MB block
    hypervisor::log_info!("[DYN PT] Test 6: Unmap multiple 4KB pages...\n");
    let result = mapper.unmap_4kb_page(0x3000_2000);
    if result.is_err() {
        hypervisor::log_info!("[DYN PT] ERROR: Failed to unmap second 4KB page\n");
        return;
    }
    hypervisor::log_info!("[DYN PT] Test 6 PASSED\n\n");

    // Clear VTTBR_EL2 so subsequent tests (e.g. FF-A MEM_SHARE) don't see stale
    // page tables and attempt Stage-2 walks on pages that were never mapped.
    // SAFETY: Test teardown clears VTTBR_EL2 to known state between independent test scenarios.
    unsafe {
        core::arch::asm!("msr vttbr_el2, xzr", "isb", options(nomem, nostack));
    }

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Dynamic Page Table Test PASSED (6 assertions)\n");
    hypervisor::log_info!("========================================\n\n");
}
