# Device Emulation Framework

This document describes the hypervisor's MMIO device emulation architecture.

## Overview

Guest I/O accesses to unmapped Stage-2 regions trap as Data Aborts to EL2. The hypervisor decodes the faulting instruction, routes the access to the appropriate emulated device, and resumes the guest.

## MMIO Trap-and-Emulate Flow

```
Guest executes LDR/STR to unmapped IPA
  → Stage-2 fault → Data Abort to EL2
  → handle_exception() → ExitReason::DataAbort
  → Read HPFAR_EL2 for IPA (NOT FAR_EL2 when guest MMU is on)
  → handle_mmio_abort(context, ipa)
      → Decode instruction (ISS or raw instruction)
      → MmioAccess { is_store, reg, size }
      → DeviceManager::handle_mmio(addr, value, size, is_write)
          → Scan devices[] for addr match
          → Route to device.read() or device.write()
      → Write result to guest register (for loads)
  → Advance PC by 4
  → flush_pending_spis_to_hardware() (for virtio completion)
  → ERET back to guest
```

### IPA Computation (HPFAR_EL2)

When guest MMU is on, FAR_EL2 = guest **virtual** address, not IPA:
```rust
let ipa_page = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8;  // HPFAR[43:4] = IPA[47:12]
let page_offset = far_el2 & 0xFFF;                       // page offset from FAR
let addr = ipa_page | page_offset;
```

### Instruction Decoding

`MmioAccess::decode()` in `src/arch/aarch64/hypervisor/decode.rs` supports two paths:

1. **ISS-based** (ISV=1 in ESR_EL2): Preferred. Works even when guest MMU is on. Extracts register, access size, and direction from ISS encoding.

2. **Instruction-based** (ISV=0): Falls back to reading the instruction at `context.pc`. Only works when PC is a physical address (guest MMU off). Decodes LDR/STR/LDRB/STRB/LDRH/STRH instruction formats.

## DeviceManager

Located in `src/devices/mod.rs`. Uses enum dispatch (no dynamic dispatch / trait objects):

### Device Enum

```rust
pub enum Device {
    Uart(pl011::VirtualUart),
    Gicd(gic::VirtualGicd),
    Gicr(gic::VirtualGicr),
    VirtioBlk(virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>),
}
```

### MmioDevice Trait

```rust
pub trait MmioDevice {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64>;
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool;
    fn base_address(&self) -> u64;
    fn size(&self) -> u64;
    fn contains(&self, addr: u64) -> bool;
    fn pending_irq(&self) -> Option<u32>;
    fn ack_irq(&mut self);
}
```

- `read()`/`write()` receive **offsets** relative to `base_address()`
- `size` = access width in bytes (1, 2, 4, or 8)
- `pending_irq()` returns SPI INTID if device wants to assert an interrupt

### Routing

Array-based: `devices: [Option<Device>; 8]`. On MMIO access, scan all devices for `dev.contains(addr)`. First match wins.

### Global Device Manager

`GlobalDeviceManager` in `src/global.rs` wraps `DeviceManager` in an `UnsafeCell` for access from the exception handler (single-threaded, no concurrency).

## PL011 UART Emulation

**File**: `src/devices/pl011/emulator.rs`
**Base**: 0x09000000 (not mapped in Stage-2 — all accesses trap)
**SPI**: INTID 33 (SPI 1)

### Full Trap-and-Emulate

All guest UART accesses trap to VirtualUart:

| Register | Offset | Read | Write |
|----------|--------|------|-------|
| UARTDR | 0x000 | Pop from RX buffer | Output char to physical UART |
| UARTFR | 0x018 | RX empty/full flags | N/A |
| UARTCR | 0x030 | Control register shadow | Stored |
| UARTIMSC | 0x038 | Interrupt mask | Stored |
| UARTRIS | 0x03C | Raw interrupt status | RX pending bit |
| UARTMIS | 0x040 | Masked interrupt status | RIS & IMSC |
| UARTICR | 0x044 | N/A | Clear interrupt |
| UARTPeriphID | 0xFE0-0xFEC | ARM PL011 ID bytes | N/A |
| UARTPCellID | 0xFF0-0xFFC | PrimeCell ID bytes | N/A |

**PeriphID/PrimeCellID**: Required by Linux `amba-pl011` driver for probe. Returns standard ARM PL011 identification bytes.

### RX Path

```
Physical UART IRQ (INTID 33)
  → handle_irq_exception(): read all bytes from physical UART FIFO
  → Push to UART_RX ring buffer (lock-free, IRQ-safe)
  → Return false (exit to run_smp())

run_smp() loop:
  → Drain UART_RX → VirtualUart.push_rx(ch)
  → If VirtualUart.pending_irq() → inject_spi(33)
  → Guest reads UARTDR → pops from VirtualUart RX buffer
```

