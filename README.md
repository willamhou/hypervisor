# ARM64 Hypervisor

A bare-metal Type-1 hypervisor for ARM64 (AArch64) written in Rust. Runs at EL2 and manages guest VMs at EL1, targeting QEMU virt machine. Boots Linux 6.12.12 to BusyBox shell with 4 vCPUs, virtio-blk storage, and multi-VM support.

## Project Goals

- Build a production-style ARM64 Type-1 hypervisor from scratch for educational and research purposes
- Full hardware-assisted virtualization: Stage-2 MMU, GICv3 virtual interface, HW timer injection
- Boot real operating systems (Linux, Zephyr) as guest VMs
- Prepare architecture for future ARM security extensions: FF-A, Secure EL2 (TEE), RME/CCA

## Features

- **Multi-VM**: 2 Linux VMs time-sliced on 1 pCPU with per-VM Stage-2, VMID-tagged TLBs, independent device managers
- **Multi-pCPU**: 4 vCPUs on 4 physical CPUs (1:1 affinity) with PSCI boot, TPIDR_EL2 per-CPU context
- **SMP Scheduling**: 4 vCPUs on 1 pCPU with cooperative (WFI) + preemptive (10ms CNTHP timer) scheduling
- **DTB Runtime Parsing**: Discovers UART, GIC, RAM, CPU count from host device tree at boot
- **Virtio-blk**: Block device via virtio-mmio transport with in-memory disk image backend
- **GICv3 Emulation**: Full GICD/GICR trap-and-emulate with write-through, List Register injection, SGI/IPI emulation
- **Device Emulation**: PL011 UART (TX+RX), GIC Distributor/Redistributor, virtio-mmio
- **Stage-2 Memory**: Dynamic page tables (2MB blocks + 4KB pages), VMID-tagged TLBs, heap gap protection
- **Linux Guest Boot**: Boots Linux 6.12.12 (custom defconfig) to BusyBox shell with 4 CPUs and virtio-blk

## Current Status

**Progress**: Milestones 0-2 complete, multi-VM + multi-pCPU + DTB parsing implemented
**Tests**: 24 test suites (~113 assertions), all passing
**Code**: ~10,000 lines (src + tests)

### Milestone Overview

```
M0: Project Setup          ████████████████████ 100%
M1: MVP Virtualization     ████████████████████ 100%
M2: Enhanced Features      ████████████████████ 100%
    2.1 GICv3 Virtual IF   ████████████████████ 100%
    2.2 Dynamic Memory     ████████████████████ 100%
    2.3 Multi-vCPU         ████████████████████ 100%
    2.4 API Documentation  ████████████████████ 100%
M3: FF-A                   ░░░░░░░░░░░░░░░░░░░░   0%
M4: Secure EL2 / TEE      ░░░░░░░░░░░░░░░░░░░░   0%
M5: RME & CCA             ░░░░░░░░░░░░░░░░░░░░   0%
```

### Latest Updates

- **Multi-VM**: 2 Linux VMs time-sliced on 1 pCPU, both boot to BusyBox shell
- **Multi-pCPU**: 4 vCPUs on 4 physical CPUs with PSCI boot and physical IPI delivery
- **DTB Runtime Parsing**: Hardware discovery from host device tree (UART, GIC, RAM, CPU count)
- **Virtio-blk**: Block storage via virtio-mmio transport with in-memory disk backend
- **Linux Boot**: Linux 6.12.12 boots to BusyBox shell with 4 CPUs, virtio-blk, full userspace

## Quick Start

### Prerequisites

- Rust nightly (with `aarch64-unknown-none` target)
- QEMU (`qemu-system-aarch64`)
- ARM64 cross-toolchain (`aarch64-linux-gnu-*`)

```bash
rustup target add aarch64-unknown-none
sudo apt install qemu-system-arm gcc-aarch64-linux-gnu
```

### Build & Run

