# Virtio-Net Inter-VM Networking Design

> Date: 2026-02-17 | Status: Approved

## Context

The hypervisor supports multi-VM mode (2 Linux VMs time-sliced on 1 pCPU) with virtio-blk storage. This design adds virtio-net for inter-VM networking via an L2 virtual switch, enabling `ping` between VMs.

**Scope**: multi-VM mode (`make run-multi-vm`). Single-VM and multi-pCPU modes get the device registered but no peer to talk to.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Backend | Inter-VM (no host OS networking) | Bare-metal hypervisor has no TAP/socket API |
| Topology | L2 virtual switch (VSwitch) | Future-proof for >2 VMs, MAC-based forwarding |
| Feature level | Standard (MAC + STATUS + CSUM) | Linux driver works with minimal, CSUM avoids checksum overhead |
| MMIO slot | Slot 1: `0x0a000200`, INTID 49 | QEMU virt convention, stride 0x200 from slot 0 |
| Frame buffering | SPSC ring buffer (8 frames/port) | Avoids cross-DEVICES lock access, handles ARP+ICMP bursts |
| AVF compatibility | Compile-time `virtio_slot()` abstraction | Addresses are hypervisor policy, not hardware discovery |

## Architecture

```
VM 0 Guest (Linux)                    VM 1 Guest (Linux)
  virtio-net driver                     virtio-net driver
  TX virtqueue (queue 1)                TX virtqueue (queue 1)
  RX virtqueue (queue 0)                RX virtqueue (queue 0)
  |                                     |
  v                                     v
VirtioMmioTransport<VirtioNet>   VirtioMmioTransport<VirtioNet>
@ 0x0a000200, INTID 49           @ 0x0a000200, INTID 49
(DEVICES[0])                     (DEVICES[1])
  |                                     |
  v                                     v
         VSwitch (L2 Virtual Switch)
  - MAC learning table (16 entries)
  - Forward by dst MAC / flood unknown+broadcast
  - PORT_RX[N] ring buffers for async delivery
```

### TX Path (Guest sends frame)

```
Guest writes QueueNotify (queue 1)
  -> VirtioMmioTransport::write() -> queue_notify(1, tx_queue)
  -> VirtioNet::process_tx(tx_queue, port_id)
     while get_avail_desc():
       strip virtio_net_hdr (10 bytes)
       VSwitch::forward(src_port, ethernet_frame)
         1. MAC learn: src_mac -> src_port
         2. Lookup dst_mac
         3. Found -> PORT_RX[dst].store(frame)
            Not found / broadcast -> flood all ports (except src)
       tx_queue.put_used(head, 0)
  -> signal_interrupt() -> inject_spi(49) [TX completion]
```

### RX Path (Run loop delivers frames)

```
run_one_iteration() / run_multi_vm():
  drain_net_rx(vm_id):
    while PORT_RX[vm_id].take():
      DEVICES[vm_id].inject_net_rx(frame)
        -> VirtioMmioTransport<VirtioNet>::inject_rx(frame)
           get_avail_desc() from RX queue (queue 0)
           write virtio_net_hdr (10 bytes, zeroed) + frame
           put_used(head, 10 + frame.len)
           signal_interrupt() -> inject_spi(49)
        -> Guest receives SPI 49 -> virtio_net driver processes RX
```

### Why Async Delivery (PORT_RX ring buffer)

TX happens inside `DEVICES[src_vm]` lock. Directly writing to `DEVICES[dst_vm]` would:
- Single-pCPU: work (UnsafeCell, no lock) but breaks abstraction
- Multi-pCPU: deadlock risk (different SpinLock instances, but fragile)

PORT_RX ring buffer decouples TX and RX, matching the existing pattern (PENDING_SPIS, UART_RX). The run loop drains PORT_RX before each VM iteration, same as it drains UART_RX.

## Components

### VirtioNet (`src/devices/virtio/net.rs`)

```rust
pub struct VirtioNet {
    mac: [u8; 6],       // mac_for_vm(vm_id): 52:54:00:00:00:(vm_id+1)
    port_id: usize,     // = VM ID, used as VSwitch port
    status: u16,        // VIRTIO_NET_S_LINK_UP = 1
}
```

VirtioDevice impl:
- `device_id()` -> 1 (VIRTIO_ID_NET)
- `device_features()` -> VERSION_1 | MAC | STATUS | CSUM | GUEST_CSUM
- `config_read()` -> MAC (6 bytes @ offset 0) + status (2 bytes @ offset 6)
- `num_queues()` -> 2 (RX=0, TX=1)
- `queue_notify(0, q)` -> no-op (guest replenishing RX buffers)
- `queue_notify(1, q)` -> process_tx(): strip hdr, forward via VSwitch

virtio_net_hdr (10 bytes, no MRG_RXBUF):
```rust
#[repr(C)]
struct VirtioNetHdr {
    flags: u8, gso_type: u8, hdr_len: u16,
    gso_size: u16, csum_start: u16, csum_offset: u16,
}
```

