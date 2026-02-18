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

// ── VSwitch L2 Virtual Switch ─────────────────────────────────────

const MAC_TABLE_SIZE: usize = 16;

struct MacEntry {
    mac: [u8; 6],
    port_id: usize,
    valid: bool,
}

impl MacEntry {
    const fn new() -> Self {
        Self {
            mac: [0; 6],
            port_id: 0,
            valid: false,
        }
    }
}

/// L2 virtual switch with MAC learning.
///
/// Forwarding logic:
/// 1. Learn src_mac -> src_port
/// 2. If dst is broadcast/multicast -> flood all ports except src
/// 3. Lookup dst_mac -> found: deliver; not found: flood
pub struct VSwitch {
    mac_table: [MacEntry; MAC_TABLE_SIZE],
    mac_count: usize,
    port_count: usize,
}

impl VSwitch {
    const fn new() -> Self {
        Self {
            mac_table: [
                MacEntry::new(), MacEntry::new(), MacEntry::new(), MacEntry::new(),
                MacEntry::new(), MacEntry::new(), MacEntry::new(), MacEntry::new(),
                MacEntry::new(), MacEntry::new(), MacEntry::new(), MacEntry::new(),
                MacEntry::new(), MacEntry::new(), MacEntry::new(), MacEntry::new(),
            ],
            mac_count: 0,
            port_count: 0,
        }
    }

    fn reset(&mut self) {
        for entry in self.mac_table.iter_mut() {
            entry.valid = false;
        }
        self.mac_count = 0;
        self.port_count = 0;
    }

    fn add_port(&mut self, _port_id: usize) {
        self.port_count += 1;
    }

    fn forward(&mut self, src_port: usize, frame: &[u8]) {
        if frame.len() < 14 {
            return; // Too short for Ethernet header
        }

        let dst_mac = &frame[0..6];
        let src_mac = &frame[6..12];

        // Learn: src_mac -> src_port
        self.learn(src_mac, src_port);

        // Check broadcast/multicast (bit 0 of first byte)
        if dst_mac[0] & 1 != 0 {
            // Flood to all ports except src
            self.flood(src_port, frame);
            return;
        }

        // Unicast: lookup dst_mac
        if let Some(dst_port) = self.lookup(dst_mac) {
            if dst_port != src_port {
                PORT_RX[dst_port].store(frame);
            }
            // If dst_port == src_port, drop (no self-delivery)
        } else {
            // Unknown unicast: flood
            self.flood(src_port, frame);
        }
    }

    fn learn(&mut self, mac: &[u8], port_id: usize) {
        // Check if already learned (update port if changed)
        for entry in self.mac_table.iter_mut() {
            if entry.valid && entry.mac == mac[..6] {
                entry.port_id = port_id;
                return;
            }
        }
        // Add new entry
        if self.mac_count < MAC_TABLE_SIZE {
            for entry in self.mac_table.iter_mut() {
                if !entry.valid {
                    entry.mac.copy_from_slice(&mac[..6]);
                    entry.port_id = port_id;
                    entry.valid = true;
                    self.mac_count += 1;
                    return;
                }
            }
        }
        // Table full — drop (no eviction in V1)
    }

    fn lookup(&self, mac: &[u8]) -> Option<usize> {
        for entry in &self.mac_table {
            if entry.valid && entry.mac == mac[..6] {
                return Some(entry.port_id);
            }
        }
        None
    }

    fn flood(&self, src_port: usize, frame: &[u8]) {
        for port in 0..MAX_PORTS {
            if port != src_port {
                PORT_RX[port].store(frame);
            }
        }
    }
}

/// Global VSwitch instance.
/// SAFETY: forward() only called from inside DEVICES lock (single producer
/// per VM iteration). add_port() only called during init. Same pattern as
/// GlobalDeviceManager in single-pCPU mode.
struct VSwitchCell(UnsafeCell<VSwitch>);
unsafe impl Sync for VSwitchCell {}

static VSWITCH: VSwitchCell = VSwitchCell(UnsafeCell::new(VSwitch::new()));

/// Public API — called from VirtioNet::process_tx() inside DEVICES lock.
pub fn vswitch_forward(src_port: usize, frame: &[u8]) {
    unsafe { (*VSWITCH.0.get()).forward(src_port, frame); }
}

/// Register a port (called during attach_virtio_net).
pub fn vswitch_add_port(port_id: usize) {
    unsafe { (*VSWITCH.0.get()).add_port(port_id); }
}

/// Reset VSwitch state (for tests).
pub fn vswitch_reset() {
    unsafe { (*VSWITCH.0.get()).reset(); }
}
