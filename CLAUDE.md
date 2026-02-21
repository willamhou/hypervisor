# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ARM64 Type-1 bare-metal hypervisor written in Rust (no_std) with ARM64 assembly. Runs at EL2 (hypervisor exception level) and manages guest VMs at EL1. Targets QEMU virt machine. Boots Linux 6.12.12 to BusyBox shell with 4 vCPUs, virtio-blk storage, and virtio-net inter-VM networking. Supports multi-VM with per-VM Stage-2, VMID-tagged TLBs, two-level scheduling, and L2 virtual switch. Includes FF-A v1.1 proxy with stub SPMC, page ownership validation via Stage-2 PTE SW bits, FF-A v1.1 descriptor parsing, SMC forwarding to EL3, and VM-to-VM memory sharing (MEM_RETRIEVE/RELINQUISH with dynamic Stage-2 page mapping). Android boot with PL031 RTC emulation, Binder IPC, binderfs, minimal init, 1GB guest RAM. Dual boot modes: NS-EL2 hypervisor via `make run-tfa-linux` (BL33) and S-EL2 SPMC via `make run-spmc` (BL32). TF-A boot chain: BL1→BL2→BL31(SPMD)→BL32(SPMC)→BL33 with manifest FDT parsing. SPMC boots SP Hello at S-EL1 via ERET with Secure Stage-2, dispatches NWd→SP DIRECT_REQ/RESP messaging.

## Build Commands

```bash
make              # Build hypervisor
make run          # Build + run in QEMU — runs 33 test suites automatically (exit: Ctrl+A then X)
make run-linux    # Build + boot Linux guest (--features linux_guest, 4 vCPUs on 1 pCPU, virtio-blk)
make run-linux-smp # Build + boot Linux guest (--features multi_pcpu, 4 vCPUs on 4 pCPUs)
make run-multi-vm # Build + boot 2 Linux VMs time-sliced (--features multi_vm)
make run-android  # Build + boot Android-configured kernel (PL031 RTC, Binder, minimal init, 1GB RAM)
make run-guest GUEST_ELF=/path/to/zephyr.elf  # Boot Zephyr guest (--features guest)
make run-sel2     # Boot TF-A with trivial BL32 at S-EL2 (requires build-tfa first)
make run-tfa-linux # Boot TF-A → hypervisor (BL33) → Linux (requires build-tfa-bl33 first)
make run-spmc     # Boot TF-A → our SPMC (BL32) at S-EL2 (requires build-tfa-spmc first)
make build-qemu   # Build QEMU 9.2.3 from source (one-time, Docker)
make build-tfa    # Build TF-A flash.bin with SPD=spmd (Docker)
make build-tfa-bl33 # Build TF-A flash.bin with PRELOADED_BL33_BASE=0x40200000
make build-spmc   # Build hypervisor as S-EL2 SPMC binary (--features sel2)
make build-sp-hello # Build SP Hello binary (S-EL1 Secure Partition)
make build-tfa-spmc # Build TF-A with real SPMC as BL32 + SP Hello
make debug        # Build + run with GDB server on port 1234
make clean        # Clean build artifacts
make check        # Check code without building
make clippy       # Run linter
make fmt          # Format code
```

**Feature flags** (Cargo features, selected via Makefile targets):
- `(default)` — unit tests only, no guest boot
- `guest` — Zephyr guest loading
- `linux_guest` — Linux guest with DynamicIdentityMapper, GICR trap-and-emulate, virtio-blk, virtio-net
- `multi_pcpu` — Multi-pCPU support (implies `linux_guest`): 1:1 vCPU-to-pCPU affinity, PSCI boot, TPIDR_EL2 context, SpinLock devices
- `multi_vm` — Multi-VM support (implies `linux_guest`): 2 VMs time-sliced on 1 pCPU, per-VM Stage-2/VMID, per-VM DeviceManager
- `sel2` — S-EL2 SPMC mode: hypervisor as BL32 (SPMC role), separate boot_sel2.S entry, linker base 0x0e100000 (secure DRAM), manifest parsing, FFA_MSG_WAIT handshake

