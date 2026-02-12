//! Bump allocator with free-list page recycling for hypervisor heap

pub struct BumpAllocator {
    next: u64,
    end: u64,
    allocated: u64,
    /// Head of the free-list (singly-linked via first 8 bytes of each free page).
    /// 0 means the free-list is empty.
    free_head: u64,
}

impl BumpAllocator {
    pub const unsafe fn new(start: u64, size: u64) -> Self {
        Self {
            next: start,
            end: start + size,
            allocated: 0,
            free_head: 0,
        }
    }

    /// Allocate a 4KB-aligned page. Pops from free-list first, falls back to bump.
    pub fn alloc_page(&mut self) -> Option<u64> {
        // Try free-list first
        if self.free_head != 0 {
            let page = self.free_head;
            // Read the next pointer from the first 8 bytes of this free page
            let next = unsafe { core::ptr::read_volatile(page as *const u64) };
            self.free_head = next;
            // Zero the page before returning
            unsafe {
                core::ptr::write_bytes(page as *mut u8, 0, 4096);
            }
            self.allocated += 4096;
            return Some(page);
        }
        // Fall back to bump allocation
        self.alloc_aligned(4096, 4096)
    }

    /// Return a 4KB page to the free-list for reuse.
    ///
    /// # Safety
    /// Caller must ensure `addr` was previously allocated via `alloc_page()`,
    /// is 4KB-aligned, and is no longer in use.
    pub unsafe fn free_page(&mut self, addr: u64) {
        debug_assert!(addr & 0xFFF == 0, "free_page: addr not 4KB-aligned");
        // Write current free_head into the first 8 bytes of the freed page
        core::ptr::write_volatile(addr as *mut u64, self.free_head);
        self.free_head = addr;
        self.allocated -= 4096;
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
        self.free_head = 0;
    }
}