```bash
make                # Build hypervisor
make run            # Build and run tests in QEMU (exit: Ctrl+A then X)
make run-linux      # Boot Linux guest (4 vCPUs on 1 pCPU, virtio-blk)
make run-linux-smp  # Boot Linux guest (4 vCPUs on 4 pCPUs)
make run-multi-vm   # Boot 2 Linux VMs time-sliced on 1 pCPU
make debug          # Run with GDB server on port 1234
make clippy         # Run linter
make fmt            # Format code
```

### Debugging

```bash
# Terminal 1
make debug

# Terminal 2
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor
(gdb) target remote :1234
(gdb) b rust_main
(gdb) c
```

## Architecture

### Privilege Model

```
┌─────────────────────────────────────────────┐
│  Guest OS (Linux / Zephyr)       EL1        │
│  ─ Uses virtual ICC registers               │
│  ─ Stage-2 translated memory               │
├─────────────────────────────────────────────┤
│  Hypervisor                      EL2        │
│  ─ Exception handling & MMIO emulation      │
│  ─ GICv3 List Register management           │
│  ─ Stage-2 page table control               │
├─────────────────────────────────────────────┤
│  Hardware (QEMU virt)                       │
│  ─ GICv3, PL011, Generic Timer             │
└─────────────────────────────────────────────┘
```

### Exception Handling Flow

```
Guest @ EL1
  │ trap (HVC / Data Abort / WFI / IRQ)
  ▼
Exception Vector (exception.S) ── save context
  │
  ▼
handle_exception() ── decode ESR_EL2
  ├── WFI → check pending timer, inject if ready
  ├── HVC → handle hypercall, advance PC
  ├── Data Abort → decode instruction → MMIO emulation
  └── IRQ → acknowledge, inject via List Register
  │
  ▼
Restore context → ERET back to guest
```

### Key Components

| Module | Path | Description |
|--------|------|-------------|
| vCPU | `src/vcpu.rs` | Virtual CPU with state machine and interrupt state |
| VM | `src/vm.rs` | VM management, Stage-2 setup, up to 8 vCPUs |
| Scheduler | `src/scheduler.rs` | Round-robin vCPU scheduler |
| Exception Handler | `src/arch/aarch64/hypervisor/exception.rs` | ESR_EL2 decode, MMIO routing |
| Instruction Decoder | `src/arch/aarch64/hypervisor/decode.rs` | Load/store decode for MMIO |
| Stage-2 MMU | `src/arch/aarch64/mm/mmu.rs` | Page tables, dynamic allocation |
| GICv3 | `src/arch/aarch64/peripherals/gicv3.rs` | List Registers, virtual interface |
| Timer | `src/arch/aarch64/peripherals/timer.rs` | Virtual timer, CNTHCTL config |
| Device Manager | `src/devices/mod.rs` | MMIO device routing |
| PL011 UART | `src/devices/pl011/` | UART emulation |
| GIC Distributor | `src/devices/gic/` | GICD emulation |
| Arch Constants | `src/arch/aarch64/defs.rs` | ARM64 named constants |
| Board Constants | `src/platform.rs` | QEMU virt platform constants |
| Arch Traits | `src/arch/traits.rs` | Portable trait definitions |
| DTB Parser | `src/dtb.rs` | Runtime hardware discovery from host DTB |
| Virtio-blk | `src/devices/virtio/` | virtio-mmio transport + block device backend |
| Global State | `src/global.rs` | Per-VM atomics, UART RX ring, pending SGIs/SPIs |
| Guest Loader | `src/guest_loader.rs` | Linux/Zephyr boot configuration |

### Memory Layout

- **IPA Space**: 48-bit, 4KB granule, 2MB block mapping
- **Stage-2**: Identity mapping (GPA == HPA)
- **Guest RAM**: 0x40000000 (base), 0x48000000 (kernel load address)
- **GIC**: 0x08000000 (GICD), 0x080A0000 (GICR) — trap-and-emulate
- **UART**: 0x09000000 (PL011) — emulated
- **Heap**: 0x41000000, 16MB bump allocator