**Note**: `multi_pcpu` and `multi_vm` are mutually exclusive — both imply `linux_guest` but use different scheduling models. `sel2` is mutually exclusive with all others.

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
| `ExitReason` | `src/arch/aarch64/regs.rs` | VM exit causes: WfiWfe, HvcCall, SmcCall, DataAbort, etc. |
| `FfaProxy` | `src/ffa/proxy.rs` | FF-A v1.1 proxy: intercepts guest SMC, handles VERSION/ID_GET/FEATURES/RXTX/messaging/memory |
| `Stage2Walker` | `src/ffa/stage2_walker.rs` | Stage-2 page table walker from VTTBR_EL2: PTE SW bits, S2AP, map_page/unmap_page for cross-VM sharing |
| `FfaDescriptors` | `src/ffa/descriptors.rs` | FF-A v1.1 composite memory region descriptor parsing |
| `SmcForward` | `src/ffa/smc_forward.rs` | SMC forwarding to EL3 + SPMC probe |
| `PlatformInfo` | `src/dtb.rs` | Runtime DTB parsing: UART, GIC, RAM, CPU count discovery |
| `VSwitch` | `src/vswitch.rs` | L2 virtual switch with MAC learning, inter-VM frame forwarding |
| `NetRxRing` | `src/vswitch.rs` | Per-port SPSC ring buffer for async RX frame delivery |
| `VirtualPl031` | `src/devices/pl031.rs` | PL031 RTC emulation: counter-based time, PrimeCell ID |
| `SpMcManifest` | `src/manifest.rs` | SPMC manifest parser: TOS_FW_CONFIG DTB (spmc_id, version) |
| `SpmcHandler` | `src/spmc_handler.rs` | S-EL2 SPMC event loop + FF-A dispatch, SP DIRECT_REQ routing via `dispatch_to_sp()` + `enter_guest()` ERET |
| `SpContext` | `src/sp_context.rs` | Per-SP state machine (Reset→Idle→Running→Blocked), wraps VcpuContext, global SpStore |
| `SecureStage2Config` | `src/secure_stage2.rs` | VSTTBR_EL2/VSTCR_EL2 config for SP isolation, `build_sp_stage2()` identity-maps SP code + UART |

### Exception Handling Flow
```
Guest @ EL1
  ↓ trap (Data Abort, HVC, SMC, WFI, MSR/MRS)
Exception Vector (arch/aarch64/exception.S) — save context
  ↓
handle_exception() (src/arch/aarch64/hypervisor/exception.rs)
  ├─ WFI → return false (exit to scheduler)
  ├─ HVC → handle_psci() (CPU_ON, CPU_OFF, SYSTEM_RESET)
  ├─ SMC → handle_smc() → PSCI or FF-A proxy or forward to EL3
  ├─ Data Abort → HPFAR_EL2 for IPA → decode instruction → MMIO dispatch
  ├─ MSR/MRS trap → handle ICC_SGI1R_EL1 (SGI emulation), sysreg emulation
  └─ IRQ → handle INTID 26 (preemption), 27 (vtimer), 33 (UART RX)
  ↓ advance PC, restore context
ERET back to guest
```

### SMP / Multi-vCPU

`run_smp()` calls `run_one_iteration()` in a loop. Each iteration runs one vCPU on a single physical CPU via cooperative + preemptive scheduling:

1. Check per-VM `pending_cpu_on` → `boot_secondary_vcpu()` (PSCI CPU_ON)
2. Wake vCPUs with pending SGIs/SPIs → `scheduler.unblock()`
3. Pick next vCPU (round-robin) → set `current_vcpu_id`
4. Drain UART RX ring → inject SPI 33
5. Inject pending SGIs/SPIs into `arch_state.ich_lr[]`
6. Arm CNTHP preemption timer (10ms, INTID 26) — only when 2+ vCPUs online
7. `vcpu.run()` → save/restore arch state → `enter_guest()` → ERET
8. Handle exit: terminal→remove, CPU_ON/preemption→yield, WFI→block, other→yield

