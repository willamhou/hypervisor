# ARM64 Hypervisor

A bare-metal Type-1 hypervisor for ARM64 (AArch64) written in Rust. Runs at EL2 and manages guest VMs at EL1, targeting QEMU virt machine. Boots Linux 6.12.12 to BusyBox shell with 4 vCPUs, virtio-blk storage, virtio-net networking, multi-VM support, and FF-A v1.1 proxy with VM-to-VM memory sharing. Dual boot modes: NS-EL2 hypervisor via TF-A boot chain (BL33) and S-EL2 SPMC (BL32) managing Secure Partitions. Integrates with pKVM at NS-EL2 for protected VM management.

## Project Goals

- Build a production-style ARM64 Type-1 hypervisor from scratch for educational and research purposes
- Full hardware-assisted virtualization: Stage-2 MMU, GICv3 virtual interface, HW timer injection
- Boot real operating systems (Linux, Zephyr) as guest VMs
- Dual-world architecture: S-EL2 SPMC managing Secure Partitions + pKVM at NS-EL2 for Normal World

## Features

- **S-EL2 SPMC**: Hypervisor as BL32 SPMC at S-EL2 -- manages Secure Partitions (SP1 Hello + SP2 IRQ), DIRECT_REQ/RESP messaging, memory sharing, NS interrupt preemption, secure vIRQ injection via HCR_EL2.VI
- **TF-A Boot Chain**: BL1->BL2->BL31(SPMD)->BL32(SPMC)->BL33(hypervisor) with manifest FDT parsing
- **pKVM Integration**: pKVM at NS-EL2 + our SPMC at S-EL2, 4-CPU SMP, FF-A v1.1 discovery, AOSP android16-6.12 kernel
- **FF-A v1.1 Proxy**: Firmware Framework for Arm -- SMC interception, stub SPMC with 2 SPs, page ownership validation, VM-to-VM memory sharing (RETRIEVE/RELINQUISH), descriptor parsing, SMC forwarding to EL3
- **Virtio-net + VSwitch**: L2 virtual switch with MAC learning, per-VM MAC addresses, inter-VM frame forwarding, auto-IP assignment
- **Multi-VM**: 2 Linux VMs time-sliced on 1 pCPU with per-VM Stage-2, VMID-tagged TLBs, independent device managers
- **Multi-pCPU**: 4 vCPUs on 4 physical CPUs (1:1 affinity) with PSCI boot, TPIDR_EL2 per-CPU context
- **SMP Scheduling**: 4 vCPUs on 1 pCPU with cooperative (WFI) + preemptive (10ms CNTHP timer) scheduling
- **DTB Runtime Parsing**: Discovers UART, GIC, RAM, CPU count from host device tree at boot
- **Virtio-blk**: Block device via virtio-mmio transport with in-memory disk image backend
- **GICv3 Emulation**: Full GICD/GICR trap-and-emulate with write-through, List Register injection, SGI/IPI emulation
- **Device Emulation**: PL011 UART (TX+RX), PL031 RTC, GIC Distributor/Redistributor, virtio-mmio
- **Stage-2 Memory**: Dynamic page tables (2MB blocks + 4KB pages), VMID-tagged TLBs, heap gap protection
- **Linux Guest Boot**: Boots Linux 6.12.12 (custom defconfig) to BusyBox shell with 4 CPUs, virtio-blk, virtio-net
- **Android Boot**: Android-configured kernel (PL031 RTC, Binder IPC, binderfs, minimal init, 1GB RAM)

## Current Status

**Progress**: Milestones 0-4 complete, including FF-A v1.1, TF-A boot chain, S-EL2 SPMC, and pKVM integration
**Tests**: 33 test suites (~347 assertions), all passing
**Code**: ~23,000 lines (src + tests + asm)

### Milestone Overview

```
M0: Project Setup          ████████████████████ 100%
M1: MVP Virtualization     ████████████████████ 100%
M2: Enhanced Features      ████████████████████ 100%
    2.1 GICv3 Virtual IF   ████████████████████ 100%
    2.2 Dynamic Memory     ████████████████████ 100%
    2.3 Multi-vCPU         ████████████████████ 100%
    2.4 API Documentation  ████████████████████ 100%
M3: FF-A v1.1              ████████████████████ 100%
M4: Secure EL2 / SPMC     ████████████████████ 100%
    4.1 TF-A Boot Chain    ████████████████████ 100%
    4.2 BL33 Hypervisor    ████████████████████ 100%
    4.3 S-EL2 SPMC (BL32)  ████████████████████ 100%
    4.4 SP Boot + Dispatch ████████████████████ 100%
    4.5 pKVM Integration   ████████████████████ 100%
M5: RME & CCA             ░░░░░░░░░░░░░░░░░░░░   0%
```

### Latest Updates

