# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ARM64 Type-1 bare-metal hypervisor written in Rust (no_std) with ARM64 assembly. Runs at EL2 (hypervisor exception level) and manages guest VMs at EL1. Targets QEMU virt machine. Boots Linux 6.12.12 to BusyBox shell with 4 vCPUs and virtio-blk storage.

## Build Commands

```bash
make              # Build hypervisor
make run          # Build + run in QEMU — runs 43 unit tests automatically (exit: Ctrl+A then X)
make run-linux    # Build + boot Linux guest (--features linux_guest, 4 vCPUs on 1 pCPU, virtio-blk)
make run-linux-smp # Build + boot Linux guest (--features multi_pcpu, 4 vCPUs on 4 pCPUs)
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
- `multi_pcpu` — Multi-pCPU support (implies `linux_guest`): 1:1 vCPU-to-pCPU affinity, PSCI boot, TPIDR_EL2 context, SpinLock devices

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

### Multi-pCPU (4 vCPUs on 4 Physical CPUs)

Feature: `multi_pcpu` (implies `linux_guest`). Target: `make run-linux-smp`.

**Architecture**: 1:1 vCPU-to-pCPU affinity. Each physical CPU runs one vCPU exclusively — no scheduler needed.

**Secondary pCPU Boot**: QEMU virt keeps secondary CPUs powered off. `wake_secondary_pcpus()` issues real PSCI CPU_ON SMC calls (`smc #0`, function_id=0xC4000003) to QEMU's EL3 firmware with `secondary_entry` as the entry point.

**Per-CPU Context Pointer**: `TPIDR_EL2` (hardware-banked per physical CPU) replaces the global `current_vcpu_context` variable in `exception.S`. Set by `enter_guest()`, read by exception/IRQ handlers.

**Physical GICR Programming**: `ensure_vtimer_enabled(cpu_id)` programs physical GICR ISENABLER0 for SGIs 0-15 + PPI 27 (vtimer) before every guest entry. Guest GICR writes only update the shadow `VirtualGicr` state.

**Cross-pCPU SPI Delivery**: `inject_spi()` reads physical GICD_IROUTER directly (EL2 bypasses Stage-2) to avoid deadlock with the `DEVICES` SpinLock. If the target is a remote pCPU, sends physical SGI 0 via `msr icc_sgi1r_el1` to wake it.

**WFI Passthrough**: TWI cleared in multi-pCPU mode — real WFI on physical CPU, woken by physical interrupts.

### GIC Emulation

| Component | Address | Mode | Implementation |
|-----------|---------|------|----------------|
| GICD | 0x08000000 | Trap + write-through | `VirtualGicd` shadow state + write-through to physical GICD |
| GICR 0-3 | 0x080A0000+ | Trap-and-emulate | `VirtualGicr` (Stage-2 unmapped, 4KB pages) |
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
| `DEVICES` | `GlobalDeviceManager` | Exception handler MMIO dispatch (UnsafeCell single-pCPU / SpinLock multi-pCPU) |
| `PENDING_CPU_ON` | `PendingCpuOn` (atomics) | PSCI CPU_ON signaling (single-pCPU mode) |
| `PENDING_CPU_ON_PER_VCPU` | `[PerVcpuCpuOnRequest; 8]` | Per-vCPU PSCI CPU_ON (multi-pCPU mode) |
| `SHARED_VTTBR` / `SHARED_VTCR` | `AtomicU64` | Stage-2 config shared from primary to secondaries (multi-pCPU) |
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

~85 assertions across 19 test suites run automatically on `make run` (no feature flags). Orchestrated sequentially in `src/main.rs`. Located in `tests/`:

| Test | Coverage | Assertions |
|------|----------|------------|
| `test_allocator` | Bump allocator page alloc/free | 4 |
| `test_heap` | Global heap (Box, Vec) | 4 |
| `test_dynamic_pagetable` | DynamicIdentityMapper 2MB mapping + 4KB unmap | 6 |
| `test_multi_vcpu` | Multi-vCPU creation, VMPIDR | 4 |
| `test_scheduler` | Round-robin scheduling, block/unblock | 4 |
| `test_vm_scheduler` | VM-integrated scheduling lifecycle | 5 |
| `test_mmio` | MMIO device registration + guest UART access | 1 |
| `test_gicv3_virt` | List Register injection, ELRSR | 6 |
| `test_complete_interrupt` | End-to-end IRQ injection flow | 1 |
| `test_guest` | Basic hypercall (HVC #0) | 1 |
| `test_guest_loader` | GuestConfig for Zephyr/Linux | 3 |
| `test_simple_guest` | Simple guest boot + exit | 1 |
| `test_decode` | MmioAccess::decode() ISS + instruction paths | 9 |
| `test_gicd` | VirtualGicd shadow state (CTLR, ISENABLER, IROUTER) | 8 |
| `test_gicr` | VirtualGicr per-vCPU state (TYPER, WAKER, ISENABLER0) | 8 |
| `test_global` | PendingCpuOn atomics + UartRxRing SPSC buffer | 6 |
| `test_guest_irq` | PENDING_SGIS/PENDING_SPIS bitmask operations | 5 |
| `test_guest_interrupt` | Guest interrupt injection + exception vector | 1 |
| `test_device_routing` | DeviceManager registration, routing, accessors | 6 |

Not wired into `main.rs` (exported but not called):
- `test_timer` — timer interrupt detection (requires manual timer setup)

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

### inject_spi() Must Not Acquire DEVICES Lock (multi-pCPU)
`inject_spi()` is called from `signal_interrupt()` inside the `DEVICES` SpinLock. Reading `DEVICES.route_spi()` would deadlock (non-reentrant). Instead, multi-pCPU mode reads physical GICD_IROUTER directly (EL2 bypasses Stage-2).

### QEMU virt Secondary CPUs Are Powered Off
Secondary physical CPUs start powered off — they do NOT execute `_start`. Must use real PSCI CPU_ON SMC (`smc #0`, function_id=0xC4000003) to QEMU's EL3 firmware.

### TPIDR_EL2 for Per-CPU Context (multi-pCPU)
`exception.S` uses `mrs x0, tpidr_el2` instead of a global variable. Each physical CPU has its own hardware-banked TPIDR_EL2. Set by `enter_guest()` via `msr tpidr_el2, x0`.

### Physical GICR Must Be Programmed for SGIs/PPIs
Guest GICR writes only update `VirtualGicr` shadow state. `ensure_vtimer_enabled()` programs physical GICR ISENABLER0 for SGIs 0-15 + PPI 27 before every guest entry.

### Platform Constants
All board-specific addresses are centralized in `src/platform.rs`. `SMP_CPUS` is the single source of truth for vCPU count — must match QEMU `-smp` and DTB cpu nodes.