**Important**: `vcpu_online_mask` must include vCPU 0 at boot — without it, preemption timer never activates.

**SGI/IPI emulation**: ICC_SGI1R_EL1 trapped via ICH_HCR_EL2.TALL1=1 → decoded (TargetList[15:0], Aff1[23:16], INTID[27:24]) → `PENDING_SGIS[vcpu_id]` atomics → injected before next entry.

### Multi-pCPU (4 vCPUs on 4 Physical CPUs)

Feature: `multi_pcpu` (implies `linux_guest`). Target: `make run-linux-smp`.

**Architecture**: 1:1 vCPU-to-pCPU affinity. Each physical CPU runs one vCPU exclusively — no scheduler needed.

**Secondary pCPU Boot**: QEMU virt keeps secondary CPUs powered off. `wake_secondary_pcpus()` issues real PSCI CPU_ON SMC calls (`smc #0`, function_id=0xC4000003) to QEMU's EL3 firmware with `secondary_entry` as the entry point.

**Per-CPU Context Pointer**: `TPIDR_EL2` (hardware-banked per physical CPU) replaces the global `current_vcpu_context` variable in `exception.S`. Set by `enter_guest()`, read by exception/IRQ handlers.

**Physical GICR Programming**: `ensure_vtimer_enabled(cpu_id)` programs physical GICR ISENABLER0 for SGIs 0-15 + PPI 27 (vtimer) before every guest entry. Guest GICR writes only update the shadow `VirtualGicr` state.

**Cross-pCPU SPI Delivery**: `inject_spi()` reads physical GICD_IROUTER directly (EL2 bypasses Stage-2) to avoid deadlock with the `DEVICES` SpinLock. If the target is a remote pCPU, sends physical SGI 0 via `msr icc_sgi1r_el1` to wake it.

**WFI Passthrough**: TWI cleared in multi-pCPU mode — real WFI on physical CPU, woken by physical interrupts.

### Multi-VM (2 Linux VMs Time-Sliced)

Feature: `multi_vm` (implies `linux_guest`). Target: `make run-multi-vm`.

**Architecture**: 2 VMs round-robin time-sliced on a single pCPU. Each VM has 4 vCPUs scheduled via the inner `run_one_iteration()` loop.

**Per-VM Global State**: `VmGlobalState` struct (indexed by `CURRENT_VM_ID`) replaces flat globals. Each VM has its own `pending_sgis`, `pending_spis`, `vcpu_online_mask`, `current_vcpu_id`, and `preemption_exit`.

**Per-VM DeviceManager**: `DEVICES: [GlobalDeviceManager; MAX_VMS]` array. Exception handler uses `CURRENT_VM_ID` to dispatch MMIO to the correct VM's devices.

**VMID-Tagged Stage-2**: `Stage2Config::new_with_vmid()` encodes VMID in VTTBR_EL2 bits [63:48] for TLB isolation. `Vm::activate_stage2()` writes VTTBR_EL2/VTCR_EL2 before guest entry.

**Two-Level Scheduler**: `run_multi_vm()` → outer VM round-robin → `CURRENT_VM_ID.store()` → `activate_stage2()` → `run_one_iteration()` → inner vCPU round-robin.

**Memory Partitioning**: VM 0 at 0x48000000 (256MB), VM 1 at 0x68000000 (256MB). Each VM gets separate kernel, DTB, initramfs, and virtio-blk disk image loaded by QEMU.

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

### Virtio-net + VSwitch

```
VirtioMmioTransport<VirtioNet>  @ 0x0a000200 (SPI 17 = INTID 49)
  ├─ MMIO registers (virtio-mmio spec)
  ├─ 2 virtqueues: RX (queue 0) + TX (queue 1)
  └─ VirtioNet backend (device_id=1, MAC 52:54:00:00:00:{vm_id+1})
```

