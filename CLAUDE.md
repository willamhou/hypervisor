# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ARM64 Type-1 bare-metal hypervisor in Rust (`no_std`) + ARM64 assembly. One codebase, two compile-time personalities:

- **NS-EL2 hypervisor** — boots Linux 6.12.12 to BusyBox (4 vCPUs, virtio-blk/net, multi-VM with per-VM Stage-2 + VMID TLBs + L2 vswitch), Android (PL031 RTC, Binder, 1GB RAM), and a FF-A v1.1 proxy (pKVM-compatible).
- **S-EL2 SPMC** (`sel2`) — runs as TF-A BL32, manages Secure Partitions at S-EL1 (SP1 Hello, SP2 IRQ, SP3 Relay), full FF-A v1.1: DIRECT_REQ (incl. SP↔SP with cycle detection), memory sharing (SHARE/LEND/DONATE/RETRIEVE/RELINQUISH/RECLAIM, NWd↔SP and SP↔SP), secure vIRQ injection, NS interrupt preemption.

End state target: TF-A BL31+SPMD @ EL3 → our SPMC @ S-EL2 → SPs @ S-EL1, alongside pKVM @ NS-EL2 → Linux/Android @ NS-EL1. Validated E2E against real pKVM (`ffa_test.ko`: 35/35 PASS). See Roadmap and `DEVELOPMENT_PLAN.md`.

## Build Commands

```bash
make              # Build hypervisor
make run          # Build + run in QEMU — runs 34 test suites automatically (exit: Ctrl+A then X)
make run-linux    # Boot Linux guest (--features linux_guest, 4 vCPUs on 1 pCPU, virtio-blk)
make run-linux-smp # Boot Linux guest (--features multi_pcpu, 4 vCPUs on 4 pCPUs)
make run-multi-vm # Boot 2 Linux VMs time-sliced (--features multi_vm)
make run-android  # Boot Android-configured kernel (PL031 RTC, Binder, minimal init, 1GB RAM)
make run-guest GUEST_ELF=/path/to/zephyr.elf  # Boot Zephyr guest (--features guest)
make debug        # Build + run with GDB server on port 1234
make check / clippy / fmt / clean

# Secure-world / TF-A chain (all Docker-based, TCG-only — KVM cannot virtualize EL3/Secure)
make build-qemu        # Build QEMU 9.2.3 from source (one-time)
make build-tfa-bl33    # TF-A flash.bin, PRELOADED_BL33_BASE=0x40200000
make run-tfa-linux     # TF-A → hypervisor (BL33) → Linux (needs build-tfa-bl33)
make build-tfa-spmc    # TF-A + real SPMC (BL32) + SP Hello/IRQ/Relay
make run-spmc          # TF-A → our SPMC (BL32) at S-EL2
make build-tfa-full    # TF-A real SPMC (BL32) + preloaded BL33 hypervisor
make run-tfa-linux-ffa # TF-A → SPMC → hypervisor (BL33) → Linux (FF-A discovery)
make build-spmc / build-sp-hello / build-sp-irq / build-sp-relay  # individual binaries

# pKVM integration (AOSP kernel as BL33)
make build-pkvm-kernel # AOSP android16-6.12 kernel for pKVM (Docker, ~15-30min first time)
make build-tfa-pkvm    # flash-pkvm.bin (ARM_LINUX_KERNEL_AS_BL33)
make run-pkvm          # pKVM (NS-EL2) + our SPMC (S-EL2), AOSP kernel as BL33
make run-pkvm-ffa-test # pKVM with FF-A test module (35/35 PASS)

# AVF / crosvm (needs ARM64 host + /dev/kvm for full validation)
make build-crosvm / build-crosvm-initramfs / run-crosvm
```

**Feature flags** (Cargo, selected by Makefile targets):
- `(default)` — unit tests only, no guest boot
- `guest` — Zephyr loading
- `linux_guest` — Linux guest: DynamicIdentityMapper, GICR trap-and-emulate, virtio-blk/net
- `multi_pcpu` (⊃ linux_guest) — 1:1 vCPU↔pCPU affinity, PSCI boot, TPIDR_EL2 context, SpinLock devices
- `multi_vm` (⊃ linux_guest) — 2 VMs time-sliced on 1 pCPU, per-VM Stage-2/VMID/DeviceManager
- `sel2` — S-EL2 SPMC mode: BL32, `boot_sel2.S`, linker base 0x0e100000, manifest parse, secondary warm-boot, boots SP1+SP2+SP3
- `tfa_boot` (⊃ linux_guest) — SPMC_PRESENT=true, NS proxy forwards DIRECT_REQ/PARTITION_INFO/MEM_SHARE/LEND/RECLAIM to real SPMC via SPMD

