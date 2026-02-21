//! Global heap management

use super::BumpAllocator;
use crate::platform;
use core::cell::UnsafeCell;

struct GlobalHeap {
    allocator: UnsafeCell<Option<BumpAllocator>>,
}

unsafe impl Sync for GlobalHeap {}

static HEAP: GlobalHeap = GlobalHeap {
    allocator: UnsafeCell::new(None),
};

/// Initialize the global heap. Must be called before any allocation.
pub unsafe fn init() {
    let alloc = BumpAllocator::new(platform::HEAP_START, platform::HEAP_SIZE);
    *HEAP.allocator.get() = Some(alloc);
}

/// Initialize the global heap at a specific address and size.
/// Used by S-EL2 SPMC for secure DRAM heap.
pub unsafe fn init_at(start: u64, size: u64) {
    let alloc = super::BumpAllocator::new(start, size);
    *HEAP.allocator.get() = Some(alloc);
}

/// Allocate a 4KB-aligned page from the global heap
pub fn alloc_page() -> Option<u64> {
    unsafe {
        (*HEAP.allocator.get())
            .as_mut()
            .and_then(|a| a.alloc_page())
    }
}

/// Allocate memory with specified size and alignment
pub fn alloc_aligned(size: u64, align: u64) -> Option<u64> {
    unsafe {
        (*HEAP.allocator.get())
            .as_mut()
            .and_then(|a| a.alloc_aligned(size, align))
    }
}

/// Allocate memory with default alignment (8 bytes)
pub fn alloc(size: u64) -> Option<u64> {
    unsafe { (*HEAP.allocator.get()).as_mut().and_then(|a| a.alloc(size)) }
}

/// Return a 4KB page to the free-list for reuse.
///
/// # Safety
/// Caller must ensure `addr` was previously allocated via `alloc_page()`,
/// is 4KB-aligned, and is no longer in use.
pub unsafe fn free_page(addr: u64) {
    (*HEAP.allocator.get()).as_mut().map(|a| a.free_page(addr));
}

/// Get remaining heap space
pub fn remaining() -> u64 {
    unsafe {
        (*HEAP.allocator.get())
            .as_ref()
            .map(|a| a.remaining())
            .unwrap_or(0)
    }
}

/// Get total allocated bytes
pub fn allocated() -> u64 {
    unsafe {
        (*HEAP.allocator.get())
            .as_ref()
            .map(|a| a.allocated())
            .unwrap_or(0)
    }
}
