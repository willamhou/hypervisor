# Virtio-Net Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add virtio-net inter-VM networking with an L2 virtual switch so two Linux VMs can `ping` each other.

**Architecture:** VirtioNet device (virtio device ID 1) on MMIO slot 1 (0x0a000200, INTID 49). TX path strips virtio_net_hdr and forwards Ethernet frames through a VSwitch with MAC learning. RX delivery is async via SPSC ring buffers (PORT_RX), drained in the run loop — same pattern as UART_RX and PENDING_SPIS.

**Tech Stack:** Rust no_std, ARM64, QEMU virt, virtio-mmio v2, Linux 6.12.12

**Design Doc:** `docs/plans/2026-02-17-virtio-net-design.md`

---

### Task 1: Add virtio_slot() Abstraction to platform.rs

**Files:**
- Modify: `src/platform.rs` (append after VIRTIO_DISK_SIZE)
- Modify: `src/devices/mod.rs:105-107` (replace VIRTIO_BLK_BASE/INTID)

**Step 1: Add virtio_slot() to platform.rs**

In `src/platform.rs`, append after the `VIRTIO_DISK_SIZE` constant (line 32):

```rust
// ── Virtio-MMIO slot layout ───────────────────────────────────────
/// Base address of the first virtio-mmio transport (QEMU virt convention)
pub const VIRTIO_MMIO_BASE: u64 = 0x0a00_0000;
/// Stride between virtio-mmio transports
pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
/// First SPI INTID for virtio devices (SPI 16 = INTID 48)
pub const VIRTIO_SPI_BASE: u32 = 48;

/// Compute (base_addr, intid) for virtio-mmio slot N.
/// Slot 0: virtio-blk (0x0a000000, INTID 48)
/// Slot 1: virtio-net (0x0a000200, INTID 49)
pub const fn virtio_slot(n: usize) -> (u64, u32) {
    (
        VIRTIO_MMIO_BASE + (n as u64) * VIRTIO_MMIO_STRIDE,
        VIRTIO_SPI_BASE + n as u32,
    )
}
```

**Step 2: Migrate virtio-blk to use virtio_slot(0)**

In `src/devices/mod.rs`, replace lines 104-107:

```rust
// Old:
const VIRTIO_BLK_BASE: u64 = 0x0a00_0000;
const VIRTIO_BLK_INTID: u32 = 48;

// New:
use crate::platform;
const VIRTIO_BLK_BASE: u64 = platform::virtio_slot(0).0;
const VIRTIO_BLK_INTID: u32 = platform::virtio_slot(0).1;
```

**Step 3: Build to verify no regressions**

Run: `make`
Expected: Clean build, no errors.

**Step 4: Run tests**

Run: `make run` (exit with Ctrl-A X after tests complete)
Expected: All 23 existing test suites pass.

**Step 5: Commit**

```bash
git add src/platform.rs src/devices/mod.rs
git commit -m "feat: add virtio_slot() MMIO abstraction, migrate virtio-blk"
```

---

### Task 2: Create NetRxRing (SPSC Ring Buffer)

**Files:**
- Create: `src/vswitch.rs`
- Modify: `src/lib.rs` (add `pub mod vswitch`)

**Step 1: Write the NetRxRing test**

Create `tests/test_net_rx_ring.rs`:

```rust
//! NetRxRing SPSC ring buffer tests

use hypervisor::vswitch::{NetRxRing, MAX_FRAME_SIZE};
use hypervisor::uart_puts;

pub fn run_net_rx_ring_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  NetRxRing Test\n");
    uart_puts(b"========================================\n\n");

    // Test 1: Empty ring take() -> None
    uart_puts(b"[NETRX] Test 1: Empty take...\n");
    let ring = NetRxRing::new();
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let result = ring.take(&mut buf);
    assert_eq_test(result.is_none(), true, "empty ring should return None");
    uart_puts(b"[NETRX] Test 1 PASSED\n\n");

    // Test 2: Store + take round-trip
    uart_puts(b"[NETRX] Test 2: Store + take...\n");
    let frame = [0xAA; 64];
    let stored = ring.store(&frame);
    assert_eq_test(stored, true, "store should succeed");
    let len = ring.take(&mut buf);
    assert_eq_test(len, Some(64), "take should return 64 bytes");
    assert_eq_test(buf[0], 0xAA, "first byte should be 0xAA");
    assert_eq_test(buf[63], 0xAA, "last byte should be 0xAA");
    uart_puts(b"[NETRX] Test 2 PASSED\n\n");

    // Test 3: Take empties ring
    uart_puts(b"[NETRX] Test 3: Take empties...\n");
    let result = ring.take(&mut buf);
    assert_eq_test(result.is_none(), true, "ring should be empty after take");
    uart_puts(b"[NETRX] Test 3 PASSED\n\n");

    // Test 4: Fill 8 frames -> all succeed
    uart_puts(b"[NETRX] Test 4: Fill 8...\n");
    let frame = [0xBB; 100];
    for _ in 0..8 {
        let ok = ring.store(&frame);
        assert_eq_test(ok, true, "store within capacity should succeed");
    }
    uart_puts(b"[NETRX] Test 4 PASSED\n\n");

    // Test 5: 9th frame -> store() returns false (full)
    uart_puts(b"[NETRX] Test 5: Overflow...\n");
    let ok = ring.store(&frame);
    assert_eq_test(ok, false, "store on full ring should fail");
    uart_puts(b"[NETRX] Test 5 PASSED\n\n");

    // Test 6: Take 1 + store 1 -> succeeds (wraparound)
    uart_puts(b"[NETRX] Test 6: Wraparound...\n");
    let len = ring.take(&mut buf);
    assert_eq_test(len, Some(100), "take should return 100");
    let ok = ring.store(&frame);
    assert_eq_test(ok, true, "store after take should succeed");
    uart_puts(b"[NETRX] Test 6 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  NetRxRing Test PASSED (8 assertions)\n");
    uart_puts(b"========================================\n\n");
}

fn assert_eq_test<T: PartialEq + core::fmt::Debug>(a: T, b: T, msg: &str) {
    if a != b {
        uart_puts(b"[NETRX] ASSERTION FAILED: ");
        uart_puts(msg.as_bytes());
        uart_puts(b"\n");
        panic!("test assertion failed");
    }
}
```