**Mutual exclusivity**: `multi_pcpu` ⊥ `multi_vm`; `sel2` ⊥ all others. `tfa_boot` pairs with `run-tfa-linux`.

**Toolchain**: Rust nightly, `aarch64-linux-gnu-{gcc,ar,objcopy}`, `qemu-system-aarch64`.

## Architecture

### Privilege Model
- **EL2** (or **S-EL2** in `sel2`): hypervisor — exceptions, Stage-2 tables, GIC virtual interface
- **EL1**: guest (Linux/Zephyr) or Secure Partition
- **Stage-2**: identity map (GPA==HPA), 2MB blocks + 4KB pages

### Core Abstractions

| Type | File | Role |
|------|------|------|
| `Vm` | `src/vm.rs` | VM lifecycle, Stage-2 setup, `run_smp()` scheduler loop |
| `Vcpu` | `src/vcpu.rs` | State machine, context save/restore |
| `VcpuContext` / `VcpuArchState` | `src/arch/aarch64/regs.rs`, `vcpu_arch_state.rs` | Guest regs; per-vCPU GIC LRs, timer, EL1 sysregs, PAC keys |
| `DeviceManager` | `src/devices/mod.rs` | Enum-dispatch MMIO routing |
| `Scheduler` | `src/scheduler.rs` | Round-robin vCPU scheduler, block/unblock |
| `ExitReason` | `src/arch/aarch64/regs.rs` | VM exit causes (WfiWfe, HvcCall, SmcCall, DataAbort, …) |
| `FfaProxy` | `src/ffa/proxy.rs` | NS-EL2 FF-A v1.1 proxy (VERSION/ID/FEATURES/RXTX/messaging/memory) |
| `Stage2Walker` | `src/ffa/stage2_walker.rs` | Walker from VTTBR_EL2: PTE SW bits, S2AP, map/unmap for cross-VM sharing |
| `FfaDescriptors` | `src/ffa/descriptors.rs` | FF-A v1.1 composite memory region descriptor parsing |
| `PlatformInfo` | `src/dtb.rs` | Runtime DTB parsing (UART/GIC/RAM/CPU discovery) |
| `VSwitch` / `NetRxRing` | `src/vswitch.rs` | L2 vswitch + MAC learning; per-port SPSC RX ring |
| `VirtualPl031` | `src/devices/pl031.rs` | PL031 RTC emulation |
| `SpmcHandler` | `src/spmc_handler.rs` | S-EL2 SPMC event loop + FF-A dispatch (see SPMC section) |
| `SpContext` | `src/sp_context.rs` | Per-SP state machine (Reset→Idle→Running→Blocked→Preempted), `owned_intids[4]`, `pending_irq`, global SpStore |
| `SecureStage2Config` | `src/secure_stage2.rs` | VSTTBR/VSTCR config for SP isolation |
| `Sel2Mmu` | `src/sel2_mmu.rs` | S-EL2 Stage-1 identity map (NS=1 for NWd DRAM) |

### Exception Handling Flow
```
Guest @ EL1 ─trap→ exception.S (save ctx) ─→ handle_exception() (arch/aarch64/hypervisor/exception.rs)
  ├─ WFI → return false (exit to scheduler)
  ├─ HVC → handle_psci() (PSCI v1.0) or HF_INTERRUPT_GET (sel2)
  ├─ SMC → handle_smc() → PSCI / FF-A proxy / forward to EL3
  ├─ Data Abort → HPFAR_EL2 for IPA → decode instr → MMIO dispatch
  ├─ MSR/MRS → ICC_SGI1R_EL1 (SGI emul), sysreg emul
  └─ IRQ → INTID 26 (preempt/poll timer), 27 (vtimer), 33 (UART RX); sel2: per-SP routing via HCR_EL2.VI
─→ advance PC, restore ctx, ERET
```