**TX path**: Guest writes QueueNotify → `process_tx()` → strip 12-byte `virtio_net_hdr_v1` → `vswitch_forward(src_port, frame)` → VSwitch MAC learning + L2 forwarding → `PORT_RX[dst].store(frame)`.

**RX path**: `drain_net_rx(vm_id)` in run loop → `PORT_RX[vm_id].take()` → `inject_net_rx()` → `inject_rx(frame)` → write 12-byte header (num_buffers=1) + frame into RX descriptor chain via `copy_nonoverlapping` → `inject_spi(49)`.

**VSwitch** (`src/vswitch.rs`): L2 virtual switch with 16-entry MAC learning table. Broadcasts/multicasts flood all ports (excluding source). Unknown unicasts also flood. MAC entries are learned on TX (source MAC → source port).

**NetRxRing**: SPSC ring buffer (9 slots, 8 usable + 1 sentinel) per VM port. Atomic head/tail with Acquire/Release ordering. Stores up to 1514-byte Ethernet frames.

**MMIO slot abstraction**: `platform::virtio_slot(n)` returns `(base_addr, intid)` for slot n. Slot 0 = virtio-blk, slot 1 = virtio-net. Stride = 0x200.

**Auto-IP**: Initramfs `/init` reads MAC from sysfs, extracts last octet, assigns `10.0.0.{octet}/24` via `ifconfig`. VM 0 → `10.0.0.1`, VM 1 → `10.0.0.2`.

### FF-A v1.1 Proxy (`src/ffa/`)

Implements the FF-A (Firmware Framework for Arm) v1.1 hypervisor proxy role (pKVM-compatible). Guest SMC calls trapped via `HCR_EL2.TSC=1` (bit 19) are routed through `handle_smc()` → `ffa::proxy::handle_ffa_call()`.

**Supported calls**: FFA_VERSION, FFA_ID_GET, FFA_SPM_ID_GET, FFA_FEATURES, FFA_RXTX_MAP/UNMAP, FFA_RX_RELEASE, FFA_PARTITION_INFO_GET, FFA_MSG_SEND_DIRECT_REQ, FFA_MSG_SEND2, FFA_MSG_WAIT, FFA_RUN, FFA_MEM_SHARE/LEND/RETRIEVE_REQ/RELINQUISH/RECLAIM, FFA_NOTIFICATION_BITMAP_CREATE/DESTROY/BIND/UNBIND/SET/GET/INFO_GET. FFA_MEM_DONATE is blocked (returns NOT_SUPPORTED). VM-to-VM memory sharing: sender shares pages via MEM_SHARE, receiver maps them via MEM_RETRIEVE_REQ (dynamic Stage-2 page mapping), receiver unmaps via MEM_RELINQUISH, sender reclaims via MEM_RECLAIM.

**Stub SPMC** (`src/ffa/stub_spmc.rs`): Simulates 2 Secure Partitions (SP1=0x8001, SP2=0x8002) for testing without a real Secure World. Direct messaging echoes x4-x7 back. Memory sharing tracks multi-range records with `MemShareRecord` (up to 4 ranges per share, `ShareInfo`/`ShareInfoFull` for reclaim/retrieve). `mark_retrieved()`/`mark_relinquished()` track retrieve state; `MEM_RECLAIM` blocked while retrieved.

**RXTX Mailbox** (`src/ffa/mailbox.rs`): Per-VM TX/RX buffer IPAs registered via FFA_RXTX_MAP. Used by PARTITION_INFO_GET to return SP descriptors. TX buffer used for FF-A v1.1 composite memory region descriptors.

**Page Ownership** (`src/ffa/memory.rs`): Stage-2 PTE software bits [56:55] track page state: Owned(0b00), SharedOwned(0b01), SharedBorrowed(0b10), Donated(0b11). Validated during MEM_SHARE/LEND (Owned required), transitioned to SharedOwned, restored on MEM_RECLAIM. S2AP bits [7:6] restrict access: SHARE→RO, LEND→NONE. Matches pKVM page ownership model.