**Step 2: Write NetRxRing implementation**

Create `src/vswitch.rs` with just the ring buffer first:

```rust
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
const NET_RX_RING_SIZE: usize = 8;
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
                FrameSlot::new(), FrameSlot::new(),
                FrameSlot::new(), FrameSlot::new(),
                FrameSlot::new(), FrameSlot::new(),
                FrameSlot::new(), FrameSlot::new(),
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
```

**Step 3: Register module**

In `src/lib.rs`, add after `pub mod dtb;`:

```rust
pub mod vswitch;
```

**Step 4: Wire test into test harness**

In `tests/mod.rs`, add:
```rust
pub mod test_net_rx_ring;
pub use test_net_rx_ring::run_net_rx_ring_test;
```

In `src/main.rs`, add before the `run_guest_interrupt_test()` call:
```rust
tests::run_net_rx_ring_test();
```

**Step 5: Build and run tests**

Run: `make run`
Expected: All existing tests pass + new NetRxRing test passes (8 assertions).

**Step 6: Commit**

```bash
git add src/vswitch.rs src/lib.rs tests/test_net_rx_ring.rs tests/mod.rs src/main.rs
git commit -m "feat: add NetRxRing SPSC ring buffer for virtio-net RX delivery"
```

---

### Task 3: Add VSwitch (L2 Virtual Switch)

**Files:**
- Modify: `src/vswitch.rs` (append VSwitch struct + global)
- Create: `tests/test_vswitch.rs`

**Step 1: Write the VSwitch test**

Create `tests/test_vswitch.rs`:

```rust
//! VSwitch L2 forwarding tests

use hypervisor::vswitch::{PORT_RX, MAX_FRAME_SIZE};
use hypervisor::uart_puts;

pub fn run_vswitch_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  VSwitch Test\n");
    uart_puts(b"========================================\n\n");

    // Setup: register 2 ports
    hypervisor::vswitch::vswitch_reset();
    hypervisor::vswitch::vswitch_add_port(0);
    hypervisor::vswitch::vswitch_add_port(1);

    // Drain any leftover frames from previous tests
    let mut drain_buf = [0u8; MAX_FRAME_SIZE];
    while PORT_RX[0].take(&mut drain_buf).is_some() {}
    while PORT_RX[1].take(&mut drain_buf).is_some() {}

    // Build test Ethernet frames
    // Frame from port 0: src=AA:BB:CC:DD:EE:00, dst=11:22:33:44:55:66
    let mut frame0 = [0u8; 64];
    // dst MAC
    frame0[0..6].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    // src MAC
    frame0[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]);

    // Test 1: Unknown unicast -> flood all ports except src
    uart_puts(b"[VSWITCH] Test 1: Unknown unicast flood...\n");
    hypervisor::vswitch::vswitch_forward(0, &frame0);
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let len = PORT_RX[1].take(&mut buf);
    assert_ok(len.is_some(), "port 1 should receive flooded frame");
    let len0 = PORT_RX[0].take(&mut buf);
    assert_ok(len0.is_none(), "port 0 (src) should NOT receive own frame");
    uart_puts(b"[VSWITCH] Test 1 PASSED\n\n");

    // Test 2: MAC learning — src MAC AA:BB:CC:DD:EE:00 learned on port 0
    // Now send frame FROM port 1 TO that learned MAC
    uart_puts(b"[VSWITCH] Test 2: MAC learning + precise forward...\n");
    let mut frame1 = [0u8; 64];
    // dst = previously learned MAC (AA:BB:CC:DD:EE:00 -> port 0)
    frame1[0..6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]);
    // src = port 1's MAC
    frame1[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01]);
    hypervisor::vswitch::vswitch_forward(1, &frame1);
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_some(), "port 0 should receive precisely forwarded frame");
    uart_puts(b"[VSWITCH] Test 2 PASSED\n\n");

    // Test 3: Broadcast floods all ports except src
    uart_puts(b"[VSWITCH] Test 3: Broadcast flood...\n");
    let mut bcast = [0u8; 64];
    bcast[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]);
    bcast[6..12].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]);
    hypervisor::vswitch::vswitch_forward(0, &bcast);
    let len = PORT_RX[1].take(&mut buf);
    assert_ok(len.is_some(), "port 1 should receive broadcast");
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_none(), "port 0 (src) should NOT receive own broadcast");
    uart_puts(b"[VSWITCH] Test 3 PASSED\n\n");

    // Test 4: No self-delivery on unicast
    uart_puts(b"[VSWITCH] Test 4: No self-delivery...\n");
    // Send from port 0 to port 0's own learned MAC
    let mut self_frame = [0u8; 64];
    self_frame[0..6].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x00]); // dst = port 0
    self_frame[6..12].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]); // src
    hypervisor::vswitch::vswitch_forward(0, &self_frame);
    // Port 0 should NOT get its own frame (dst_port == src_port)
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_none(), "no self-delivery when dst_port == src_port");
    uart_puts(b"[VSWITCH] Test 4 PASSED\n\n");

    // Test 5: MAC table capacity (fill 16 entries)
    uart_puts(b"[VSWITCH] Test 5: MAC table capacity...\n");
    hypervisor::vswitch::vswitch_reset();
    hypervisor::vswitch::vswitch_add_port(0);
    hypervisor::vswitch::vswitch_add_port(1);
    // Drain rings
    while PORT_RX[0].take(&mut drain_buf).is_some() {}
    while PORT_RX[1].take(&mut drain_buf).is_some() {}

    for i in 0..16u8 {
        let mut f = [0u8; 64];
        f[0..6].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]); // broadcast dst
        f[6..12].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, i]); // unique src
        hypervisor::vswitch::vswitch_forward(0, &f);
    }
    // All should have been learned (table has 16 slots)
    // Drain port 1 broadcasts
    while PORT_RX[1].take(&mut drain_buf).is_some() {}
    // Send to first learned MAC — should forward precisely
    let mut to_first = [0u8; 64];
    to_first[0..6].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x00]); // dst = first
    to_first[6..12].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0xFF]); // src
    hypervisor::vswitch::vswitch_forward(1, &to_first);
    let len = PORT_RX[0].take(&mut buf);
    assert_ok(len.is_some(), "MAC table should hold 16 entries");
    uart_puts(b"[VSWITCH] Test 5 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  VSwitch Test PASSED (6 assertions)\n");
    uart_puts(b"========================================\n\n");
}

fn assert_ok(cond: bool, msg: &str) {
    if !cond {
        uart_puts(b"[VSWITCH] ASSERTION FAILED: ");
        uart_puts(msg.as_bytes());
        uart_puts(b"\n");
        panic!("test assertion failed");
    }
}
```

**Step 2: Implement VSwitch**

Append to `src/vswitch.rs` (after `PORT_RX`):

```rust
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
```

**Step 3: Wire VSwitch test into harness**

In `tests/mod.rs`, add:
```rust
pub mod test_vswitch;
pub use test_vswitch::run_vswitch_test;
```

In `src/main.rs`, add after `run_net_rx_ring_test()`:
```rust
tests::run_vswitch_test();
```

**Step 4: Build and run tests**

Run: `make run`
Expected: All tests pass + new VSwitch test passes (6 assertions).

**Step 5: Commit**

```bash
git add src/vswitch.rs tests/test_vswitch.rs tests/mod.rs src/main.rs
git commit -m "feat: add VSwitch L2 virtual switch with MAC learning"
```

---

### Task 4: Create VirtioNet Device Backend

**Files:**
- Create: `src/devices/virtio/net.rs`
- Modify: `src/devices/virtio/mod.rs` (add `pub mod net`)

**Step 1: Write the VirtioNet test**

Create `tests/test_virtio_net.rs`:

```rust
//! VirtioNet device backend tests

use hypervisor::devices::virtio::net::VirtioNet;
use hypervisor::devices::virtio::VirtioDevice;
use hypervisor::uart_puts;

pub fn run_virtio_net_test() {
    uart_puts(b"\n========================================\n");
    uart_puts(b"  VirtioNet Device Test\n");
    uart_puts(b"========================================\n\n");

    let net = VirtioNet::new(0);

    // Test 1: device_id
    uart_puts(b"[VNET] Test 1: device_id...\n");
    assert_eq_vnet(net.device_id(), 1, "device_id should be 1 (VIRTIO_ID_NET)");
    uart_puts(b"[VNET] Test 1 PASSED\n\n");

    // Test 2: device_features (VERSION_1 | MAC | STATUS, no CSUM)
    uart_puts(b"[VNET] Test 2: device_features...\n");
    let features = net.device_features();
    let version_1: u64 = 1 << 32;
    let mac: u64 = 1 << 5;
    let status: u64 = 1 << 16;
    let csum: u64 = 1 << 0;
    assert_eq_vnet(features & version_1, version_1, "VERSION_1 should be set");
    assert_eq_vnet(features & mac, mac, "MAC should be set");
    assert_eq_vnet(features & status, status, "STATUS should be set");
    assert_eq_vnet(features & csum, 0, "CSUM should NOT be set");
    uart_puts(b"[VNET] Test 2 PASSED\n\n");

    // Test 3: num_queues
    uart_puts(b"[VNET] Test 3: num_queues...\n");
    assert_eq_vnet(net.num_queues(), 2, "should have 2 queues (RX + TX)");
    uart_puts(b"[VNET] Test 3 PASSED\n\n");

    // Test 4: config_read MAC bytes
    uart_puts(b"[VNET] Test 4: config_read MAC...\n");
    // MAC for VM 0: 52:54:00:00:00:01
    let byte0 = net.config_read(0, 1);
    assert_eq_vnet(byte0, 0x52, "MAC[0] should be 0x52");
    let byte5 = net.config_read(5, 1);
    assert_eq_vnet(byte5, 0x01, "MAC[5] should be 0x01");
    uart_puts(b"[VNET] Test 4 PASSED\n\n");

    // Test 5: config_read status (LINK_UP = 1)
    uart_puts(b"[VNET] Test 5: config_read status...\n");
    let status_val = net.config_read(6, 2);
    assert_eq_vnet(status_val, 1, "status should be LINK_UP (1)");
    uart_puts(b"[VNET] Test 5 PASSED\n\n");

    // Test 6: mac_for_vm
    uart_puts(b"[VNET] Test 6: mac_for_vm...\n");
    let mac0 = VirtioNet::mac_for_vm(0);
    assert_eq_vnet(mac0, [0x52, 0x54, 0x00, 0x00, 0x00, 0x01], "VM 0 MAC");
    let mac1 = VirtioNet::mac_for_vm(1);
    assert_eq_vnet(mac1, [0x52, 0x54, 0x00, 0x00, 0x00, 0x02], "VM 1 MAC");
    uart_puts(b"[VNET] Test 6 PASSED\n\n");

    uart_puts(b"========================================\n");
    uart_puts(b"  VirtioNet Device Test PASSED (8 assertions)\n");
    uart_puts(b"========================================\n\n");
}

fn assert_eq_vnet<T: PartialEq + core::fmt::Debug>(a: T, b: T, msg: &str) {
    if a != b {
        uart_puts(b"[VNET] ASSERTION FAILED: ");
        uart_puts(msg.as_bytes());
        uart_puts(b"\n");
        panic!("test assertion failed");
    }
}
```

**Step 2: Implement VirtioNet**

Create `src/devices/virtio/net.rs`:

```rust
//! Virtio network device backend.
//!
//! Implements a virtio-net device (device ID 1) for inter-VM networking.
//! TX: strips virtio_net_hdr, forwards Ethernet frame via VSwitch.
//! RX: inject_rx() writes virtio_net_hdr + frame into guest RX queue.

use super::VirtioDevice;
use super::queue::Virtqueue;

// ── Feature bits ────────────────────────────────────────────────────
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_F_STATUS: u64 = 1 << 16;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;

// Status bits
const VIRTIO_NET_S_LINK_UP: u16 = 1;

/// Size of virtio_net_hdr_v1 (with num_buffers field).
/// Linux always uses this size for VERSION_1 devices.
const VIRTIO_NET_HDR_SIZE: usize = 12;

/// Virtio-net device backend.
pub struct VirtioNet {
    mac: [u8; 6],
    port_id: usize,
    status: u16,
}

impl VirtioNet {
    /// Create a new VirtioNet device for the given VM.
    pub fn new(vm_id: usize) -> Self {
        Self {
            mac: Self::mac_for_vm(vm_id),
            port_id: vm_id,
            status: VIRTIO_NET_S_LINK_UP,
        }
    }

    /// Generate a deterministic MAC address for a VM.
    /// VM 0 -> 52:54:00:00:00:01, VM 1 -> 52:54:00:00:00:02
    pub fn mac_for_vm(vm_id: usize) -> [u8; 6] {
        [0x52, 0x54, 0x00, 0x00, 0x00, (vm_id + 1) as u8]
    }

    /// Process TX queue: strip virtio_net_hdr, forward frames via VSwitch.
    fn process_tx(&mut self, queue: &mut Virtqueue) {
        while let Some(chain) = queue.get_avail_desc() {
            // Descriptor chain: [virtio_net_hdr] [frame data...]
            // Could be 1 descriptor (hdr + frame) or 2+ (hdr, then frame)
            let mut total_len = 0usize;
            let mut frame_buf = [0u8; crate::vswitch::MAX_FRAME_SIZE];
            let mut frame_len = 0usize;

            for i in 0..chain.count {
                let desc = &chain.descs[i];
                let buf_addr = desc.addr as *const u8;
                let buf_len = desc.len as usize;

                if total_len < VIRTIO_NET_HDR_SIZE {
                    // Still in the header region — skip header bytes
                    let skip = core::cmp::min(VIRTIO_NET_HDR_SIZE - total_len, buf_len);
                    let data_start = skip;
                    let data_len = buf_len - skip;
                    if data_len > 0 && frame_len + data_len <= frame_buf.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                buf_addr.add(data_start),
                                frame_buf.as_mut_ptr().add(frame_len),
                                data_len,
                            );
                        }
                        frame_len += data_len;
                    }
                } else {
                    // Pure frame data
                    if frame_len + buf_len <= frame_buf.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                buf_addr,
                                frame_buf.as_mut_ptr().add(frame_len),
                                buf_len,
                            );
                        }
                        frame_len += buf_len;
                    }
                }
                total_len += buf_len;
            }

            // Forward the Ethernet frame through the VSwitch
            if frame_len >= 14 {
                crate::vswitch::vswitch_forward(self.port_id, &frame_buf[..frame_len]);
            }

            queue.put_used(chain.head, 0);
        }
    }
}

impl VirtioDevice for VirtioNet {
    fn device_id(&self) -> u32 { 1 } // VIRTIO_ID_NET

    fn device_features(&self) -> u64 {
        VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC | VIRTIO_NET_F_STATUS
    }

    fn config_read(&self, offset: u64, size: u8) -> u64 {
        // Config space layout:
        //   0x00-0x05: mac[6]     (6 bytes)
        //   0x06-0x07: status     (u16)
        match (offset, size) {
            // Single byte reads of MAC address
            (o @ 0..=5, 1) => self.mac[o as usize] as u64,
            // 2-byte read of status
            (6, 2) => self.status as u64,
            // 4-byte read spanning MAC bytes
            (0, 4) => {
                (self.mac[0] as u64)
                    | ((self.mac[1] as u64) << 8)
                    | ((self.mac[2] as u64) << 16)
                    | ((self.mac[3] as u64) << 24)
            }
            (4, 4) => {
                (self.mac[4] as u64)
                    | ((self.mac[5] as u64) << 8)
                    | ((self.status as u64) << 16)
            }
            _ => 0,
        }
    }

    fn config_write(&mut self, _offset: u64, _value: u64, _size: u8) {
        // Config space is read-only for net
    }

    fn queue_notify(&mut self, queue_idx: u16, queue: &mut Virtqueue) {
        match queue_idx {
            0 => {} // RX queue — guest replenishing buffers, no action needed
            1 => self.process_tx(queue),
            _ => {}
        }
    }

    fn num_queues(&self) -> u16 { 2 } // RX=0, TX=1

    fn max_queue_size(&self) -> u16 { 256 }
}
```

**Step 3: Register module**

In `src/devices/virtio/mod.rs`, add after `pub mod blk;`:
```rust
pub mod net;
```

**Step 4: Wire test into harness**

In `tests/mod.rs`, add:
```rust
pub mod test_virtio_net;
pub use test_virtio_net::run_virtio_net_test;
```

In `src/main.rs`, add after `run_vswitch_test()`:
```rust
tests::run_virtio_net_test();
```

**Step 5: Build and run tests**

Run: `make run`
Expected: All tests pass + new VirtioNet test passes (8 assertions).

**Step 6: Commit**

```bash
git add src/devices/virtio/net.rs src/devices/virtio/mod.rs tests/test_virtio_net.rs tests/mod.rs src/main.rs
git commit -m "feat: add VirtioNet device backend (virtio ID 1, MAC+STATUS)"
```

---

### Task 5: Add VirtioNet to Device Enum + DeviceManager

**Files:**
- Modify: `src/devices/mod.rs` (add variant, match arms, attach method, accessor)

**Step 1: Add Device::VirtioNet variant**

In `src/devices/mod.rs`, add to the `Device` enum (after `VirtioBlk`):
```rust
VirtioNet(virtio::mmio::VirtioMmioTransport<virtio::net::VirtioNet>),
```

**Step 2: Add match arms to all 6 MmioDevice methods**

