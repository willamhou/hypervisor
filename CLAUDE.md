# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ARM64 Type-1 bare-metal hypervisor written in Rust (no_std) with ARM64 assembly. Runs at EL2 (hypervisor exception level) and manages guest VMs at EL1. Targets QEMU virt machine. Boots Linux 6.12.12 to BusyBox shell with 4 vCPUs and virtio-blk storage.

## Build Commands

```bash
make              # Build hypervisor
make run          # Build + run in QEMU — runs 40 unit tests automatically (exit: Ctrl+A then X)
make run-linux    # Build + boot Linux guest (--features linux_guest, 4 vCPUs, virtio-blk)
make run-guest GUEST_ELF=/path/to/zephyr.elf  # Boot Zephyr guest (--features guest)
make debug        # Build + run with GDB server on port 1234
make clean        # Clean build artifacts
make check        # Check code without building
make clippy       # Run linter
make fmt          # Format code
```

**Feature flags** (Cargo features, selected via Makefile targets):
- `(default)` — unit tests only, no guest boot
- `guest` — Zephyr guest loading
- `linux_guest` — Linux guest with DynamicIdentityMapper, GICR trap-and-emulate, virtio-blk

**Toolchain requirements**: Rust nightly, `aarch64-linux-gnu-gcc`, `aarch64-linux-gnu-ar`, `aarch64-linux-gnu-objcopy`, `qemu-system-aarch64`

## Architecture

### Privilege Model
- **EL2**: Hypervisor — exception handling, Stage-2 page tables, GIC virtual interface
- **EL1**: Guest — Linux kernel or Zephyr RTOS
- **Stage-2 Translation**: Identity mapping (GPA == HPA), 2MB blocks + 4KB pages

### Core Abstractions

| Type | File | Role |
|------|------|------|
| `Vm` | `src/vm.rs` | VM lifecycle, Stage-2 setup, `run_smp()` scheduler loop |
| `Vcpu` | `src/vcpu.rs` | State machine (Uninitialized→Ready→Running→Stopped), context save/restore |
| `VcpuContext` | `src/arch/aarch64/regs.rs` | Guest registers (x0-x30, SP, PC, SPSR, system regs) |
| `VcpuArchState` | `src/arch/aarch64/vcpu_arch_state.rs` | Per-vCPU GIC LRs, timer, EL1 sysregs, PAC keys |
| `DeviceManager` | `src/devices/mod.rs` | Enum-dispatch MMIO routing to emulated devices |
| `Scheduler` | `src/scheduler.rs` | Round-robin vCPU scheduler with block/unblock |
| `ExitReason` | `src/arch/aarch64/regs.rs` | VM exit causes: WfiWfe, HvcCall, DataAbort, etc. |

### Exception Handling Flow
```
Guest @ EL1
  ↓ trap (Data Abort, HVC, WFI, MSR/MRS)
Exception Vector (arch/aarch64/exception.S) — save context
  ↓
handle_exception() (src/arch/aarch64/hypervisor/exception.rs)
  ├─ WFI → return false (exit to scheduler)
  ├─ HVC → handle_psci() (CPU_ON, CPU_OFF, SYSTEM_RESET)
  ├─ Data Abort → HPFAR_EL2 for IPA → decode instruction → MMIO dispatch
  ├─ MSR/MRS trap → handle ICC_SGI1R_EL1 (SGI emulation), sysreg emulation
  └─ IRQ → handle INTID 26 (preemption), 27 (vtimer), 33 (UART RX)
  ↓ advance PC, restore context
ERET back to guest
```

### SMP / Multi-vCPU

The `run_smp()` loop in `src/vm.rs` runs all vCPUs on a single physical CPU via cooperative + preemptive scheduling:

