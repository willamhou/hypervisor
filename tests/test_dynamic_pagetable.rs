//! Dynamic page table allocation tests

use hypervisor::arch::aarch64::mm::mmu::{DynamicIdentityMapper, MemoryAttribute};
use hypervisor::uart_puts;

pub fn run_dynamic_pt_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Dynamic Page Table Test\n");
    uart_puts(b"========================================\n\n");

    // Test 1: Create mapper
    uart_puts(b"[DYN PT] Test 1: Create mapper...\n");
    let mut mapper = DynamicIdentityMapper::new();
    let vttbr = mapper.vttbr();
    if vttbr == 0 {
        uart_puts(b"[DYN PT] ERROR: VTTBR is zero\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 1 PASSED\n\n");

    // Test 2: Map a 2MB region
    uart_puts(b"[DYN PT] Test 2: Map 2MB region...\n");
    let result = mapper.map_region(0x1000_0000, 0x20_0000, MemoryAttribute::Normal);
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to map region\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 2 PASSED\n\n");

    // Test 3: Map multiple regions
    uart_puts(b"[DYN PT] Test 3: Map multiple regions...\n");
    let result = mapper.map_region(0x2000_0000, 0x40_0000, MemoryAttribute::Device);
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to map second region\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 3 PASSED\n\n");

    // Test 4: Verify VTTBR is non-zero and page-aligned
    uart_puts(b"[DYN PT] Test 4: Verify VTTBR...\n");
    let final_vttbr = mapper.vttbr();
    if final_vttbr == 0 {
        uart_puts(b"[DYN PT] ERROR: VTTBR is zero\n");
        return;
    }
    if final_vttbr % 4096 != 0 {
        uart_puts(b"[DYN PT] ERROR: VTTBR not page-aligned\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 4 PASSED\n\n");

    // Test 5: Unmap a 4KB page from a 2MB block
    uart_puts(b"[DYN PT] Test 5: Unmap 4KB page...\n");
    // Map a fresh 2MB region, then unmap a single 4KB page within it
    let result = mapper.map_region(0x3000_0000, 0x20_0000, MemoryAttribute::Normal);
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to map region for 4KB test\n");
        return;
    }
    let result = mapper.unmap_4kb_page(0x3000_1000); // Unmap second page
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to unmap 4KB page\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 5 PASSED\n\n");

    // Test 6: Unmap multiple 4KB pages in same 2MB block
    uart_puts(b"[DYN PT] Test 6: Unmap multiple 4KB pages...\n");
    let result = mapper.unmap_4kb_page(0x3000_2000);
    if result.is_err() {
        uart_puts(b"[DYN PT] ERROR: Failed to unmap second 4KB page\n");
        return;
    }
    uart_puts(b"[DYN PT] Test 6 PASSED\n\n");

    // Clear VTTBR_EL2 so subsequent tests (e.g. FF-A MEM_SHARE) don't see stale
    // page tables and attempt Stage-2 walks on pages that were never mapped.
    unsafe {
        core::arch::asm!("msr vttbr_el2, xzr", "isb", options(nomem, nostack));
    }

    uart_puts(b"========================================\n");
    uart_puts(b"  Dynamic Page Table Test PASSED (6 assertions)\n");
    uart_puts(b"========================================\n\n");
}