For each method in `impl MmioDevice for Device` (`read`, `write`, `base_address`, `size`, `pending_irq`, `ack_irq`), add:
```rust
Device::VirtioNet(d) => d.method_name(...),
```

**Step 3: Add attach_virtio_net() and virtio_net_mut() to DeviceManager**

After `attach_virtio_blk()`:

```rust
/// Attach a virtio-net device for the given VM.
pub fn attach_virtio_net(&mut self, vm_id: usize) {
    let (base, intid) = crate::platform::virtio_slot(1);
    let net = virtio::net::VirtioNet::new(vm_id);
    let transport = virtio::mmio::VirtioMmioTransport::new(base, net, intid);
    self.register_device(Device::VirtioNet(transport));
    crate::vswitch::vswitch_add_port(vm_id);
}

/// Get a mutable reference to the virtio-net transport (for RX injection).
pub fn virtio_net_mut(&mut self) -> Option<&mut virtio::mmio::VirtioMmioTransport<virtio::net::VirtioNet>> {
    for slot in self.devices.iter_mut() {
        if let Some(Device::VirtioNet(transport)) = slot {
            return Some(transport);
        }
    }
    None
}
```

**Step 4: Build and run tests**

Run: `make run`
Expected: All tests pass. The new Device variant is compiled but not yet wired into guest boot.

**Step 5: Commit**

```bash
git add src/devices/mod.rs
git commit -m "feat: add VirtioNet to Device enum with dispatch + attach + accessor"
```

---

### Task 6: Add inject_rx() to VirtioMmioTransport<VirtioNet>

**Files:**
- Modify: `src/devices/virtio/mmio.rs` (add impl block for VirtioNet specialization)

**Step 1: Add inject_rx method**

At the end of `src/devices/virtio/mmio.rs`, add:

```rust
/// Specialized methods for VirtioNet transport (RX injection).
impl VirtioMmioTransport<super::net::VirtioNet> {
    /// Inject a received frame into the guest's RX virtqueue.
    ///
    /// Writes a 12-byte virtio_net_hdr (zeroed, num_buffers=1) followed by
    /// the Ethernet frame data into the first available RX descriptor.
    /// Signals an interrupt (inject_spi) after writing.
    ///
    /// Returns false if no RX descriptor is available (guest hasn't
    /// replenished its RX queue).
    pub fn inject_rx(&mut self, frame: &[u8]) -> bool {
        let rx_queue = &mut self.queues[0];
        let chain = match rx_queue.get_avail_desc() {
            Some(c) => c,
            None => return false, // No available RX buffer
        };

        if chain.count == 0 {
            return false;
        }

        // virtio_net_hdr_v1: 12 bytes, all zeroed except num_buffers=1
        let hdr: [u8; 12] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]; // num_buffers=1 (LE u16 at offset 10)

        let total_len = 12 + frame.len();

        // Write header + frame into descriptor buffer(s)
        let mut written = 0usize;
        let combined_len = hdr.len() + frame.len();

        for i in 0..chain.count {
            let desc = &chain.descs[i];
            // Only write to device-writable descriptors (WRITE flag set)
            if desc.flags & super::queue::VIRTQ_DESC_F_WRITE == 0 {
                continue;
            }
            let buf_addr = desc.addr as *mut u8;
            let buf_cap = desc.len as usize;
            let mut buf_written = 0usize;

            while written < combined_len && buf_written < buf_cap {
                let byte = if written < 12 {
                    hdr[written]
                } else {
                    frame[written - 12]
                };
                unsafe { core::ptr::write_volatile(buf_addr.add(buf_written), byte); }
                written += 1;
                buf_written += 1;
            }
        }

        rx_queue.put_used(chain.head, total_len as u32);
        self.signal_interrupt();
        true
    }
}
```

**Step 2: Build**

Run: `make`
Expected: Clean build.

**Step 3: Commit**

```bash
git add src/devices/virtio/mmio.rs
git commit -m "feat: add inject_rx() for VirtioNet RX frame delivery"
```

---

### Task 7: Wire GlobalDeviceManager + Run Loop Integration

**Files:**
- Modify: `src/global.rs` (add attach_virtio_net, inject_net_rx to both cfg variants)
- Modify: `src/vm.rs` (add drain_net_rx calls)
- Modify: `src/guest_loader.rs` (call attach_virtio_net at VM init)

**Step 1: Add methods to GlobalDeviceManager (non-multi_pcpu)**

In `src/global.rs`, in the `#[cfg(not(feature = "multi_pcpu"))]` impl block, add after `uart_mut()`:

```rust
pub fn attach_virtio_net(&self, vm_id: usize) {
    unsafe { (*self.devices.get()).attach_virtio_net(vm_id); }
}

pub fn inject_net_rx(&self, frame: &[u8]) -> bool {
    unsafe {
        if let Some(transport) = (*self.devices.get()).virtio_net_mut() {
            transport.inject_rx(frame)
        } else {
            false
        }
    }
}
```