- **pKVM + SPMC**: pKVM at NS-EL2 + our SPMC at S-EL2 -- boots to BusyBox shell with 4 CPUs
- **E2E Memory Sharing**: NWd SHARE -> SP RETRIEVE -> SP write -> SP RELINQUISH -> NWd verify -> NWd RECLAIM
- **Multi-SP Dispatch**: SP1 (Hello) + SP2 (IRQ) with per-SP INTID ownership and cross-SP preemption
- **NS Interrupt Preemption**: IRQ during SP -> FFA_INTERRUPT -> FFA_RUN resume
- **S-EL2 Stage-1 MMU**: Identity map with NS=1 for NWd DRAM, secondary CPU warm-boot

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
make                    # Build hypervisor
make run                # Build and run tests in QEMU (exit: Ctrl+A then X)
make run-linux          # Boot Linux guest (4 vCPUs on 1 pCPU, virtio-blk)
make run-linux-smp      # Boot Linux guest (4 vCPUs on 4 pCPUs)
make run-multi-vm       # Boot 2 Linux VMs time-sliced on 1 pCPU
make run-android        # Boot Android-configured kernel (PL031 RTC, Binder, 1GB RAM)
make run-sel2           # Boot TF-A with BL32 at S-EL2
make run-tfa-linux      # Boot TF-A -> hypervisor (BL33) -> Linux
make run-spmc           # Boot TF-A -> our SPMC (BL32) at S-EL2
make run-tfa-linux-ffa  # Boot TF-A -> SPMC -> hypervisor (BL33) -> Linux (FF-A)
make run-pkvm           # Boot pKVM (NS-EL2) + our SPMC (S-EL2)
make debug              # Run with GDB server on port 1234
make clippy             # Run linter
make fmt                # Format code
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
┌─────────────────────────────────────────────────────────────┐
│  Guest OS (Linux / Android / Zephyr)              NS-EL1   │
├─────────────────────────────────────────────────────────────┤
│  pKVM (protected KVM)                             NS-EL2   │
│  - Normal World VM management                              │
├─────────────────────────────────────────────────────────────┤
│  Secure Partitions (SP1 Hello, SP2 IRQ)           S-EL1    │
├─────────────────────────────────────────────────────────────┤
│  Our Hypervisor (SPMC role)                       S-EL2    │
│  - SPMC event loop, FF-A dispatch                          │
│  - Secure Stage-2, SP lifecycle                            │
├─────────────────────────────────────────────────────────────┤
│  TF-A BL31 + SPMD                                EL3      │
│  - SMC relay, world switch                                 │
├─────────────────────────────────────────────────────────────┤
│  Hardware (QEMU virt)                                      │
│  - GICv3, PL011, PL031, Generic Timer, virtio-mmio        │
└─────────────────────────────────────────────────────────────┘
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
  ├── SMC → PSCI or FF-A proxy or forward to EL3
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
| PL031 RTC | `src/devices/pl031.rs` | Real-time clock emulation |
| GIC Distributor | `src/devices/gic/` | GICD/GICR emulation |
| Virtio-blk | `src/devices/virtio/blk.rs` | Block device backend |
| Virtio-net | `src/devices/virtio/net.rs` | Network device backend |
| VSwitch | `src/vswitch.rs` | L2 virtual switch with MAC learning |
| FF-A Proxy | `src/ffa/proxy.rs` | FF-A v1.1 SMC interception and dispatch |
| FF-A Stage-2 Walker | `src/ffa/stage2_walker.rs` | PTE SW bits for page ownership |
| FF-A Stub SPMC | `src/ffa/stub_spmc.rs` | Simulated Secure Partitions |
| FF-A Descriptors | `src/ffa/descriptors.rs` | Memory region descriptor parsing |
| FF-A SMC Forward | `src/ffa/smc_forward.rs` | SMC forwarding to EL3 |
| FF-A Notifications | `src/ffa/notifications.rs` | Per-partition notification bitmaps |
| SPMC Handler | `src/spmc_handler.rs` | S-EL2 SPMC event loop + FF-A dispatch |
| SP Context | `src/sp_context.rs` | Per-SP state machine, INTID ownership |
| Secure Stage-2 | `src/secure_stage2.rs` | VSTTBR/VSTCR config for SP isolation |
| S-EL2 MMU | `src/sel2_mmu.rs` | S-EL2 Stage-1 identity map (NS=1 for NWd) |
| Manifest Parser | `src/manifest.rs` | TOS_FW_CONFIG DTB parsing |
| Region Registry | `src/mm/region_registry.rs` | Stage-2 region registration |
| Log | `src/log.rs` | Structured logging macros |
| DTB Parser | `src/dtb.rs` | Runtime hardware discovery from host DTB |
| Global State | `src/global.rs` | Per-VM atomics, UART RX ring, pending SGIs/SPIs |
| Guest Loader | `src/guest_loader.rs` | Linux/Zephyr boot configuration |
| Arch Constants | `src/arch/aarch64/defs.rs` | ARM64 named constants |
| Board Constants | `src/platform.rs` | QEMU virt platform constants |

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