### Interrupt Handling

- **GICv3 List Registers**: Hardware-assisted virtual interrupt injection
- **HW=1 injection**: Links virtual INTID to physical INTID; guest EOI auto-deactivates physical interrupt
- **EOImode=1**: Priority drop on EOIR, deactivation via DIR (software) or HW bit (hardware)
- **Timer**: Virtual Timer PPI 27, masked at EL2 after injection, re-armed by guest

## Testing

24 test suites (~113 assertions) run automatically on `make run`:

| Test | Description |
|------|-------------|
| `test_allocator` | Bump allocator page allocation |
| `test_heap` | Heap initialization and global allocator |
| `test_dynamic_pagetable` | Dynamic Stage-2 page table creation |
| `test_multi_vcpu` | Multi-vCPU creation and management |
| `test_scheduler` | Round-robin scheduler logic |
| `test_vm_scheduler` | VM-integrated scheduling |
| `test_gicv3_virt` | GICv3 virtual interface and List Registers |
| `test_guest` | Basic guest execution and hypercall |
| `test_mmio` | MMIO device emulation |
| `test_complete_interrupt` | End-to-end interrupt flow with GICv3 LRs |
| `test_guest_irq` | SGI/SPI pending bitmask operations |
| `test_guest_loader` | Guest loader configuration |
| `test_simple_guest` | Simple guest boot and exit |
| `test_decode` | MMIO instruction decode (ISS + instruction paths) |
| `test_gicd` | GICD shadow state (CTLR, ISENABLER, IROUTER) |
| `test_gicr` | GICR per-vCPU state (TYPER, WAKER, ISENABLER0) |
| `test_global` | PendingCpuOn atomics + UartRxRing SPSC buffer |
| `test_device_routing` | DeviceManager registration, routing, accessors |
| `test_vm_state_isolation` | Per-VM state isolation |
| `test_vmid_vttbr` | VMID encoding in VTTBR |
| `test_multi_vm_devices` | Per-VM device manager isolation |
| `test_vm_activate` | VM Stage-2 activation |
| `test_dtb` | DTB runtime parsing validation |
| `test_guest_interrupt` | Guest interrupt injection + exception vector |

## Roadmap

### Completed

- M0: Project setup, QEMU boot, UART output
- M1: vCPU framework, Stage-2 MMU, exception handling, device emulation, interrupt injection
- M2: GICv3 virtual interface, dynamic memory, multi-vCPU scheduler, API documentation
- Linux 6.12.12 guest boot to BusyBox shell with 4 vCPUs and virtio-blk
- Multi-pCPU: 4 vCPUs on 4 physical CPUs with PSCI boot, TPIDR_EL2, physical IPI
- Multi-VM: 2 Linux VMs time-sliced on 1 pCPU with per-VM Stage-2 and VMID TLBs
- DTB runtime parsing: hardware discovery from host device tree
- Code refactoring: named constants, dead code removal, architecture traits

### Planned

- **M3 — FF-A**: Firmware Framework for Armv8-A — hypervisor endpoint, direct messaging, memory sharing
- **M4 — Secure EL2**: TEE support, S-EL2 implementation, OP-TEE integration
- **M5 — RME & CCA**: Realm Management Extension, Confidential Compute Architecture

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) for the full roadmap.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and contribution guidelines.

## References

- [ARM Architecture Reference Manual (ARMv8-A)](https://developer.arm.com/documentation/ddi0487/latest)
- [ARM GIC Architecture Specification](https://developer.arm.com/documentation/ihi0069/latest)
- [ARM FF-A Specification](https://developer.arm.com/documentation/den0077/latest)
- [Hafnium](https://github.com/TF-Hafnium/hafnium) — Reference hypervisor
- [KVM/ARM](https://www.kernel.org/doc/html/latest/virt/kvm/arm/index.html) — Linux KVM ARM implementation

## License

MIT

---

**Author**: [willamhou](https://github.com/willamhou)