**Step 2: Add methods to GlobalDeviceManager (multi_pcpu)**

In the `#[cfg(feature = "multi_pcpu")]` impl block, add after `drain_uart_rx()`:

```rust
pub fn attach_virtio_net(&self, vm_id: usize) {
    self.devices.lock().attach_virtio_net(vm_id);
}

pub fn inject_net_rx(&self, frame: &[u8]) -> bool {
    if let Some(transport) = self.devices.lock().virtio_net_mut() {
        transport.inject_rx(frame)
    } else {
        false
    }
}
```

**Step 3: Add drain_net_rx() function**

In `src/vm.rs`, add a free function after `inject_pending_spis()`:

```rust
/// Drain pending network RX frames from PORT_RX into the guest's
/// virtio-net RX queue via DEVICES[vm_id].inject_net_rx().
///
/// Precondition: CURRENT_VM_ID must be set (inject_net_rx -> inject_spi
/// reads it to route the SPI to the correct VM).
fn drain_net_rx(vm_id: usize) {
    use crate::vswitch::{PORT_RX, MAX_FRAME_SIZE};
    if PORT_RX[vm_id].is_empty() {
        return; // fast path
    }
    let mut buf = [0u8; MAX_FRAME_SIZE];
    while let Some(len) = PORT_RX[vm_id].take(&mut buf) {
        crate::global::DEVICES[vm_id].inject_net_rx(&buf[..len]);
    }
}
```

**Step 4: Call drain_net_rx() in run_one_iteration()**

In `src/vm.rs`, in `run_one_iteration()`, add after the UART RX drain block (after `inject_spi(33)` block, before `inject_pending_sgis`):

```rust
// Drain pending network RX frames
drain_net_rx(self.id);
```

**Step 5: Call drain_net_rx() in run_multi_vm()**

In `src/vm.rs`, in `run_multi_vm()`, add after `vm.activate_stage2();` and before `if vm.run_one_iteration()`:

```rust
// Drain network RX for this VM (CURRENT_VM_ID already set above)
drain_net_rx(vm.id);
```

**Step 6: Call drain_net_rx() in run_vcpu() (multi-pCPU)**

In `src/vm.rs`, in `run_vcpu()`, add after `DEVICES[self.id].drain_uart_rx();`:

```rust
// Drain pending network RX frames
drain_net_rx(self.id);
```

**Step 7: Register virtio-net in guest_loader.rs**

In `src/guest_loader.rs`, in `run_guest()`, add after the `attach_virtio_blk` block:

```rust
// Attach virtio-net device
if config.guest_type == GuestType::Linux {
    crate::global::DEVICES[0].attach_virtio_net(0);
}
```

In `run_multi_vm_guests()`, add after `DEVICES[0].attach_virtio_blk(...)`:
```rust
crate::global::DEVICES[0].attach_virtio_net(0);
```

And after `DEVICES[1].attach_virtio_blk(...)`:
```rust
crate::global::DEVICES[1].attach_virtio_net(1);
```

**Step 8: Build and run tests**

Run: `make run`
Expected: All tests pass. drain_net_rx is a no-op since PORT_RX is empty.

**Step 9: Commit**

```bash
git add src/global.rs src/vm.rs src/guest_loader.rs
git commit -m "feat: wire virtio-net into GlobalDeviceManager, run loops, and guest boot"
```

---

### Task 8: Update Guest DTB Files

**Files:**
- Modify: `guest/linux/guest.dts`
- Modify: `guest/linux/guest-vm1.dts`

**Step 1: Add virtio_mmio@a000200 to guest.dts**

In `guest/linux/guest.dts`, add after the `virtio_mmio@a000000` node (after line 90):

```dts
	virtio_mmio@a000200 {
		dma-coherent;
		interrupts = <0x00 0x11 0x01>;
		reg = <0x00 0xa000200 0x00 0x200>;
		compatible = "virtio,mmio";
	};
```

**Step 2: Add virtio_mmio@a000200 to guest-vm1.dts**

In `guest/linux/guest-vm1.dts`, add after the `virtio_mmio@a000000` node (after line 90):

```dts
	virtio_mmio@a000200 {
		dma-coherent;
		interrupts = <0x00 0x11 0x01>;
		reg = <0x00 0xa000200 0x00 0x200>;
		compatible = "virtio,mmio";
	};
```

**Step 3: Compile DTBs**

Run:
```bash
dtc -I dts -O dtb -o guest/linux/guest.dtb guest/linux/guest.dts
dtc -I dts -O dtb -o guest/linux/guest-vm1.dtb guest/linux/guest-vm1.dts
```
Expected: No errors. Warning about missing phandle references is OK.

**Step 4: Commit**

