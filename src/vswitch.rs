//! Virtual Switch and Network RX Ring Buffer
//!
//! PORT_RX[port_id] is a per-port SPSC ring buffer.
//! Producer: VSwitch::forward() (inside DEVICES lock during TX)
//! Consumer: run loop drain_net_rx() (outside DEVICES lock)

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Maximum Ethernet frame size (no jumbo frames)
pub const MAX_FRAME_SIZE: usize = 1514;
/// Ring buffer depth per port
const NET_RX_RING_SIZE: usize = 9; // 8 usable + 1 sentinel slot for SPSC full detection
/// Maximum number of ports (matches MAX_VMS)
const MAX_PORTS: usize = 2;

/// A single frame slot in the ring buffer.
struct FrameSlot {
    buf: [u8; MAX_FRAME_SIZE],
    len: u16,
}

impl FrameSlot {
    const fn new() -> Self {
        Self {
            buf: [0u8; MAX_FRAME_SIZE],
            len: 0,
        }
    }
}

/// Per-port SPSC ring buffer for async frame delivery.
///
/// Single producer (VSwitch::forward, inside DEVICES lock) and
/// single consumer (drain_net_rx, in run loop). Uses atomic
/// head/tail indices for lock-free synchronization.
pub struct NetRxRing {
    frames: UnsafeCell<[FrameSlot; NET_RX_RING_SIZE]>,
    head: AtomicUsize, // consumer reads from here
    tail: AtomicUsize, // producer writes here
}

// SAFETY: SPSC — single producer (VSwitch in DEVICES lock),
// single consumer (run loop). Atomic indices provide ordering.
unsafe impl Sync for NetRxRing {}

impl NetRxRing {
    pub const fn new() -> Self {
        Self {
            frames: UnsafeCell::new([
                FrameSlot::new(), FrameSlot::new(), FrameSlot::new(),
                FrameSlot::new(), FrameSlot::new(), FrameSlot::new(),
                FrameSlot::new(), FrameSlot::new(), FrameSlot::new(),
            ]),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Store a frame into the ring (producer side).
    /// Returns false if ring is full.
    pub fn store(&self, frame: &[u8]) -> bool {
        let len = frame.len();
        if len == 0 || len > MAX_FRAME_SIZE {
            return false;
        }
        let tail = self.tail.load(Ordering::Relaxed);
        let next = (tail + 1) % NET_RX_RING_SIZE;
        if next == self.head.load(Ordering::Acquire) {
            return false; // full
        }
        unsafe {
            let slots = &mut *self.frames.get();
            slots[tail].buf[..len].copy_from_slice(frame);
            slots[tail].len = len as u16;
        }
        self.tail.store(next, Ordering::Release);
        true
    }

    /// Take a frame from the ring (consumer side).
    /// Returns Some(len) if a frame was copied into `buf`, None if empty.
    pub fn take(&self, buf: &mut [u8]) -> Option<usize> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None; // empty
        }
        let len;
        unsafe {
            let slots = &*self.frames.get();
            len = slots[head].len as usize;
            let copy_len = core::cmp::min(len, buf.len());
            buf[..copy_len].copy_from_slice(&slots[head].buf[..copy_len]);
        }
        self.head.store((head + 1) % NET_RX_RING_SIZE, Ordering::Release);
        Some(len)
    }

    /// Check if the ring is empty (for fast-path skip in run loop).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed) == self.tail.load(Ordering::Acquire)
    }
}

/// Per-port RX ring buffers. Index = VM ID (= port ID).
pub static PORT_RX: [NetRxRing; MAX_PORTS] = [
    NetRxRing::new(),
    NetRxRing::new(),
];