**Stage-2 Walker** (`src/ffa/stage2_walker.rs`): Lightweight page table walker reconstructed from `VTTBR_EL2` at SMC handling time. Reads/writes PTE SW bits and S2AP without owning page table memory. Used by MEM_SHARE/LEND/RECLAIM for ownership validation. `map_page()` creates 4KB page entries in a target VM's Stage-2 (allocates L2/L3 tables from heap), used by MEM_RETRIEVE_REQ for cross-VM sharing. `unmap_page()` zeroes L3 PTEs, used by MEM_RELINQUISH. `PER_VM_VTTBR` global stores each VM's L0 table PA for constructing walkers for non-active VMs. Gated by `#[cfg(feature = "linux_guest")]` — unit tests skip Stage-2 validation (stale VTTBR from earlier page table tests).

**Descriptor Parsing** (`src/ffa/descriptors.rs`): Parses FF-A v1.1 composite memory region descriptors (DEN0077A Table 5.19-5.25): `FfaMemRegion`(48B) → `FfaMemAccessDesc`(16B) → `FfaCompositeMemRegion`(16B) → `FfaMemRegionAddrRange`(16B). Uses `core::ptr::read_unaligned` for packed struct safety. Falls back to register-based protocol (x3=IPA, x4=count, x5=receiver) when no mailbox is mapped.

**SMC Forwarding** (`src/ffa/smc_forward.rs`): `forward_smc()` uses inline `smc #0` to forward calls to EL3 (HCR_EL2.TSC only traps EL1 SMC). `probe_spmc()` sends FFA_VERSION to detect a real SPMC at EL3. `ffa::proxy::init()` called at boot (linux_guest only) to set `SPMC_PRESENT` flag. Unknown SMCs in `handle_smc()` catch-all are forwarded to EL3 instead of returning -1.

**SMC routing**: `is_ffa_function(fid)` checks for SMC32/64 function IDs in the 0x84/0xC4 range with low byte >= 0x60. PSCI functions (0x84000000-0x8400001F, 0xC4000000-0xC4000003) are handled separately.

### UART (PL011) Emulation

Full trap-and-emulate (Stage-2 unmapped). TX: guest writes UARTDR → `output_char()` to physical UART. RX: physical IRQ (INTID 33) → `UART_RX` ring buffer → `VirtualUart.push_rx()` → inject SPI 33. Linux amba-pl011 probe requires PeriphID/PrimeCellID registers.

### PL031 RTC Emulation (`src/devices/pl031.rs`)

Trap-and-emulate at `0x09010000` (SPI 2 = INTID 34). Counter-based time: `RTCDR = load_value + (CNTVCT_EL0 / CNTFRQ_EL0)` when enabled (RTCCR bit 0). Registers: RTCDR (0x000, read), RTCLR (0x008, write), RTCCR (0x00C, control), RTCIMSC/RTCRIS/RTCMIS/RTCICR (0x010-0x01C, stubs). PrimeCell ID registers (0xFE0-0xFFC) required for Linux amba bus probe. 4 unit tests in `tests/test_pl031.rs`.

### DTB Runtime Parsing (`src/dtb.rs`)

At boot, QEMU passes the host DTB address in x0. `boot.S` preserves it in callee-saved x20, then passes to `rust_main(dtb_addr: usize)`. `dtb::init()` uses the `fdt` crate (v0.1.5, zero-copy, no-alloc) to discover platform hardware:

- **UART**: `arm,pl011` compatible → `uart_base`
- **GIC**: `arm,gic-v3` compatible → `gicd_base`, `gicr_base`, `gicr_size`
- **RAM**: `/memory` node → `ram_base`, `ram_size`
- **CPUs**: `cpus` node → `num_cpus`

Helpers: `gicr_rd_base(cpu_id) = gicr_base + cpu_id * 0x20000`, `gicr_sgi_base(cpu_id) = gicr_rd_base + 0x10000`.