1. Check `PENDING_CPU_ON` atomics → `boot_secondary_vcpu()` (PSCI CPU_ON)
2. Wake vCPUs with pending SGIs/SPIs → `scheduler.unblock()`
3. Pick next vCPU (round-robin) → set `CURRENT_VCPU_ID`
4. Drain UART RX ring → inject SPI 33
5. Inject pending SGIs/SPIs into `arch_state.ich_lr[]`
6. Arm CNTHP preemption timer (10ms, INTID 26)
7. `vcpu.run()` → save/restore arch state → `enter_guest()` → ERET
8. Handle exit: WFI→block, preemption→yield, real exit→remove

**SGI/IPI emulation**: ICC_SGI1R_EL1 trapped via ICH_HCR_EL2.TALL1=1 → decoded (TargetList[15:0], Aff1[23:16], INTID[27:24]) → `PENDING_SGIS[vcpu_id]` atomics → injected before next entry.

### GIC Emulation

| Component | Address | Mode | Implementation |
|-----------|---------|------|----------------|
| GICD | 0x08000000 | Passthrough + shadow | `VirtualGicd` tracks IROUTER for SPI routing |
| GICR 0,1,3 | 0x080A0000+ | Trap-and-emulate | `VirtualGicr` (Stage-2 unmapped, 4KB pages) |
| GICR 2 | 0x080E0000 | Passthrough | QEMU bug workaround (L3 unmap causes external aborts) |
| ICC regs | System regs | Virtual | ICH_HCR_EL2.En=1 redirects to ICV_* at EL1 |
| ICC_SGI1R | System reg | Trapped | TALL1=1, decoded for IPI emulation |

**List Register injection**: 4 LRs (ICH_LR0-3_EL2). HW=1 for vtimer (INTID 27) enables physical-virtual EOI linkage. EOImode=1 for proper priority drop / deactivation split.

### Virtio-blk

```
VirtioMmioTransport<VirtioBlk>  @ 0x0a000000 (SPI 16 = INTID 48)
  ├─ MMIO registers (virtio-mmio spec)
  ├─ Virtqueue (descriptor table + available ring + used ring)
  └─ VirtioBlk backend (disk image at 0x58000000, loaded by QEMU)
```

Guest writes QueueNotify → `process_request()` → read/write disk image via `copy_nonoverlapping` (identity-mapped) → update used ring → `inject_spi(48)` → `flush_pending_spis_to_hardware()`.

### UART (PL011) Emulation

Full trap-and-emulate (Stage-2 unmapped). TX: guest writes UARTDR → `output_char()` to physical UART. RX: physical IRQ (INTID 33) → `UART_RX` ring buffer → `VirtualUart.push_rx()` → inject SPI 33. Linux amba-pl011 probe requires PeriphID/PrimeCellID registers.

### Memory Layout

| Region | Address | Purpose |
|--------|---------|---------|
| Hypervisor code | 0x40000000 | Linker base (RAM start) |
| Heap | 0x41000000 (16MB) | Page table allocation, `BumpAllocator` |
| DTB | 0x47000000 | Device tree blob |
| Kernel | 0x48000000 | Linux Image load address |
| Initramfs | 0x54000000 | BusyBox initramfs |
| Disk image | 0x58000000 | virtio-blk backing store |
| Guest RAM | 0x48000000-0x68000000 | 512MB declared to Linux |

**Stage-2 mappers**:
- `IdentityMapper` (static, 2MB-only) — used by unit tests (`make run`)
- `DynamicIdentityMapper` (heap-allocated, 2MB+4KB) — used by Linux guest (`make run-linux`), supports `unmap_4kb_page()` for GICR trap setup

**Heap gap**: Heap lies within guest's PA range but is left unmapped in Stage-2 to prevent guest corruption of page tables. Guest kernel never accesses this range (declared memory starts at 0x48000000).

### Global State (`src/global.rs`)