### TX Path

Guest writes UARTDR → `VirtualUart.write()` → `output_char()` → writes directly to physical UART UARTDR at 0x09000000.

## Virtio-mmio / Virtio-blk

### Transport Layer

**File**: `src/devices/virtio/mmio.rs`
**Base**: 0x0a000000 (first QEMU virt virtio-mmio slot)
**SPI**: INTID 48 (SPI 16)

`VirtioMmioTransport<T: VirtioBackend>` implements the virtio-mmio register layout:

| Register | Offset | Description |
|----------|--------|-------------|
| MagicValue | 0x000 | `0x74726976` ("virt") |
| Version | 0x004 | 2 (virtio-mmio v2) |
| DeviceID | 0x008 | From backend (2 = block) |
| VendorID | 0x00C | `0x554D4551` ("QEMU") |
| DeviceFeatures | 0x010 | Feature bits (sel by DeviceFeaturesSel) |
| DriverFeatures | 0x020 | Guest-negotiated features |
| QueueSel | 0x030 | Queue index selector |
| QueueNumMax | 0x034 | Max queue size (256) |
| QueueNum | 0x038 | Guest-set queue size |
| QueueReady | 0x044 | Queue activation |
| QueueNotify | 0x050 | **Write triggers request processing** |
| InterruptStatus | 0x060 | Pending interrupt flags |
| InterruptACK | 0x064 | Clear interrupt flags |
| Status | 0x070 | Device status state machine |
| QueueDescLow/High | 0x080/0x084 | Descriptor table GPA |
| QueueDriverLow/High | 0x090/0x094 | Available ring GPA |
| QueueDeviceLow/High | 0x0A0/0x0A4 | Used ring GPA |
| ConfigGeneration | 0x0FC | Config space change counter |
| Config space | 0x100+ | Backend-specific (capacity, etc.) |

### Virtio-blk Backend

**File**: `src/devices/virtio/blk.rs`
**Disk image**: Loaded at 0x58000000 by QEMU `-device loader`
**Size**: 2MB (4096 x 512-byte sectors)

Request processing (triggered by QueueNotify write):

```
1. Read descriptor chain from Descriptor Table (GPA)
2. Parse VirtioBlkReqHeader: type (IN/OUT), sector
3. For VIRTIO_BLK_T_IN (read):
   → copy_nonoverlapping(disk + sector*512, data_buf, len)
4. For VIRTIO_BLK_T_OUT (write):
   → copy_nonoverlapping(data_buf, disk + sector*512, len)
5. Write status byte (0 = OK) to status descriptor
6. Update Used Ring: add entry with descriptor index + bytes written
7. inject_spi(48) → queues completion interrupt
8. flush_pending_spis_to_hardware() → immediate LR injection
```

**Identity mapping**: Since IPA==HPA, guest buffer addresses can be used directly with `core::ptr::copy_nonoverlapping()`.

### Virtqueue Structure

```
Descriptor Table: array of {addr, len, flags, next}
Available Ring: {flags, idx, ring[]} — guest writes
Used Ring: {flags, idx, ring[{id, len}]} — device writes
```

## GICD / GICR Emulation

See [gic-emulation.md](gic-emulation.md).

## Adding a New Device

1. **Create device module**: `src/devices/mydev/mod.rs`
2. **Implement `MmioDevice`**: `read()`, `write()`, `base_address()`, `size()`
3. **Add variant to `Device` enum**: `MyDev(mydev::VirtualMyDev)`
4. **Add match arms** in all `Device` impl methods (read, write, base_address, size, pending_irq, ack_irq)
5. **Register in `Vm::new()`**: `DEVICES.register_device(Device::MyDev(...))`
6. **Stage-2 setup**: If the device's address range should trap, ensure it's NOT mapped (or explicitly unmapped)
7. **DTB**: Add device node to `guest/linux/guest.dts` with compatible string and interrupt

## Source Files

| File | Role |
|------|------|
| `src/devices/mod.rs` | `Device` enum, `MmioDevice` trait, `DeviceManager` |
| `src/devices/pl011/emulator.rs` | VirtualUart — PL011 UART emulation |
| `src/devices/gic/distributor.rs` | VirtualGicd — GICD shadow state |
| `src/devices/gic/redistributor.rs` | VirtualGicr — GICR trap-and-emulate |
| `src/devices/virtio/mmio.rs` | VirtioMmioTransport — virtio-mmio register handling |
| `src/devices/virtio/blk.rs` | VirtioBlk — block device backend |
| `src/arch/aarch64/hypervisor/decode.rs` | MmioAccess — instruction decoding |
| `src/global.rs` | GlobalDeviceManager, UART_RX ring buffer |