Falls back to QEMU virt defaults if DTB parse fails (e.g., QEMU passes addr=0 with `-kernel`). `platform::num_cpus()` reads DTB at runtime; `MAX_SMP_CPUS = 8` is the compile-time array capacity.

**Pre-DTB code** (`uart_puts` in `lib.rs`, GICD/GICC statics in `gic.rs`) still uses hardcoded `platform::UART_BASE`/`GICD_BASE` because they run before DTB init or require `const` for Rust `static`.

### Memory Layout

| Region | Address | Purpose |
|--------|---------|---------|
| SPMC code (sel2) | 0x0e100000 | S-EL2 linker base (secure DRAM, BL32) |
| Hypervisor code (NS) | 0x40200000 | NS-EL2 linker base (avoids QEMU DTB at 0x40000000 in -bios mode) |
| Heap | 0x41000000 (16MB) | Page table allocation, `BumpAllocator` |
| DTB (VM 0) | 0x47000000 | Device tree blob |
| Kernel (VM 0) | 0x48000000 | Linux Image load address |
| Initramfs (VM 0) | 0x54000000 | BusyBox initramfs |
| Disk image (VM 0) | 0x58000000 | virtio-blk backing store |
| VM 0 RAM | 0x48000000-0x58000000 | 256MB (single-VM: 0x48000000-0x88000000 = 1GB) |
| DTB (VM 1) | 0x67000000 | Device tree blob (multi_vm only) |
| Kernel (VM 1) | 0x68000000 | Linux Image load address (multi_vm only) |
| VM 1 RAM | 0x68000000-0x78000000 | 256MB (multi_vm only) |
| Disk image (VM 1) | 0x78000000 | virtio-blk backing store (multi_vm only) |

**Stage-2 mappers**:
- `IdentityMapper` (static, 2MB-only) — used by unit tests (`make run`)
- `DynamicIdentityMapper` (heap-allocated, 2MB+4KB) — used by Linux guest (`make run-linux`), supports `unmap_4kb_page()` for GICR trap setup

**Heap gap**: Heap lies within guest's PA range but is left unmapped in Stage-2 to prevent guest corruption of page tables. Guest kernel never accesses this range (declared memory starts at 0x48000000).

### Global State (`src/global.rs`)

| Global | Type | Purpose |
|--------|------|---------|
| `DEVICES` | `[GlobalDeviceManager; MAX_VMS]` | Per-VM MMIO dispatch (UnsafeCell single-pCPU / SpinLock multi-pCPU) |
| `VM_STATE` | `[VmGlobalState; MAX_VMS]` | Per-VM state (see below) |
| `CURRENT_VM_ID` | `AtomicUsize` | Which VM is currently active |
| `PENDING_CPU_ON_PER_VCPU` | `[PerVcpuCpuOnRequest; 8]` | Per-vCPU PSCI CPU_ON (multi-pCPU mode only) |
| `SHARED_VTTBR` / `SHARED_VTCR` | `AtomicU64` | Stage-2 config shared from primary to secondaries (multi-pCPU) |
| `PER_VM_VTTBR` | `[AtomicU64; MAX_VMS]` | Per-VM L0 table PA for cross-VM Stage-2 access (FF-A RETRIEVE) |
| `UART_RX` | `UartRxRing` | Lock-free ring buffer, IRQ handler → run loop |
| `PORT_RX` | `[NetRxRing; MAX_PORTS]` | Per-VM SPSC ring for virtio-net RX frames |
| `VSWITCH` | `UnsafeCell<VSwitch>` | L2 virtual switch with MAC learning table |

`VmGlobalState` contains per-VM: `pending_sgis[MAX_VCPUS]`, `pending_spis[MAX_VCPUS]`, `terminal_exit[MAX_VCPUS]`, `vcpu_online_mask`, `current_vcpu_id`, `pending_cpu_on`, `preemption_exit`. Accessed via `vm_state(vm_id)` or `current_vm_state()`.

### Device Manager Pattern

