//! Test page ownership tracking via Stage-2 PTE SW bits

use hypervisor::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute};
use hypervisor::ffa::stage2_walker::Stage2Walker;

pub fn run_page_ownership_test() {
    hypervisor::log_info!("\n=== Test: Page Ownership (SW bits) ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    // Create a mapper and map a 2MB region
    let mut mapper = DynamicIdentityMapper::new();
    mapper
        .map_region(0x5000_0000, 0x0020_0000, MemoryAttribute::Normal)
        .unwrap();

    // Test 1: Default SW bits should be 0 (OWNED)
    {
        let bits = mapper.read_sw_bits(0x5000_0000);
        if bits == Some(0) {
            hypervisor::log_info!("  [PASS] Default SW bits = 0 (OWNED)\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Default SW bits\n");
            fail += 1;
        }
    }

    // Test 2: Write SHARED_OWNED (0b01) and read back
    {
        mapper.write_sw_bits(0x5000_0000, 0b01).unwrap();
        let bits = mapper.read_sw_bits(0x5000_0000);
        if bits == Some(0b01) {
            hypervisor::log_info!("  [PASS] Write/read SHARED_OWNED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Write/read SHARED_OWNED\n");
            fail += 1;
        }
    }

    // Test 3: Write back to OWNED (0b00) and verify
    {
        mapper.write_sw_bits(0x5000_0000, 0b00).unwrap();
        let bits = mapper.read_sw_bits(0x5000_0000);
        if bits == Some(0b00) {
            hypervisor::log_info!("  [PASS] Restore to OWNED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Restore to OWNED\n");
            fail += 1;
        }
    }

    // Test 4: Unmapped IPA returns None
    {
        let bits = mapper.read_sw_bits(0x9000_0000);
        if bits.is_none() {
            hypervisor::log_info!("  [PASS] Unmapped IPA returns None\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Unmapped IPA should be None\n");
            fail += 1;
        }
    }

    // Test 5: 2MB block split — modify SW bits on one 4KB page within a 2MB block
    //
    // The Stage2Walker should split the 2MB block into 512 x 4KB page entries
    // so that only the target page is modified.
    {
        let mut mapper2 = DynamicIdentityMapper::new();
        mapper2
            .map_region(0x6000_0000, 0x0020_0000, MemoryAttribute::Normal)
            .unwrap();
        let walker = Stage2Walker::new(mapper2.vttbr());

        // Before split: both pages should be OWNED (0b00)
        let bits_before = walker.read_sw_bits(0x6000_0000);
        if bits_before == Some(0b00) {
            hypervisor::log_info!("  [PASS] Pre-split: base page SW=OWNED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Pre-split: base page SW bits\n");
            fail += 1;
        }

        // Write SharedOwned to page at offset +0x1000 (second 4KB page in block)
        walker.write_sw_bits(0x6000_1000, 0b01).unwrap();

        // Target page should be SharedOwned
        let target_bits = walker.read_sw_bits(0x6000_1000);
        if target_bits == Some(0b01) {
            hypervisor::log_info!("  [PASS] Post-split: target page SW=SHARED_OWNED\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Post-split: target page SW bits\n");
            fail += 1;
        }

        // Neighboring pages should still be OWNED (block split preserves attributes)
        let base_bits = walker.read_sw_bits(0x6000_0000);
        if base_bits == Some(0b00) {
            hypervisor::log_info!("  [PASS] Post-split: base page SW=OWNED (unchanged)\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Post-split: base page should be OWNED\n");
            fail += 1;
        }

        // Test S2AP split: set target page to RO, verify neighbor stays RW
        walker.set_s2ap(0x6000_1000, 0b01).unwrap(); // RO
        let target_s2ap = walker.read_s2ap(0x6000_1000);
        if target_s2ap == Some(0b01) {
            hypervisor::log_info!("  [PASS] Post-split: target S2AP=RO\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Post-split: target S2AP\n");
            fail += 1;
        }

        let base_s2ap = walker.read_s2ap(0x6000_0000);
        if base_s2ap == Some(0b11) {
            hypervisor::log_info!("  [PASS] Post-split: base S2AP=RW (unchanged)\n");
            pass += 1;
        } else {
            hypervisor::log_info!("  [FAIL] Post-split: base S2AP should be RW\n");
            fail += 1;
        }

        // Leak mapper to avoid double-free of page tables
        #[allow(clippy::forget_non_drop)]
        core::mem::forget(mapper2);
    }

    hypervisor::log_info!("  Results: {} passed, {} failed\n", pass, fail);
    assert!(fail == 0, "Page ownership tests failed");
}