### SMP / Multi-vCPU
`run_smp()` loops `run_one_iteration()`, each running one vCPU on one pCPU (cooperative + preemptive):
check `pending_cpu_on` → wake vCPUs w/ pending SGIs/SPIs → round-robin pick → drain UART RX (SPI 33) → inject SGIs/SPIs into `ich_lr[]` → arm CNTHP preempt timer (10ms, INTID 26, only if 2+ online) → `vcpu.run()`/`enter_guest()` → handle exit.
- `vcpu_online_mask` **must include vCPU 0 at boot** or preempt timer never arms.
- SGI/IPI: ICC_SGI1R_EL1 trapped via ICH_HCR_EL2.TALL1=1 → decoded → `PENDING_SGIS[vcpu_id]` atomics → injected before entry.

### Multi-pCPU (`multi_pcpu`, `run-linux-smp`)
1:1 vCPU↔pCPU affinity, no scheduler. Secondary pCPUs are powered off — `wake_secondary_pcpus()` issues real PSCI CPU_ON SMC. `TPIDR_EL2` (HW-banked) holds per-pCPU context. `ensure_vtimer_enabled()` programs physical GICR ISENABLER0 before each entry. `inject_spi()` reads physical GICD_IROUTER directly (avoids DEVICES-lock deadlock); cross-pCPU wake via physical SGI 0. WFI passthrough (TWI cleared).

### Multi-VM (`multi_vm`, `run-multi-vm`)
2 VMs round-robin time-sliced on 1 pCPU, each with 4 vCPUs. `VmGlobalState[CURRENT_VM_ID]` replaces flat globals (per-VM SGIs/SPIs/online_mask/current_vcpu/preempt). `DEVICES: [_; MAX_VMS]` per-VM. VMID encoded in VTTBR_EL2[63:48] for TLB isolation. Two-level scheduler: outer VM round-robin → `activate_stage2()` → inner `run_one_iteration()`. Memory: VM0 @ 0x48000000, VM1 @ 0x68000000 (256MB each).

### GIC Emulation
| Component | Address | Mode |
|-----------|---------|------|
| GICD | 0x08000000 | Trap + write-through (`VirtualGicd` shadow) |
| GICR 0-3 | 0x080A0000+ | Trap-and-emulate (`VirtualGicr`, Stage-2 unmapped, 4KB pages) |
| ICC regs | sysregs | Virtual (ICH_HCR_EL2.En=1 → ICV_* at EL1) |
| ICC_SGI1R | sysreg | Trapped (TALL1=1, decoded for IPI) |

LR injection: 4 LRs (ICH_LR0-3_EL2). HW=1 for vtimer (INTID 27) → physical-virtual EOI linkage. EOImode=1 for priority-drop/deactivation split.

### Virtio
- **virtio-blk** @ 0x0a000000 (SPI 16/INTID 48): QueueNotify → `process_request()` → r/w disk image @ 0x58000000 (identity-mapped) → used ring → `inject_spi(48)`.
- **virtio-net** @ 0x0a000200 (SPI 17/INTID 49): 2 vqueues (RX q0, TX q1). TX: strip 12B `virtio_net_hdr_v1` → `vswitch_forward()`. RX: `drain_net_rx()` → `PORT_RX[vm].take()` → write header+frame into RX chain → `inject_spi(49)`.
- **VSwitch**: 16-entry MAC learning, flood broadcast/multicast/unknown-unicast (excl. source), learn src MAC on TX.
- **NetRxRing**: SPSC ring (8 usable slots), up to 1514B frames.
- `platform::virtio_slot(n)` → `(base, intid)`; slot 0 = blk, slot 1 = net, stride 0x200.
- **Auto-IP**: initramfs `/init` reads MAC → assigns `10.0.0.{last_octet}/24`.

### FF-A v1.1 Proxy (`src/ffa/`)
NS-EL2 hypervisor proxy role (pKVM-compatible). Guest SMC trapped via `HCR_EL2.TSC=1` → `handle_smc()` → `ffa::proxy::handle_ffa_call()`.

