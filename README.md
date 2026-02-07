# ARM64 Hypervisor

A bare-metal Type-1 hypervisor for ARM64 (AArch64) written in Rust. Runs at EL2 and manages guest VMs at EL1, targeting QEMU virt machine. Successfully boots Linux 6.12 (Debian arm64).

## Project Goals

- Build a production-style ARM64 Type-1 hypervisor from scratch for educational and research purposes
- Full hardware-assisted virtualization: Stage-2 MMU, GICv3 virtual interface, HW timer injection
- Boot real operating systems (Linux, Zephyr) as guest VMs
- Prepare architecture for future ARM security extensions: FF-A, Secure EL2 (TEE), RME/CCA

## Features

- **vCPU Management**: Complete virtual CPU abstraction with state machine, context save/restore, multi-vCPU scheduling
- **Stage-2 Memory**: Dynamic page table allocation (bump allocator + heap), 2MB block identity mapping, NORMAL/DEVICE attributes
- **GICv3 Virtual Interface**: List Register-based interrupt injection with HW=1 linking, EOImode=1 for correct deactivation
- **Timer Virtualization**: Virtual timer (PPI 27) with physical-virtual HW linking, automatic EOI via guest virtual EOI
- **Device Emulation**: Trap-and-emulate MMIO framework with PL011 UART and GIC Distributor
- **GIC Passthrough**: Guest direct access to GICD/GICR, virtual ICC registers via ICH_HCR_EL2
- **Linux Guest Boot**: Boots Linux 6.12 (Debian arm64) with full kernel initialization
- **Architecture Traits**: Portable trait abstractions for future RISC-V support

## Current Status

**Progress**: Milestone 1 complete, Milestone 2 (Sprint 2.1-2.4) complete, code refactored
**Tests**: 15 test files, all passing
**Code**: ~7,000 lines (src + tests)

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

- **Code Refactoring**: Extracted ~200 magic numbers into named constants (`defs.rs`, `platform.rs`), removed dead code, converted `static mut` to atomics, added architecture-portable traits
- **Linux Boot**: Linux 6.12 (Debian arm64) boots fully under the hypervisor with HW=1 timer virtualization (reaches "VFS: Unable to mount root fs" — expected without rootfs)
- **GICv3**: List Register-based interrupt injection, HW bit linking for timer auto-deactivation
- **Multi-vCPU**: Round-robin scheduler, up to 8 vCPUs per VM

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
make              # Build hypervisor
make run          # Build and run tests in QEMU (exit: Ctrl+A then X)
make run-linux    # Boot Linux guest (requires kernel Image + DTB)
make debug        # Run with GDB server on port 1234
make clippy       # Run linter
make fmt          # Format code
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
| Guest Loader | `src/guest_loader.rs` | Linux/Zephyr boot configuration |

### Memory Layout

- **IPA Space**: 48-bit, 4KB granule, 2MB block mapping
- **Stage-2**: Identity mapping (GPA == HPA)
- **Guest RAM**: 0x40000000 (base), 0x48000000 (kernel load address)
- **GIC**: 0x08000000 (GICD), 0x080A0000 (GICR) — passthrough
- **UART**: 0x09000000 (PL011) — emulated
- **Heap**: 0x41000000, 16MB bump allocator

### Interrupt Handling

- **GICv3 List Registers**: Hardware-assisted virtual interrupt injection
- **HW=1 injection**: Links virtual INTID to physical INTID; guest EOI auto-deactivates physical interrupt
- **EOImode=1**: Priority drop on EOIR, deactivation via DIR (software) or HW bit (hardware)
- **Timer**: Virtual Timer PPI 27, masked at EL2 after injection, re-armed by guest

## Testing

All tests run automatically on `make run`. 15 test files covering:

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
| `test_timer` | Timer interrupt detection at EL2 |
| `test_mmio` | MMIO device emulation |
| `test_complete_interrupt` | End-to-end interrupt flow with GICv3 LRs |
| `test_guest_irq` | Guest interrupt handling |
| `test_guest_interrupt` | Guest interrupt injection |
| `test_guest_loader` | Guest loader configuration |
| `test_simple_guest` | Simple guest boot and exit |

```
$ make run
...
[TEST] Allocator Test PASSED
[TEST] Heap Test PASSED
[TEST] Dynamic Page Table Test PASSED
[TEST] Multi-vCPU Test PASSED
[TEST] Scheduler Test PASSED
[TEST] VM Scheduler Test PASSED
[TEST] GICv3 Virtualization Test PASSED
[TEST] Guest Execution Test PASSED
[TEST] Timer Interrupt Test PASSED
[TEST] MMIO Device Test PASSED
[TEST] Complete Interrupt Test PASSED
[TEST] Guest Loader Test PASSED
[TEST] Simple Guest Test PASSED

========================================
All Sprints Complete (2.1-2.4)
========================================
```

## Roadmap

### Completed

- M0: Project setup, QEMU boot, UART output
- M1: vCPU framework, Stage-2 MMU, exception handling, device emulation, interrupt injection
- M2: GICv3 virtual interface, dynamic memory, multi-vCPU scheduler, API documentation
- Linux 6.12 guest boot with HW=1 timer virtualization
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