Enum-dispatch (no dynamic dispatch / trait objects):
```rust
pub enum Device {
    Uart(pl011::VirtualUart),
    Gicd(gic::VirtualGicd),
    Gicr(gic::VirtualGicr),
    VirtioBlk(virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>),
    VirtioNet(virtio::mmio::VirtioMmioTransport<virtio::net::VirtioNet>),
    Pl031(pl031::VirtualPl031),
}
```
Array-based routing: `devices: [Option<Device>; 8]`, scan for `dev.contains(addr)`.

## Build System

- **build.rs**: Cross-compiles boot assembly and `exception.S` via `aarch64-linux-gnu-gcc`, archives into `libboot.a`, links with `--whole-archive`. Feature-gated: `sel2` selects `boot_sel2.S` + `linker_sel2.ld`, otherwise `boot.S` + `linker.ld`
- **Target**: `aarch64-unknown-none.json` (custom spec: `llvm-target: aarch64-unknown-none`, `panic-strategy: abort`, `disable-redzone: true`)
- **Linker**: `arch/aarch64/linker.ld` — base at 0x40200000 (NS-EL2, avoids QEMU DTB at 0x40000000 in `-bios` mode); `arch/aarch64/linker_sel2.ld` — base at 0x0e100000 (S-EL2, secure DRAM)

## Tests

~248 assertions across 33 test suites run automatically on `make run` (no feature flags). Orchestrated sequentially in `src/main.rs`. Located in `tests/`:

| Test | Coverage | Assertions |
|------|----------|------------|
| `test_dtb` | DTB parsing, PlatformInfo defaults, GICR helpers | 8 |
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
| `test_guest_irq` | Per-VM PENDING_SGIS/PENDING_SPIS bitmask operations | 5 |
| `test_device_routing` | DeviceManager registration, routing, accessors | 6 |
| `test_vm_state_isolation` | Per-VM SGI/SPI/online_mask/vcpu_id independence | 4 |
| `test_vmid_vttbr` | VMID 0/1 encoding in VTTBR_EL2 bits [63:48] | 2 |
| `test_multi_vm_devices` | DEVICES[0]/DEVICES[1] registration + MMIO isolation | 3 |
| `test_vm_activate` | Vm initial VTTBR/VTCR state | 2 |
| `test_net_rx_ring` | NetRxRing SPSC: empty/store/take/fill/overflow/wraparound | 8 |
| `test_vswitch` | VSwitch: flood/MAC learning/broadcast/no-self/capacity | 6 |
| `test_virtio_net` | VirtioNet: device_id/features/queues/config/mac_for_vm | 8 |
| `test_page_ownership` | Stage-2 PTE SW bits: read/write OWNED/SHARED_OWNED, unmapped IPA, 2MB block→4KB split | 9 |
| `test_pl031` | PL031 RTC: RTCDR readable, RTCLR write+readback, PeriphID/PrimeCellID, unknown offset | 4 |
| `test_ffa` | FF-A proxy: VERSION/ID_GET/FEATURES/RXTX/messaging/MEM_SHARE/RECLAIM/descriptors/SMC forward/VM-to-VM RETRIEVE/RELINQUISH/SPM_ID_GET/RUN/notifications/MSG_SEND2/MSG_WAIT | 44 |
| `test_spmc_handler` | SPMC dispatch: VERSION/ID_GET/SPM_ID_GET/FEATURES/PARTITION_INFO/DIRECT_REQ echo/framework msg | 24 |
| `test_sp_context` | SpContext: state machine transitions, VcpuContext fields, set/get args (x0-x7) | 16 |
| `test_secure_stage2` | SecureStage2Config: VSTTBR address, VSTCR T0SZ, new_from_vsttbr | 4 |
| `test_guest_interrupt` | Guest interrupt injection + exception vector (blocks) | 1 |

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