33 test suites (~347 assertions) run automatically on `make run`:

| Test | Description |
|------|-------------|
| `test_dtb` | DTB parsing, PlatformInfo defaults, GICR helpers |
| `test_allocator` | Bump allocator page allocation |
| `test_heap` | Heap initialization and global allocator |
| `test_dynamic_pagetable` | Dynamic Stage-2 page table creation + 4KB unmap |
| `test_multi_vcpu` | Multi-vCPU creation and management |
| `test_scheduler` | Round-robin scheduler logic |
| `test_vm_scheduler` | VM-integrated scheduling lifecycle |
| `test_mmio` | MMIO device emulation |
| `test_gicv3_virt` | GICv3 virtual interface and List Registers |
| `test_complete_interrupt` | End-to-end interrupt flow with GICv3 LRs |
| `test_guest` | Basic guest execution and hypercall |
| `test_guest_loader` | Guest loader configuration |
| `test_simple_guest` | Simple guest boot and exit |
| `test_decode` | MMIO instruction decode (ISS + instruction paths) |
| `test_gicd` | GICD shadow state (CTLR, ISENABLER, IROUTER) |
| `test_gicr` | GICR per-vCPU state (TYPER, WAKER, ISENABLER0) |
| `test_global` | PendingCpuOn atomics + UartRxRing SPSC buffer |
| `test_guest_irq` | Per-VM PENDING_SGIS/PENDING_SPIS bitmask operations |
| `test_device_routing` | DeviceManager registration, routing, accessors |
| `test_vm_state_isolation` | Per-VM SGI/SPI/online_mask/vcpu_id independence |
| `test_vmid_vttbr` | VMID 0/1 encoding in VTTBR_EL2 |
| `test_multi_vm_devices` | Per-VM device manager isolation |
| `test_vm_activate` | VM Stage-2 activation |
| `test_pl031` | PL031 RTC time read/write |
| `test_net_rx_ring` | NetRxRing SPSC: empty/store/take/fill/overflow/wraparound |
| `test_vswitch` | VSwitch: flood/MAC learning/broadcast/no-self/capacity |
| `test_virtio_net` | VirtioNet: device_id/features/queues/config/mac |
| `test_page_ownership` | Stage-2 PTE SW bits: ownership transitions |
| `test_ffa` | FF-A proxy: VERSION/ID_GET/FEATURES/RXTX/messaging/memory/VM-to-VM |
| `test_spmc_handler` | SPMC dispatch: VERSION/FEATURES/DIRECT_REQ/memory/multi-SP/FFA_RUN/helpers/notifications (95 assertions) |
| `test_sp_context` | SpContext: state machine, CAS, illegal transitions, INTID ownership, IRQ overflow (58 assertions) |
| `test_secure_stage2` | SecureStage2Config: VSTTBR/VSTCR validation (4 assertions) |
| `test_log` | Structured logging macros |
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
- Virtio-net + VSwitch: L2 virtual switch, inter-VM networking, auto-IP
- FF-A v1.1 proxy: SMC trap, stub SPMC, page ownership, descriptor parsing, SMC forwarding, notifications, indirect messaging
- VM-to-VM FF-A memory sharing: MEM_RETRIEVE_REQ/RELINQUISH with dynamic Stage-2 mapping
- Android boot: PL031 RTC emulation, Binder IPC, binderfs, minimal init, 1GB RAM
- TF-A boot chain: BL1->BL2->BL31(SPMD)->BL32(SPMC)->BL33(hypervisor), manifest FDT parsing
- S-EL2 SPMC: SPMC event loop, SP1 (Hello) + SP2 (IRQ) at S-EL1, DIRECT_REQ/RESP, Secure Stage-2
- End-to-end DIRECT_REQ: NS proxy -> SPMD -> SPMC -> SP (x4 += 0x1000 proof)
- RXTX + PARTITION_INFO_GET forwarding, Linux FF-A driver discovery
- NS interrupt preemption: IRQ during SP -> FFA_INTERRUPT -> FFA_RUN resume
- Multi-SP + secure vIRQ injection: per-SP INTID ownership, HCR_EL2.VI, cross-SP preemption
- pKVM integration: pKVM at NS-EL2 + our SPMC at S-EL2, 4-CPU SMP, FF-A v1.1 discovery
- E2E memory sharing: NWd SHARE -> SP RETRIEVE -> SP write -> SP RELINQUISH -> NWd verify -> NWd RECLAIM

### In Progress

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
