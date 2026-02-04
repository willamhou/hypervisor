//! Global heap tests

use hypervisor::uart_puts;

pub fn run_heap_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  Global Heap Test\n");
    uart_puts(b"========================================\n\n");

    // Test 1: Allocate a page from global heap
    uart_puts(b"[HEAP] Test 1: Page allocation...\n");
    let page = hypervisor::mm::heap::alloc_page();
    if page.is_none() {
        uart_puts(b"[HEAP] ERROR: Failed to allocate page\n");
        return;
    }
    let page_addr = page.unwrap();
    uart_puts(b"[HEAP] Test 1 PASSED\n\n");

    // Test 2: Write to allocated page
    uart_puts(b"[HEAP] Test 2: Write to allocated page...\n");
    let ptr = page_addr as *mut u64;
    unsafe {
        *ptr = 0xDEAD_BEEF_CAFE_BABE;
        let read_back = *ptr;
        if read_back != 0xDEAD_BEEF_CAFE_BABE {
            uart_puts(b"[HEAP] ERROR: Memory read-back mismatch\n");
            return;
        }
    }
    uart_puts(b"[HEAP] Test 2 PASSED\n\n");

    // Test 3: Check remaining space
    uart_puts(b"[HEAP] Test 3: Remaining space check...\n");
    let remaining = hypervisor::mm::heap::remaining();
    if remaining == 0 {
        uart_puts(b"[HEAP] ERROR: No remaining space\n");
        return;
    }
    uart_puts(b"[HEAP] Test 3 PASSED\n\n");

    // Test 4: Multiple allocations
    uart_puts(b"[HEAP] Test 4: Multiple allocations...\n");
    let a1 = hypervisor::mm::heap::alloc_page();
    let a2 = hypervisor::mm::heap::alloc_page();
    if a1.is_none() || a2.is_none() {
        uart_puts(b"[HEAP] ERROR: Multiple allocations failed\n");
        return;
    }
    let a1_addr = a1.unwrap();
    let a2_addr = a2.unwrap();
    if a2_addr != a1_addr + 4096 {
        uart_puts(b"[HEAP] ERROR: Pages not sequential\n");
        return;
    }
    uart_puts(b"[HEAP] Test 4 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  Global Heap Test PASSED\n");
    uart_puts(b"========================================\n\n");
}
