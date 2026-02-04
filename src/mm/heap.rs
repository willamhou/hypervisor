//! Global heap management

use super::BumpAllocator;
use core::cell::UnsafeCell;

const HEAP_START: u64 = 0x4100_0000;
const HEAP_SIZE: u64 = 0x100_0000; // 16MB

struct GlobalHeap {
    allocator: UnsafeCell<Option<BumpAllocator>>,
}

unsafe impl Sync for GlobalHeap {}

static HEAP: GlobalHeap = GlobalHeap {
    allocator: UnsafeCell::new(None),
};

/// Initialize the global heap. Must be called before any allocation.
pub unsafe fn init() {
    let alloc = BumpAllocator::new(HEAP_START, HEAP_SIZE);
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
    unsafe {
        (*HEAP.allocator.get())
            .as_mut()
            .and_then(|a| a.alloc(size))
    }
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
