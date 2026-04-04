//! Global heap management

use super::BumpAllocator;
use crate::platform;
use crate::sync::SpinLock;

static HEAP: SpinLock<Option<BumpAllocator>> = SpinLock::new(None);

/// Initialize the global heap. Must be called before any allocation.
pub unsafe fn init() {
    let alloc = BumpAllocator::new(platform::HEAP_START, platform::HEAP_SIZE);
    *HEAP.lock() = Some(alloc);
}

/// Initialize the global heap at a specific address and size.
/// Used by S-EL2 SPMC for secure DRAM heap.
pub unsafe fn init_at(start: u64, size: u64) {
    let alloc = super::BumpAllocator::new(start, size);
    *HEAP.lock() = Some(alloc);
}

/// Allocate a 4KB-aligned page from the global heap
pub fn alloc_page() -> Option<u64> {
    HEAP.lock().as_mut().and_then(|a| a.alloc_page())
}

/// Allocate memory with specified size and alignment
pub fn alloc_aligned(size: u64, align: u64) -> Option<u64> {
    HEAP.lock()
        .as_mut()
        .and_then(|a| a.alloc_aligned(size, align))
}

/// Allocate memory with default alignment (8 bytes)
pub fn alloc(size: u64) -> Option<u64> {
    HEAP.lock().as_mut().and_then(|a| a.alloc(size))
}

/// Return a 4KB page to the free-list for reuse.
///
/// # Safety
/// Caller must ensure `addr` was previously allocated via `alloc_page()`,
/// is 4KB-aligned, and is no longer in use.
pub unsafe fn free_page(addr: u64) {
    HEAP.lock().as_mut().map(|a| a.free_page(addr));
}

/// Get remaining heap space
pub fn remaining() -> u64 {
    HEAP.lock().as_ref().map(|a| a.remaining()).unwrap_or(0)
}

/// Get total allocated bytes
pub fn allocated() -> u64 {
    HEAP.lock().as_ref().map(|a| a.allocated()).unwrap_or(0)
}
