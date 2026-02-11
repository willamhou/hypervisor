//! Split virtqueue implementation for virtio devices.
//!
//! The guest allocates descriptor table, available ring, and used ring in
//! guest physical memory. Since we use identity mapping (GPA == HPA), the
//! hypervisor can directly read/write these structures via volatile pointers.

/// A single virtqueue descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtqDesc {
    /// Guest physical address of the buffer
    pub addr: u64,
    /// Length of the buffer in bytes
    pub len: u32,
    /// Descriptor flags (NEXT, WRITE, INDIRECT)
    pub flags: u16,
    /// Index of the next descriptor in the chain (if NEXT flag set)
    pub next: u16,
}

/// Descriptor flags
pub const VIRTQ_DESC_F_NEXT: u16 = 1;
pub const VIRTQ_DESC_F_WRITE: u16 = 2;

/// Available ring header (followed by ring[num] entries).
#[repr(C)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    // ring: [u16; num] follows
}

/// Used ring header (followed by ring[num] VirtqUsedElem entries).
#[repr(C)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    // ring: [VirtqUsedElem; num] follows
}

/// A single used ring element.
#[repr(C)]
#[derive(Clone, Copy)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

/// A descriptor chain: the head index plus the descriptors themselves.
pub struct DescChain {
    /// Head descriptor index (needed for put_used)
    pub head: u16,
    /// The descriptors in order
    pub descs: [VirtqDesc; 4],
    /// Number of valid descriptors
    pub count: usize,
}

/// Split virtqueue state.
pub struct Virtqueue {
    /// Guest physical address of the descriptor table
    desc_addr: u64,
    /// Guest physical address of the available ring
    avail_addr: u64,
    /// Guest physical address of the used ring
    used_addr: u64,
    /// Queue size (number of descriptors, must be power of 2)
    pub num: u16,
    /// Last available index we processed
    last_avail_idx: u16,
    /// Whether the queue has been set up by the driver
    pub ready: bool,
}

impl Virtqueue {
    pub const fn new() -> Self {
        Self {
            desc_addr: 0,
            avail_addr: 0,
            used_addr: 0,
            num: 0,
            last_avail_idx: 0,
            ready: false,
        }
    }

    pub fn set_desc_addr(&mut self, low: u32, high: u32) {
        self.desc_addr = (low as u64) | ((high as u64) << 32);
    }

    pub fn set_avail_addr(&mut self, low: u32, high: u32) {
        self.avail_addr = (low as u64) | ((high as u64) << 32);
    }

    pub fn set_used_addr(&mut self, low: u32, high: u32) {
        self.used_addr = (low as u64) | ((high as u64) << 32);
    }

    /// Get low 32 bits of descriptor address (for split high/low writes)
    pub fn desc_addr_low(&self) -> u32 { self.desc_addr as u32 }
    /// Get low 32 bits of available ring address
    pub fn avail_addr_low(&self) -> u32 { self.avail_addr as u32 }
    /// Get low 32 bits of used ring address
    pub fn used_addr_low(&self) -> u32 { self.used_addr as u32 }

    /// Reset the queue to initial state
    pub fn reset(&mut self) {
        self.desc_addr = 0;
        self.avail_addr = 0;
        self.used_addr = 0;
        self.num = 0;
        self.last_avail_idx = 0;
        self.ready = false;
    }

    /// Check if there are new available descriptors to process.
    fn has_avail(&self) -> bool {
        if !self.ready || self.avail_addr == 0 {
            return false;
        }
        let avail = self.avail_addr as *const VirtqAvail;
        let avail_idx = unsafe { core::ptr::read_volatile(&(*avail).idx) };
        avail_idx != self.last_avail_idx
    }

    /// Get the next available descriptor chain from the guest.
    ///
    /// Returns `None` if no new descriptors are available.
    /// The returned `DescChain` contains up to 4 chained descriptors.
    pub fn get_avail_desc(&mut self) -> Option<DescChain> {
        if !self.has_avail() {
            return None;
        }

        // The ring array starts right after the VirtqAvail header (4 bytes)
        let ring_base = (self.avail_addr + 4) as *const u16;

        let ring_idx = (self.last_avail_idx % self.num) as usize;
        let head = unsafe { core::ptr::read_volatile(ring_base.add(ring_idx)) };
        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);

        // Walk the descriptor chain.
        //
        // Safety: relies on identity mapping (GPA == HPA). The bounds check
        // `idx >= self.num` prevents reading past the descriptor table, but
        // does NOT validate that desc.addr fields point to valid guest memory.
        // A malicious guest could set desc.addr to an arbitrary physical address.
        // This is acceptable for an educational hypervisor on a single-guest system.
        let desc_base = self.desc_addr as *const VirtqDesc;
        let mut chain = DescChain {
            head,
            descs: [VirtqDesc { addr: 0, len: 0, flags: 0, next: 0 }; 4],
            count: 0,
        };

        let mut idx = head;
        for _ in 0..4 {
            if (idx as u16) >= self.num {
                break;
            }
            let desc = unsafe { core::ptr::read_volatile(desc_base.add(idx as usize)) };
            chain.descs[chain.count] = desc;
            chain.count += 1;

            if desc.flags & VIRTQ_DESC_F_NEXT == 0 {
                break;
            }
            idx = desc.next;
        }

        Some(chain)
    }

    /// Put a used descriptor back into the used ring.
    ///
    /// `head` is the head descriptor index from the original chain.
    /// `len` is the total number of bytes written to the device-writable descriptors.
    pub fn put_used(&mut self, head: u16, len: u32) {
        if self.used_addr == 0 {
            return;
        }

        let used = self.used_addr as *mut VirtqUsed;
        let used_idx = unsafe { core::ptr::read_volatile(&(*used).idx) };
        let ring_idx = (used_idx % self.num) as usize;

        // Used ring elements start after the VirtqUsed header (4 bytes)
        let elem_base = (self.used_addr + 4) as *mut VirtqUsedElem;
        unsafe {
            let elem = VirtqUsedElem {
                id: head as u32,
                len,
            };
            core::ptr::write_volatile(elem_base.add(ring_idx), elem);

            // Memory barrier before updating the index
            core::sync::atomic::fence(core::sync::atomic::Ordering::Release);

            // Advance the used index
            core::ptr::write_volatile(&mut (*used).idx, used_idx.wrapping_add(1));
        }
    }
}