### VSwitch (`src/vswitch.rs`)

```rust
const MAX_PORTS: usize = 4;
const MAC_TABLE_SIZE: usize = 16;

pub struct VSwitch {
    mac_table: [MacEntry; MAC_TABLE_SIZE],
    mac_count: usize,
    port_count: usize,
}

struct MacEntry {
    mac: [u8; 6],
    port_id: usize,
    valid: bool,
}
```

API:
- `add_port(port_id)` — register port (called at VM init)
- `forward(src_port, frame)` — L2 forward: learn src MAC, lookup dst MAC, deliver or flood
- Internal: `deliver_to_port(port_id, frame)` -> `PORT_RX[port_id].store(frame)`

Forwarding logic:
1. Learn: `mac_table[src_mac] = src_port`
2. If dst is broadcast/multicast (`dst[0] & 1 != 0`) -> flood all ports except src
3. Lookup dst_mac -> found: deliver to port; not found: flood

### NetRxRing (`src/vswitch.rs`)

Per-port SPSC ring buffer (producer: VSwitch, consumer: run loop):

```rust
const NET_RX_RING_SIZE: usize = 8;
const MAX_FRAME_SIZE: usize = 1514;

pub struct NetRxRing {
    frames: UnsafeCell<[FrameSlot; NET_RX_RING_SIZE]>,
    head: AtomicUsize,  // consumer index
    tail: AtomicUsize,  // producer index
}

struct FrameSlot {
    buf: [u8; MAX_FRAME_SIZE],
    len: u16,
}

pub static PORT_RX: [NetRxRing; MAX_PORTS] = [...];
```

8 frames x 1514 bytes x 4 ports = ~48KB. Acceptable for 16MB heap.

### VirtioMmioTransport Extension

```rust
impl VirtioMmioTransport<VirtioNet> {
    /// VSwitch -> guest RX: write frame to RX queue, signal interrupt
    pub fn inject_rx(&mut self, frame: &[u8]) -> bool {
        let rx_queue = &mut self.queues[0];
        // get avail desc, write virtio_net_hdr (10B zeroed) + frame, put_used
        // signal_interrupt() -> inject_spi(49)
    }
}
```

### Device Enum Dispatch Macro

Replace 6 hand-written match arms with macro:

```rust
macro_rules! dispatch_device {
    ($self:expr, $method:ident $(, $arg:expr)*) => {
        match $self {
            Device::Uart(d) => d.$method($($arg),*),
            Device::Gicd(d) => d.$method($($arg),*),
            Device::Gicr(d) => d.$method($($arg),*),
            Device::VirtioBlk(d) => d.$method($($arg),*),
            Device::VirtioNet(d) => d.$method($($arg),*),
        }
    };
}
```

### Virtio Slot Abstraction (`src/platform.rs`)

```rust
pub const VIRTIO_MMIO_BASE: u64 = 0x0a00_0000;
pub const VIRTIO_MMIO_STRIDE: u64 = 0x200;
pub const VIRTIO_SPI_BASE: u32 = 48;

pub const fn virtio_slot(n: usize) -> (u64, u32) {
    (VIRTIO_MMIO_BASE + (n as u64) * VIRTIO_MMIO_STRIDE,
     VIRTIO_SPI_BASE + n as u32)
}
// slot 0: virtio-blk  (0x0a000000, INTID 48)
// slot 1: virtio-net  (0x0a000200, INTID 49)
```

Existing `VIRTIO_BLK_BASE` / `VIRTIO_BLK_INTID` in `devices/mod.rs` migrated to use `virtio_slot(0)`.

## Integration

### Guest DTB

New node in `guest/linux/guest.dts` and `guest-vm1.dts`:

```dts
virtio_mmio@a000200 {
    dma-coherent;
    interrupts = <0x00 0x11 0x01>;  // SPI 17 = INTID 49
    reg = <0x00 0xa000200 0x00 0x200>;
    compatible = "virtio,mmio";
};
```

### Stage-2

No changes needed. `0x0a000200` is outside mapped guest RAM and GIC regions — accesses trap as Stage-2 Data Abort -> MMIO dispatch.

### Device Registration (`guest_loader.rs`)

```rust
// Single-VM (run_guest):
DEVICES[0].attach_virtio_net(0);

// Multi-VM (run_multi_vm_guests):
DEVICES[0].attach_virtio_net(0);
DEVICES[1].attach_virtio_net(1);
```

### Run Loop Integration

Drain net RX in the same location as UART RX drain:

- `run_one_iteration()`: after `drain_uart_rx`, before `inject_pending_sgis`
- `run_multi_vm()`: after `activate_stage2`, before `run_one_iteration`
- `run_vcpu()` (multi-pCPU): after `drain_uart_rx`

### GlobalDeviceManager