**Calls**: VERSION, ID_GET, SPM_ID_GET, FEATURES, RXTX_MAP/UNMAP, RX_RELEASE, PARTITION_INFO_GET, MSG_SEND_DIRECT_REQ, MSG_SEND2, MSG_WAIT, RUN, MEM_SHARE/LEND/RETRIEVE_REQ/RELINQUISH/RECLAIM, MEM_FRAG_TX/RX, NOTIFICATION_*, CONSOLE_LOG_32/64. MEM_DONATE blocked (NOT_SUPPORTED). FEATURES returns donated SGI INTIDs for SRI(9)/NPI(8).

- **VM-to-VM sharing**: sender MEM_SHARE → receiver MEM_RETRIEVE_REQ (dynamic Stage-2 page map) → MEM_RELINQUISH (unmap) → sender MEM_RECLAIM. With `tfa_boot` + SP receiver (part_id≥0x8000): SHARE/LEND/RECLAIM forwarded to real SPMC (dual record: local SW bits + SPMC handle).
- **Stub SPMC** (`stub_spmc.rs`): simulates SP1=0x8001/SP2=0x8002 without Secure World (for unit tests). Echoes x4-x7, tracks multi-range `MemShareRecord`.
- **RXTX mailbox** (`mailbox.rs`): per-VM TX/RX IPAs via FFA_RXTX_MAP; used by PARTITION_INFO_GET + composite descriptors.
- **Page ownership** (`memory.rs`): Stage-2 PTE SW bits [56:55] = Owned/SharedOwned/SharedBorrowed/Donated; S2AP [7:6] restricts (SHARE→RO, LEND→NONE). Matches pKVM model.
- **Stage2Walker** (`stage2_walker.rs`): reconstructed from VTTBR_EL2 per-SMC; `map_page`/`unmap_page` create/zero 4KB L3 entries. `PER_VM_VTTBR` holds each VM's L0 PA. Gated `#[cfg(feature="linux_guest")]`.
- **Descriptors** (`descriptors.rs`): FF-A v1.1 composite region (DEN0077A Tbl 5.19-5.25): `FfaMemRegion`(48B)→`FfaMemAccessDesc`(16B)→`FfaCompositeMemRegion`(16B)→`FfaMemRegionAddrRange`(16B). Uses `read_unaligned`. Falls back to register protocol (x3=IPA,x4=count,x5=receiver) when no mailbox.
- **Fragmentation**: MEM_FRAG_TX (sender), MEM_FRAG_RX (receiver, RETRIEVE_RESP > RX buffer).
- **SMC forwarding** (`smc_forward.rs`): `forward_smc()` (inline `smc #0`), `probe_spmc()` (FFA_VERSION), `init()` sets SPMC_PRESENT. Unknown SMCs forwarded to EL3.
- **Routing**: `is_ffa_function(fid)` = 0x84/0xC4 range, low byte ≥ 0x60. PSCI handled separately.

### S-EL2 SPMC (`SpmcHandler`, `sel2` feature)
Event loop + FF-A dispatch:
- **Multi-SP DIRECT_REQ**: `dispatch_to_sp()` + `enter_guest()` ERET; SP-initiated calls handled in `handle_sp_exit()` loop (MEM_RETRIEVE_REQ/RELINQUISH/CONSOLE_LOG → handle locally → re-enter SP).
- **SP→SP DIRECT_REQ**: CallStack cycle detection, recursive `dispatch_to_sp`, chain preemption (Blocked→Preempted sentinel).
- **NS interrupt preemption**: `SP_IRQ_PREEMPTED` flag, CNTHP timer, FFA_INTERRUPT return; `resume_preempted_sp()` via FFA_RUN.
- **Secure vIRQ**: `inject_pending_virq()` (HCR_EL2.VI), cross-SP `dispatch_interrupt_to_sp()`.
- **Memory sharing**: SHARE/LEND/DONATE/RETRIEVE/RELINQUISH/RECLAIM with `SpmcShareRecord`, dynamic Secure Stage-2 map via Stage2Walker; SP↔SP via `handle_sp_exit`.
- **NWd RXTX management**, PARTITION_INFO_GET (24B descriptors to NWd RX), MSG_SEND2/MSG_WAIT (per-SP SpMailbox), CONSOLE_LOG, SRI/NPI feature IDs.

### Other Devices
- **PL011 UART**: full trap-and-emulate. TX → physical UART. RX: physical IRQ 33 → `UART_RX` ring → inject SPI 33. PeriphID/PrimeCellID regs needed for Linux amba-pl011 probe.
- **PL031 RTC** (`devices/pl031.rs`): @ 0x09010000 (SPI 2/INTID 34). `RTCDR = load + CNTVCT/CNTFRQ`. PrimeCell IDs needed for amba probe.