| Global | Type | Purpose |
|--------|------|---------|
| `DEVICES` | `GlobalDeviceManager` | Exception handler MMIO dispatch (UnsafeCell, single-threaded) |
| `PENDING_CPU_ON` | `PendingCpuOn` (atomics) | PSCI CPU_ON signaling between trap handler and run loop |
| `VCPU_ONLINE_MASK` | `AtomicU64` | Bit N = vCPU N online |
| `CURRENT_VCPU_ID` | `AtomicUsize` | Which vCPU is currently executing |
| `PENDING_SGIS` | `[AtomicU32; MAX_VCPUS]` | Per-vCPU pending SGI bitmask |
| `PENDING_SPIS` | `[AtomicU32; MAX_VCPUS]` | Per-vCPU pending SPI bitmask |
| `PREEMPTION_EXIT` | `AtomicBool` | CNTHP timer fired, yield to scheduler |
| `UART_RX` | `UartRxRing` | Lock-free ring buffer, IRQ handler → run loop |

### Device Manager Pattern

Enum-dispatch (no dynamic dispatch / trait objects):
```rust
pub enum Device {
    Uart(pl011::VirtualUart),
    Gicd(gic::VirtualGicd),
    Gicr(gic::VirtualGicr),
    VirtioBlk(virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>),
}
```
Array-based routing: `devices: [Option<Device>; 8]`, scan for `dev.contains(addr)`.

## Build System

- **build.rs**: Cross-compiles `boot.S` and `exception.S` via `aarch64-linux-gnu-gcc`, archives into `libboot.a`, links with `--whole-archive`
- **Target**: `aarch64-unknown-none.json` (custom spec: `llvm-target: aarch64-unknown-none`, `panic-strategy: abort`, `disable-redzone: true`)
- **Linker**: `arch/aarch64/linker.ld` — base at 0x40000000, `.text.boot` first

## Tests

All 40 tests run automatically on `make run` (no feature flags). Orchestrated sequentially in `src/main.rs`. Located in `tests/`:

| Test | Coverage |
|------|----------|
| `test_allocator` | Bump allocator page alloc/free |
| `test_heap` | Global heap (Box, Vec) |
| `test_dynamic_pagetable` | DynamicIdentityMapper 2MB mapping |
| `test_multi_vcpu` | Multi-vCPU creation, VMPIDR |
| `test_scheduler` | Round-robin scheduling, block/unblock |
| `test_vm_scheduler` | VM-integrated scheduling lifecycle |
| `test_gicv3_virt` | List Register injection, ELRSR |
| `test_guest` | Basic hypercall (HVC #0) |
| `test_timer` | Timer interrupt detection |
| `test_mmio` | MMIO device registration + routing |
| `test_complete_interrupt` | End-to-end IRQ injection flow |
| `test_guest_irq` | HCR_EL2.VI injection |
| `test_guest_interrupt` | Guest exception vector handling |
| `test_guest_loader` | GuestConfig for Zephyr/Linux |
| `test_simple_guest` | Simple guest boot + exit |

## Critical Implementation Details

### HPFAR_EL2 for MMIO (must-know)
When guest MMU is on, `FAR_EL2` = guest VA, NOT IPA. Use `HPFAR_EL2` for the IPA:
```
IPA = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8 | (far_el2 & 0xFFF)
```

### Never Modify Guest SPSR_EL2
Guest controls its own `PSTATE.I` (interrupt mask). Overriding causes spinlock deadlocks.

### CNTHP Timer Must Be Re-enabled
Guest can re-disable INTID 26 via GICR writes. `ensure_cnthp_enabled()` directly writes physical GICR (EL2 bypasses Stage-2) before every vCPU entry.

### ICC_SGI1R_EL1 Bit Fields
- TargetList: bits [15:0] (NOT [23:16])
- Aff1: bits [23:16] (NOT [27:24])
- INTID: bits [27:24] (NOT [3:0])

### Platform Constants
All board-specific addresses are centralized in `src/platform.rs`. `SMP_CPUS` is the single source of truth for vCPU count — must match QEMU `-smp` and DTB cpu nodes.