### HCR_EL2.TSC for SMC Trapping
`HCR_TSC = 1 << 19` traps guest SMC instructions to EL2 as `EC_SMC64 (0x17)`. Unlike HVC traps, the trapped SMC sets `ELR_EL2` to the SMC instruction itself — exception handler must advance PC by 4. This enables the FF-A proxy to intercept guest FF-A SMC calls and route them through `handle_smc()`.

### CPTR_EL3.TFP Traps FP/SIMD at S-EL2 (must-know for TF-A builds)
TF-A's default `CPTR_EL3.TFP=1` traps ALL FP/SIMD instructions from S-EL2 to EL3. Rust debug-mode `read_volatile` uses NEON SIMD internally (`cnt v0.8b, v0.8b` for popcount alignment check in `is_aligned_to`), causing silent hangs on any memory read. Fix: `CTX_INCLUDE_FPREGS=1` in TF-A build (clears CPTR_EL3.TFP). Requires `ENABLE_SVE_FOR_NS=0` and `ENABLE_SME_FOR_NS=0` to avoid build conflicts.

### S-EL2 SPMC Boot (`sel2` feature)
Entry point: `boot_sel2.S` → `rust_main_sel2(manifest_addr, hw_config_addr, core_id)`. SPMD passes x0=TOS_FW_CONFIG (manifest DTB at 0x0e002000), x1=HW_CONFIG, x4=core_id. Init: exception vectors → manifest parse → GIC init → Secure Stage-2 → parse SPKG header (img_offset=0x4000) → clear SCTLR_EL1/VBAR_EL1 → ERET to SP1 → SP calls FFA_MSG_WAIT → SPMC event loop. `src/manifest.rs` parses `/attribute` node (spmc_id, maj_ver, min_ver) per FF-A Core Manifest v1.0 (DEN0077A).

### SP Package Format (SPKG)
BL2 loads raw SP packages to `load-address` from `tb_fw_config.dts`. SPKG header (24 bytes LE): magic("SPKG"), version, pm_offset(0x1000), pm_size, img_offset(0x4000), img_size. SPMC must parse header and enter SP at `load_addr + img_offset`. The sp_manifest.dts UUID gets byte-swapped by `sp_mk_generator.py` (LE conversion); `tb_fw_config.dts` UUID must match the swapped form. Use `fiptool info fip.bin` to verify.

### Diagnostic Fault Handler (`exception.S`)
`fault_diag_print` handles exceptions when TPIDR_EL2=0 (no vCPU context — host-level fault). Prints ESR_EL2, ELR_EL2, FAR_EL2, HPFAR_EL2 to UART. Used during S-EL2 boot to diagnose Data Aborts. Located at end of `exception.S` (outside vector table alignment constraints).

### Platform Constants
Guest-specific addresses (heap, kernel load, virtio disk) are in `src/platform.rs`. Host hardware addresses (UART, GIC, RAM, CPU count) are discovered at runtime from DTB via `src/dtb.rs` — use `platform::num_cpus()` and `dtb::platform_info()` instead of hardcoded constants. `MAX_SMP_CPUS = 8` is the compile-time array capacity; `SMP_CPUS = 4` is the fallback default.

## Roadmap: NS-EL2 → S-EL2 SPMC → pKVM Integration

**Target architecture** (end state):
```
EL3:    TF-A BL31 + SPMD (SMC relay, world switch)
S-EL2:  Our hypervisor (SPMC role, BL32) → manages Secure Partitions
S-EL1:  Secure Partitions (bare-metal SPs)
NS-EL2: pKVM (Linux KVM protected mode) → manages Normal World VMs
NS-EL1: Linux/Android guest
```

**Phase 3** (done): NS-EL2 complete — 2MB block split, FF-A notifications, indirect messaging
**Phase 4** (in progress): QEMU `secure=on` + TF-A boot chain → Sprint 4.1-4.4 done (SPMC + SP Hello + 6/6 BL33 tests)
**Phase 4.5**: pKVM at NS-EL2 + our SPMC at S-EL2 → end-to-end FF-A path
**Phase 5**: RME & CCA (Realm Manager)

See `DEVELOPMENT_PLAN.md` for full details.