### DTB Runtime Parsing (`src/dtb.rs`)
QEMU passes host DTB in x0 → `boot.S` preserves in x20 → `rust_main(dtb_addr)`. `dtb::init()` (`fdt` crate v0.1.5) discovers UART (`arm,pl011`), GIC (`arm,gic-v3`), RAM (`/memory`), CPUs. Helpers: `gicr_rd_base(cpu)=gicr_base+cpu*0x20000`, `gicr_sgi_base=rd_base+0x10000`. Falls back to QEMU virt defaults if parse fails. `platform::num_cpus()` reads DTB; `MAX_SMP_CPUS=8` is array capacity. Pre-DTB code (`uart_puts`, GIC statics) uses hardcoded `platform::*` constants.

### Memory Layout
| Region | Address | Purpose |
|--------|---------|---------|
| SPMC code (sel2) | 0x0e100000 | S-EL2 linker base (secure DRAM, BL32) |
| SP1/SP2/SP3 | 0x0e300000 / 0x0e400000 / 0x0e500000 | Hello / IRQ / Relay (1MB each) |
| Secure heap | 0x0e600000 | S-EL2 page table alloc |
| Hypervisor (NS) | 0x40200000 | NS-EL2 linker base (avoids QEMU DTB @ 0x40000000) |
| Heap | 0x41000000 (16MB) | `BumpAllocator` page tables |
| DTB/Kernel/Initramfs/Disk (VM0) | 0x47000000 / 0x48000000 / 0x54000000 / 0x58000000 | |
| VM0 RAM | 0x48000000–0x58000000 (256MB; single-VM: 1GB to 0x88000000) | |
| DTB/Kernel/Disk (VM1, multi_vm) | 0x67000000 / 0x68000000 / 0x78000000 | |
| VM1 RAM | 0x68000000–0x78000000 (256MB) | |

**Stage-2 mappers**: `IdentityMapper` (static, 2MB-only, unit tests) vs `DynamicIdentityMapper` (heap, 2MB+4KB, Linux guest, supports `unmap_4kb_page()` for GICR). **Heap gap**: heap is in guest PA range but left Stage-2-unmapped so guest can't corrupt page tables (guest RAM starts at 0x48000000).

### Global State (`src/global.rs`)
`DEVICES[MAX_VMS]` (per-VM MMIO), `VM_STATE[MAX_VMS]` (`VmGlobalState`: pending_sgis/spis, terminal_exit, online_mask, current_vcpu, pending_cpu_on, preempt), `CURRENT_VM_ID`, `PENDING_CPU_ON_PER_VCPU[8]` (multi-pCPU), `SHARED_VTTBR/VTCR`, `PER_VM_VTTBR[MAX_VMS]` (cross-VM Stage-2 for RETRIEVE), `UART_RX`, `PORT_RX[MAX_PORTS]`, `VSWITCH`.

**SPMC globals** (`sel2`, `SpinLock`-protected for per-CPU SPMD concurrency): `NWD_RXTX`, `SPMC_SHARES`, `NOTIF_STATE`, `STAGE2_LOCK` (serializes map/unmap to prevent TOCTOU). Global heap also `SpinLock<Option<BumpAllocator>>`.

### Device Manager Pattern
Enum-dispatch (no `dyn Trait`): `enum Device { Uart, Gicd, Gicr, VirtioBlk, VirtioNet, Pl031 }`. Array routing `[Option<Device>; 8]`, scan `dev.contains(addr)`.

## Build System
- **build.rs**: cross-compiles boot asm + `exception.S` via `aarch64-linux-gnu-gcc` → `libboot.a` (`--whole-archive`). `sel2` → `boot_sel2.S` + `linker_sel2.ld`, else `boot.S` + `linker.ld`.
- **Target**: `aarch64-unknown-none.json` (panic=abort, disable-redzone).
- **Linker**: `linker.ld` base 0x40200000 (NS-EL2); `linker_sel2.ld` base 0x0e100000 (S-EL2).

