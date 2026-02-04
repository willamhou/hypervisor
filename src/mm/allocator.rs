//! Simple bump allocator for hypervisor heap

pub struct BumpAllocator {
    next: u64,
    end: u64,
    allocated: u64,
}

impl BumpAllocator {
    pub const unsafe fn new(start: u64, size: u64) -> Self {
        Self {
            next: start,
            end: start + size,
            allocated: 0,
        }
    }

    pub fn alloc_page(&mut self) -> Option<u64> {
        self.alloc_aligned(4096, 4096)
    }

    pub fn alloc_aligned(&mut self, size: u64, align: u64) -> Option<u64> {
        let aligned = (self.next + align - 1) & !(align - 1);
        let new_next = aligned + size;

        if new_next > self.end {
            return None;
        }

        self.next = new_next;
        self.allocated += size;
        Some(aligned)
    }

    pub fn alloc(&mut self, size: u64) -> Option<u64> {
        self.alloc_aligned(size, 8)
    }

    pub fn remaining(&self) -> u64 {
        self.end - self.next
    }

    pub fn allocated(&self) -> u64 {
        self.allocated
    }

    pub unsafe fn reset(&mut self, start: u64) {
        self.next = start;
        self.allocated = 0;
    }
}
