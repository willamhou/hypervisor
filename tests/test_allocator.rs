//! Bump allocator tests

use hypervisor::mm::allocator::BumpAllocator;

pub fn run_allocator_test() {
    hypervisor::log_info!("\n========================================\n");
    hypervisor::log_info!("  Bump Allocator Test\n");
    hypervisor::log_info!("========================================\n\n");

    let heap_start = 0x4100_0000u64;
    let heap_size = 0x10_0000u64; // 1MB

    let mut alloc = unsafe { BumpAllocator::new(heap_start, heap_size) };

    // Test 1: Allocate 4KB page
    hypervisor::log_info!("[ALLOC] Test 1: Page allocation...\n");
    let page1 = alloc.alloc_page();
    if page1.is_none() {
        hypervisor::log_info!("[ALLOC] ERROR: Failed to allocate page\n");
        return;
    }
    let page1 = page1.unwrap();
    if page1 % 4096 != 0 {
        hypervisor::log_info!("[ALLOC] ERROR: Page not 4KB aligned\n");
        return;
    }
    hypervisor::log_info!("[ALLOC] Test 1 PASSED\n\n");

    // Test 2: Sequential allocation
    hypervisor::log_info!("[ALLOC] Test 2: Sequential allocation...\n");
    let page2 = alloc.alloc_page();
    if page2.is_none() {
        hypervisor::log_info!("[ALLOC] ERROR: Failed to allocate second page\n");
        return;
    }
    if page2.unwrap() != page1 + 4096 {
        hypervisor::log_info!("[ALLOC] ERROR: Pages not sequential\n");
        return;
    }
    hypervisor::log_info!("[ALLOC] Test 2 PASSED\n\n");

    // Test 3: Aligned allocation
    hypervisor::log_info!("[ALLOC] Test 3: Aligned allocation...\n");
    let aligned = alloc.alloc_aligned(256, 2048);
    if aligned.is_none() {
        hypervisor::log_info!("[ALLOC] ERROR: Failed aligned allocation\n");
        return;
    }
    if aligned.unwrap() % 2048 != 0 {
        hypervisor::log_info!("[ALLOC] ERROR: Allocation not aligned to 2048\n");
        return;
    }
    hypervisor::log_info!("[ALLOC] Test 3 PASSED\n\n");

    // Test 4: Remaining space
    hypervisor::log_info!("[ALLOC] Test 4: Remaining space check...\n");
    let remaining = alloc.remaining();
    if remaining == 0 {
        hypervisor::log_info!("[ALLOC] ERROR: No remaining space reported\n");
        return;
    }
    hypervisor::log_info!("[ALLOC] Test 4 PASSED\n\n");

    hypervisor::log_info!("========================================\n");
    hypervisor::log_info!("  Bump Allocator Test PASSED\n");
    hypervisor::log_info!("========================================\n\n");
}