## Coding Standards
Project-specific firmware rules (full rationale: `docs/RUST_FIRMWARE_CODING_GUIDELINES.md`). Violating these causes silent hangs/UB at EL2/S-EL2, not just lints:
- **`no_std` + core/alloc only**; `Box`/`Vec` via custom `BumpAllocator`.
- **No dynamic dispatch** — enum-dispatch (see `Device`), never `dyn Trait`.
- **No recursion** — statically bounded call graphs (128KB/pCPU stack). Sanctioned exception: SPMC's depth-limited `dispatch_to_sp()` SP→SP chain (CallStack cycle detection).
- **No floating-point/SIMD** — `CPTR_EL3.TFP` traps FP/SIMD from S-EL2 → silent hang. Rust **debug builds emit NEON** for `read_volatile` alignment checks → S-EL2 must build `opt-level >= 1` (see CPTR_EL3.TFP note).
- **Allocate at boot only** — never on hot path (exception/MMIO/SMC).
- **Error handling**: `.expect()` at boot; `Result<T,i32>` for FF-A; **never panic** in exception handlers (return `bool`/`SmcResult8`); `Option`/`bool` for device emul.

## Tests
~457 assertions across 34 test suites run automatically on `make run` (no features), orchestrated in `src/main.rs`, located in `tests/`. Largest suites: `test_spmc_handler` (182 — SPMC dispatch, all FF-A calls, SP↔SP, memory sharing, DONATE, isolation, stress), `test_sp_context` (58 — state machine + illegal transitions), `test_ffa` (55 — proxy calls + descriptors + SMC forward), `test_page_ownership` (9), `test_decode` (9). Others cover DTB, allocator/heap, page tables, scheduler, GIC (gicd/gicr/gicv3_virt), MMIO/decode, virtio-net/vswitch/net_rx_ring, pl031, multi-VM isolation, VMID, secure_stage2, log. `test_timer` exists but is not wired into `main.rs`. When adding a suite, register it in `src/main.rs` and update this count.

## Critical Implementation Details

### HPFAR_EL2 for MMIO (must-know)
When guest MMU is on, `FAR_EL2` = guest VA, NOT IPA. Use `HPFAR_EL2`:
```
IPA = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8 | (far_el2 & 0xFFF)
```

### Never Modify Guest SPSR_EL2
Guest controls its own `PSTATE.I`. Overriding causes spinlock deadlocks.

### CNTHP Timer Must Be Re-enabled
Guest can re-disable INTID 26 via GICR writes. `ensure_cnthp_enabled()` writes physical GICR directly (EL2 bypasses Stage-2) before every vCPU entry.

### ICC_SGI1R_EL1 Bit Fields
TargetList [15:0] (NOT [23:16]), Aff1 [23:16] (NOT [27:24]), INTID [27:24] (NOT [3:0]).

### inject_spi() Must Not Acquire DEVICES Lock (multi-pCPU)
Called from `signal_interrupt()` already inside the `DEVICES` SpinLock → reading `DEVICES.route_spi()` deadlocks (non-reentrant). Multi-pCPU reads physical GICD_IROUTER directly.

### QEMU virt Secondary CPUs Are Powered Off
They do NOT execute `_start`. Must use real PSCI CPU_ON SMC (`smc #0`, fid=0xC4000003) to QEMU EL3 firmware.

### TPIDR_EL2 for Per-CPU Context (multi-pCPU)
`exception.S` uses `mrs x0, tpidr_el2` (HW-banked per pCPU), set by `enter_guest()`.

### Physical GICR Must Be Programmed for SGIs/PPIs
Guest GICR writes only update shadow state. `ensure_vtimer_enabled()` programs physical GICR ISENABLER0 for SGIs 0-15 + PPI 27 before each entry.

### HCR_EL2.TSC for SMC Trapping
`HCR_TSC = 1<<19` traps guest SMC to EL2 as `EC_SMC64 (0x17)`. Unlike HVC, ELR_EL2 = SMC instr itself — handler must advance PC by 4.

### CPTR_EL3.TFP Traps FP/SIMD at S-EL2 (must-know for TF-A builds)
TF-A default `CPTR_EL3.TFP=1` traps ALL FP/SIMD from S-EL2 → EL3. Rust debug `read_volatile` uses NEON (`cnt v0.8b` for alignment popcount) → silent hang on any read. Fix: `CTX_INCLUDE_FPREGS=1` in TF-A build (needs `ENABLE_SVE_FOR_NS=0` + `ENABLE_SME_FOR_NS=0`).