Both cfg variants get new methods:
- `attach_virtio_net(vm_id: usize)`
- `inject_net_rx(frame: &[u8]) -> bool`

### Initramfs Auto-IP

Modify `/init` in `guest/linux/initramfs.cpio.gz` to auto-configure eth0:

```bash
if [ -e /sys/class/net/eth0 ]; then
    MAC=$(cat /sys/class/net/eth0/address)
    IP_LAST=$(echo "$MAC" | awk -F: '{print $6+0}')
    ip addr add 10.0.0.${IP_LAST}/24 dev eth0
    ip link set eth0 up
    echo "Network: eth0 10.0.0.${IP_LAST}/24 (MAC: ${MAC})"
fi
```

MAC `52:54:00:00:00:01` -> IP `10.0.0.1` (VM 0)
MAC `52:54:00:00:00:02` -> IP `10.0.0.2` (VM 1)

Single-VM mode: no eth0 device -> script skips. No behavioral change.

BusyBox v1.36.0 confirmed: `awk`, `sed`, `ip`, `cat` all available.

## Testing

### Unit Tests (3 new suites, ~18 assertions)

**test_virtio_net** (~6 assertions):
- device_id() == 1
- device_features() includes VERSION_1 | MAC | STATUS | CSUM | GUEST_CSUM
- config_read() returns correct MAC + LINK_UP status
- num_queues() == 2
- mac_for_vm(0) == [52:54:00:00:00:01]
- mac_for_vm(1) == [52:54:00:00:00:02]

**test_vswitch** (~6 assertions):
- Flood on unknown unicast
- MAC learning from src address
- Precise forwarding after learning
- Broadcast floods all ports except src
- No self-delivery (src_port excluded)
- MAC table capacity

**test_net_rx_ring** (~6 assertions):
- Empty ring take() -> None
- Store + take round-trip
- Take empties ring
- Fill 8 frames -> all succeed
- 9th frame -> store() returns false
- Take 1 + store 1 -> succeeds

### Integration Test (manual, `make run-multi-vm`)

Expected boot output:
```
[VM 0] virtio_net virtio1: [eth0] MAC 52:54:00:00:00:01
Network: eth0 10.0.0.1/24 (MAC: 52:54:00:00:00:01)
[VM 1] virtio_net virtio1: [eth0] MAC 52:54:00:00:00:02
Network: eth0 10.0.0.2/24 (MAC: 52:54:00:00:00:02)
```

Verification (in VM 0 shell):
```
/ # ping -c 3 10.0.0.2
PING 10.0.0.2: 3 packets transmitted, 3 received, 0% packet loss
```

## File Change Summary

| File | Change | Impact |
|------|--------|--------|
| `src/devices/virtio/net.rs` | **NEW** — VirtioNet device backend | MEDIUM |
| `src/devices/virtio/mod.rs` | Add `pub mod net` | LOW |
| `src/vswitch.rs` | **NEW** — VSwitch + NetRxRing + PORT_RX | MEDIUM |
| `src/lib.rs` | Add `pub mod vswitch` | LOW |
| `src/devices/mod.rs` | VirtioNet variant + dispatch macro + attach/inject methods | MEDIUM |
| `src/platform.rs` | `virtio_slot()` abstraction | LOW |
| `src/global.rs` | `attach_virtio_net` / `inject_net_rx` proxy methods | LOW |
| `src/guest_loader.rs` | Register virtio-net at VM init | LOW |
| `src/vm.rs` | `drain_net_rx()` in run loops | LOW |
| `guest/linux/guest.dts` | Add `virtio_mmio@a000200` node | LOW |
| `guest/linux/guest-vm1.dts` | Add `virtio_mmio@a000200` node | LOW |
| `guest/linux/initramfs.cpio.gz` | Auto-IP in `/init` | LOW |
| `tests/test_virtio_net.rs` | **NEW** — VirtioNet unit tests | LOW |
| `tests/test_vswitch.rs` | **NEW** — VSwitch unit tests | LOW |
| `tests/test_net_rx_ring.rs` | **NEW** — NetRxRing unit tests | LOW |

## AVF Compatibility Notes

This design leaves extension points for future AVF-style mechanisms:
- `virtio_slot()` can be replaced with DTB-driven discovery
- VirtioDevice trait is transport-agnostic (works with future virtio-pci)
- MMIO_GUARD: add HVC branch in exception handler to register MMIO regions
- MEM_SHARE: add hypercall for explicit memory sharing (bounce buffers)

## Known Limitations (V2)

- Single-frame latency: RX delivery waits for next VM scheduling iteration (~10ms worst case)
- No multi-queue: single RX/TX pair (adequate for inter-VM ping, not for throughput)
- No VIRTIO_NET_F_MRG_RXBUF: large frames require single descriptor
- No promiscuous mode or VLAN support
- Initramfs rebuild is manual (not a Makefile target)