```bash
git add guest/linux/guest.dts guest/linux/guest-vm1.dts guest/linux/guest.dtb guest/linux/guest-vm1.dtb
git commit -m "feat: add virtio-net MMIO node to guest DTBs (slot 1, SPI 17)"
```

---

### Task 9: Update Initramfs with Auto-IP Configuration

**Files:**
- Modify: `guest/linux/initramfs.cpio.gz` (rebuild with updated /init)

**Step 1: Extract current initramfs**

```bash
mkdir -p /tmp/initramfs-edit
cd /tmp/initramfs-edit
zcat /home/willamhou/sides/hypervisor/guest/linux/initramfs.cpio.gz | cpio -idm
```

**Step 2: Read current /init script**

Read `/tmp/initramfs-edit/init` to understand the existing structure.

**Step 3: Add auto-IP block to /init**

Append before the final `exec /bin/sh` (or after mounting /dev):

```bash
# Auto-configure eth0 from MAC address (virtio-net)
if [ -e /sys/class/net/eth0 ]; then
    MAC=$(cat /sys/class/net/eth0/address)
    IP_LAST=$(echo "$MAC" | awk -F: '{print $6+0}')
    ip addr add 10.0.0.${IP_LAST}/24 dev eth0
    ip link set eth0 up
    echo "Network: eth0 10.0.0.${IP_LAST}/24 (MAC: ${MAC})"
fi
```

**Step 4: Rebuild initramfs**

```bash
cd /tmp/initramfs-edit
find . | cpio -o -H newc | gzip > /home/willamhou/sides/hypervisor/guest/linux/initramfs.cpio.gz
```

**Step 5: Commit**

```bash
git add guest/linux/initramfs.cpio.gz
git commit -m "feat: add auto-IP eth0 configuration to initramfs /init"
```

---

### Task 10: Integration Test — Linux Boot with virtio-net

**Step 1: Test single-VM Linux boot (regression)**

Run: `make run-linux`
Expected:
- Linux boots to shell
- Kernel probes virtio-net: `virtio_net virtio1: ...` in boot log
- eth0 appears but no peer (single VM)
- No panics or hangs
- Exit with Ctrl-A X

**Step 2: Test multi-VM boot with ping**

Run: `make run-multi-vm`
Expected:
- Both VMs boot
- Both VMs auto-configure eth0 with 10.0.0.x/24
- In VM 0 shell: `ping -c 3 10.0.0.2` gets replies

**Step 3: Test unit tests (regression)**

Run: `make run`
Expected: All tests pass (26+ suites now).

**Step 4: Final commit (if any fixes needed)**

Fix any issues found during integration testing and commit.

---

## Dependency Graph

```
Task 1 (virtio_slot)
   |
Task 2 (NetRxRing) ──────────────────┐
   |                                   |
Task 3 (VSwitch) ─────────┐          |
   |                        |          |
Task 4 (VirtioNet) ───────┤          |
   |                        |          |
Task 5 (Device enum) ──────┤          |
   |                        |          |
Task 6 (inject_rx) ────────┤          |
   |                        |          |
Task 7 (Global + run loop) ┘──────────┘
   |
Task 8 (DTB) ─── can be done in parallel with 2-7
   |
Task 9 (Initramfs) ─── can be done in parallel with 2-7
   |
Task 10 (Integration test) ─── depends on all above
```

## File Change Summary

| File | Change |
|------|--------|
| `src/platform.rs` | Add `virtio_slot()` constants |
| `src/vswitch.rs` | **NEW** — NetRxRing + VSwitch + PORT_RX + VSWITCH |
| `src/lib.rs` | Add `pub mod vswitch` |
| `src/devices/virtio/net.rs` | **NEW** — VirtioNet device backend |
| `src/devices/virtio/mod.rs` | Add `pub mod net` |
| `src/devices/virtio/mmio.rs` | Add `inject_rx()` for VirtioNet |
| `src/devices/mod.rs` | VirtioNet variant + match arms + attach + accessor |
| `src/global.rs` | `attach_virtio_net` + `inject_net_rx` (both cfg variants) |
| `src/vm.rs` | `drain_net_rx()` in 3 run loops |
| `src/guest_loader.rs` | Register virtio-net at VM init |
| `guest/linux/guest.dts` | Add `virtio_mmio@a000200` |
| `guest/linux/guest-vm1.dts` | Add `virtio_mmio@a000200` |
| `guest/linux/initramfs.cpio.gz` | Auto-IP in `/init` |
| `tests/test_net_rx_ring.rs` | **NEW** — 8 assertions |
| `tests/test_vswitch.rs` | **NEW** — 6 assertions |
| `tests/test_virtio_net.rs` | **NEW** — 8 assertions |
| `tests/mod.rs` | Register 3 new test modules |
| `src/main.rs` | Call 3 new test functions |