### S-EL2 SPMC Boot (`sel2`)
`boot_sel2.S` → `rust_main_sel2(manifest, hw_config, core_id)`. SPMD passes x0=TOS_FW_CONFIG (@0x0e002000), x4=core_id. Init: vectors → manifest parse → S-EL2 Stage-1 MMU → GIC (PPI 26+29 Secure Group 1) → CNTHCTL_EL2 → Secure Stage-2 → parse SPKG (img_offset=0x4000) → clear SCTLR_EL1/VBAR_EL1 → ERET to SP1 → detect/boot SP2/SP3 → FFA_SECONDARY_EP_REGISTER → event loop. `src/manifest.rs` parses `/attribute` node (FF-A Core Manifest v1.0).

**Secondary warm-boot**: pKVM PSCI CPU_ON → SPMD `spmd_cpu_on_finish_handler()` → ERET to `secondary_entry_sel2` → per-CPU stack (3×32KB in `.bss.sel2_pcpu_stacks`) → `rust_main_sel2_secondary()` (VBAR → reuse primary MMU via `install_sel2_stage1_secondary()` → FFA_MSG_WAIT).

### S-EL2 Stage-1 MMU (`src/sel2_mmu.rs`)
S-EL2 MMU off by default → all accesses hit **Secure** PA space. NWd RXTX PAs are in **Non-Secure** DRAM — writing without translation hits the Secure alias (pKVM reads zeros). Fix: `init_sel2_stage1()` static identity map (3 pages, no heap): L1[1-2] = 1GB blocks @ 0x40000000/0x80000000 **NS=1** Normal WB XN; L2[64-79] = 2MB Device NS=0 (GIC+UART); L2[112-127] = 2MB Normal NS=0 (SPMC+SPs+heap). Regs: MAIR/TCR (T0SZ=16,4KB,48-bit)/TTBR0/SCTLR.{M,C,I}=1. Independent of Secure Stage-2 (VSTTBR_EL2).

### Secure Virtual Interrupt Injection (Phase D)
Hafnium-compatible HCR_EL2.VI for vIRQ to SPs at S-EL1:
1. Per-SP INTID ownership (`owned_intids[4]`; SP2 owns INTID 29).
2. CNTHP poll timer at S-EL2 (CNTPS inaccessible at S-EL1 since SCR_EL3.ST=0).
3. IRQ routing: owned-by-current → queue + VI; owned-by-other → queue + preempt; unowned → FFA_INTERRUPT.
4. HCR_EL2.VI → HW auto-vector to VBAR_EL1+0x280 on ERET.
5. HF_INTERRUPT_GET (HVC x0=0xFF04) → SPMC returns INTID, clears VI.
6. Cross-SP: `dispatch_interrupt_to_sp()` preempts SP1 → SP2 handler → resume SP1.

**SP2 (sp_irq)** (`tfa/sp_irq/`): S-EL1, VBAR_EL1 IRQ handler, DIRECT_REQ_32+64, slow-path busy-loop until vIRQ, responds w/ INTID in x5.

### SP Package Format (SPKG)
BL2 loads raw packages to `load-address` from `tb_fw_config.dts`. SPKG header (24B LE): magic("SPKG"), version, pm_offset(0x1000), pm_size, img_offset(0x4000), img_size. SPMC enters SP at `load_addr + img_offset`. `sp_manifest.dts` UUID gets byte-swapped by `sp_mk_generator.py`; `tb_fw_config.dts` UUID must match swapped form. Verify with `fiptool info fip.bin`.

### Diagnostic Fault Handler (`exception.S`)
`fault_diag_print` handles exceptions when TPIDR_EL2=0 (host-level fault). Prints ESR/ELR/FAR/HPFAR_EL2 to UART. Used during S-EL2 boot. At end of `exception.S` (outside vector alignment).

### Platform Constants
Guest addresses (heap, kernel load, disk) in `src/platform.rs`. Host hardware (UART/GIC/RAM/CPU) discovered at runtime via `src/dtb.rs` — use `platform::num_cpus()`/`dtb::platform_info()`, not hardcoded constants. `MAX_SMP_CPUS=8` capacity; `SMP_CPUS=4` fallback.

## Related Documentation
- `ARCHITECTURE.md` / `docs/architecture.md` — narrative architecture (latter is exhaustive)
- `DEVELOPMENT_PLAN.md` — full roadmap + per-sprint detail
- `REQUIREMENTS.md` — feature requirements
- `CONTRIBUTING.md` — dev setup
- `docs/RUST_FIRMWARE_CODING_GUIDELINES.md` — full coding standards

## Roadmap (NS-EL2 → S-EL2 SPMC → pKVM)
All phases below are **done** unless noted; see `DEVELOPMENT_PLAN.md` for per-sprint detail.

> **E2E re-verified 2026-05-26** on a native ARM64 (aarch64) Linux host using the **distro QEMU 8.2.2** (no custom QEMU 9.2.3 needed): `sg docker -c 'make build-tfa-spmc'` + `make run-spmc` boots the full chain TF-A BL31 v2.12.0+SPMD @ EL3 → our SPMC @ S-EL2 → SP1/SP2/SP3 @ S-EL1 → BL33 FF-A client, **20/20 BL33 tests PASS**. The Makefile note "secure=on requires QEMU 9.2+" is overly cautious; 8.2.2 handles S-EL2 fine. `make run` (NS-EL2 TCG unit tests) also passes 34/34. Secure world is TCG-only (KVM can't virtualize EL3/Secure) — and that's sufficient.

- **Phase 3**: NS-EL2 complete (2MB block split, notifications, indirect messaging).
- **Phase 4 / Sprint 5.1-5.2 / Phase C / Phase D**: TF-A boot chain, SPMC + SPs, DIRECT_REQ E2E (NS proxy → SPMD → SPMC → SP), RXTX + PARTITION_INFO forwarding, NS interrupt preemption, multi-SP + secure vIRQ injection.
- **Phase 4.5**: pKVM @ NS-EL2 + our SPMC @ S-EL2 — `make run-pkvm` boots AOSP android16-6.12 to BusyBox (`Protected hVHE mode initialized successfully`), FF-A v1.1 discovery works.
- **M4.6 + Phase 4.6**: SPMC-side + true E2E memory sharing, FfaMemRegion struct fix, NWd-vs-SP RETRIEVE distinction, SP2 DIRECT_REQ_64. `ffa_test.ko`: 20/20.
- **Phase 4.7**: security hardening (cross-SP isolation, IPA/page-count validation, fragment tracking, stress tests, MEM_LEND negatives).
- **Phase 5.1 (+pKVM)**: SP→SP DIRECT_REQ (cycle detection, chain preemption, SP3 relay), SP↔SP MEM_SHARE/RECLAIM, MEM_DONATE (irrevocable). `ffa_test.ko`: 35/35. ~457 assertions / 34 suites.
- **Phase 4.5 AVF** (partial): crosvm VMM in pKVM host (EL0) creates a VM via /dev/kvm. KVM API validated (5/5: /dev/kvm, CREATE_VM, CREATE_VCPU). Blocked: crosvm `failed to create IRQ chip` (QEMU TCG can't create the in-kernel vGICv3 `KVM_DEV_TYPE_ARM_VGIC_V3`). **Re-confirmed 2026-05-26 on a native ARM64 host + stock QEMU 8.2.2** (`make run-crosvm`, full rebuild via Docker): the guest boots pKVM cleanly (`Protected hVHE mode initialized successfully`, `nv: 554/669 trap handlers`, `/dev/kvm` PASS) but crosvm still dies at `failed to create IRQ chip` ~1s after `crosvm run`. The Makefile's crosvm command is **non-protected** (no `--protected-vm`), so this also proves a *normal* (non-pVM) crosvm guest hits the same vGICv3 wall — protected vs non-protected is irrelevant; the wall is the in-kernel vGICv3 under TCG, and being on real ARM hardware does not change it. crosvm is KVM-only (no emulation fallback). Real fix needs nested-virt KVM (`-accel kvm` + host `kvm.nested=1`) or native crosvm on the host (`/dev/kvm` + pKVM-mode boot). NOTE: KVM cannot virtualize EL3/Secure, so the full NS→Secure chain stays TCG regardless of host (and that's sufficient — 20/20, verified above).
- **Phase 5** (next): RME & CCA (Realm Manager).
