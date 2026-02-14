# Multi-pCPU Support Design

**Date**: 2026-02-15
**Status**: Approved
**Phase**: 9

## Goal

Run 4 guest vCPUs on 4 physical CPUs in parallel, eliminating the single-pCPU cooperative/preemptive scheduler. This transforms the hypervisor from a time-sharing model to a true parallel execution model.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| vCPU affinity | Fixed (vCPU N → pCPU N), migration-ready | Simplest; eliminates scheduler. Architecture doesn't block future migration. |
| GIC strategy | Keep VirtualGicr/VirtualGicd trap-and-emulate | Already implemented (Phase 7-8). Supports N:M if needed later. KVM uses same approach. |
| Device locking | Per-device spinlock | GICD, Virtio, UART each get a spinlock. GICR needs no lock (per-vCPU isolation with fixed affinity). |
| PSCI CPU_ON | WFE/SEV wakeup | pCPU is idle when CPU_ON arrives; WFE/SEV is simplest ARM wakeup. |
| Cross-pCPU SGI/SPI | Physical SGI at EL2 | SEV cannot interrupt guest execution. Physical SGI forces VM exit → check pending → inject. |

## Architecture Overview

```
pCPU 0                    pCPU 1                    pCPU 2                    pCPU 3
┌─────────────┐          ┌─────────────┐          ┌─────────────┐          ┌─────────────┐
│ run_vcpu(0)  │          │ run_vcpu(1)  │          │ run_vcpu(2)  │          │ run_vcpu(3)  │
│  vCPU 0     │          │  vCPU 1     │          │  vCPU 2     │          │  vCPU 3     │
│  ┌────────┐ │          │  ┌────────┐ │          │  ┌────────┐ │          │  ┌────────┐ │
│  │VcpuCtx │ │          │  │VcpuCtx │ │          │  │VcpuCtx │ │          │  │VcpuCtx │ │
│  │ArchSt  │ │          │  │ArchSt  │ │          │  │ArchSt  │ │          │  │ArchSt  │ │
│  └────────┘ │          │  └────────┘ │          │  └────────┘ │          │  └────────┘ │
│  PerCpuCtx  │          │  PerCpuCtx  │          │  PerCpuCtx  │          │  PerCpuCtx  │
└──────┬──────┘          └──────┬──────┘          └──────┬──────┘          └──────┬──────┘
       │ trap                   │ trap                   │ trap                   │ trap
       ▼                        ▼                        ▼                        ▼
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                           Shared State                                                  │
│  ┌──────────────────┐  ┌──────────────────┐  ┌────────────────┐                        │
│  │ VirtualGicd 🔒   │  │ VirtioBlk 🔒     │  │ VirtualUart 🔒 │                        │
│  └──────────────────┘  └──────────────────┘  └────────────────┘                        │
│  ┌──────────────────────────────────────────┐                                          │
│  │ VirtualGicr (per-vCPU, no lock needed)   │                                          │
│  └──────────────────────────────────────────┘                                          │
│  ┌──────────────────────────────────────────┐                                          │
│  │ PENDING_SGIS/SPIS[vcpu_id] (atomics)     │                                          │
│  └──────────────────────────────────────────┘                                          │
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

## Boot Sequence

### Phase 1: Assembly Entry (`boot.S`)

```
_start:
    mrs x0, MPIDR_EL1
    and x0, x0, #0xFF          // Extract Aff0 = CPU ID
    cbz x0, primary_boot       // CPU 0 → primary path

secondary_wait:
    wfe                         // Wait for SEV from CPU 0
    adr x1, BOOT_READY
    ldr w2, [x1, x0, lsl #2]   // BOOT_READY[my_id]
    cbz w2, secondary_wait     // Not ready yet, keep waiting
    // Set up per-pCPU stack
    adr x1, pcpu_stacks
    add sp, x1, x0, lsl #14   // stack = base + cpu_id * 16KB
    add sp, sp, #(16 * 1024)   // Point to top
    mov x0, x0                 // Pass cpu_id as arg
    b rust_main_secondary

primary_boot:
    // Existing boot path (BSS clear, heap init, etc.)
    adr x0, stack_top
    mov sp, x0
    bl rust_main
```

### Phase 2: Primary CPU Init (`rust_main`)

1. Initialize heap, Stage-2 page tables, devices (same as today)
2. Allocate per-pCPU stacks (4 × 16KB)
3. Set `BOOT_READY[1..3] = 1`
4. Execute `SEV` to wake secondary CPUs
5. Create vCPU 0, enter `run_vcpu(0)`

### Phase 3: Secondary CPU Init (`rust_main_secondary`)

1. Set VBAR_EL2 (exception vectors, same table as CPU 0)
2. Set VTTBR_EL2 (shared Stage-2 page tables)
3. Set HCR_EL2 (same flags as CPU 0)
4. Initialize per-pCPU GIC state (enable Group 1 at EL2)
5. Enter idle loop: `WFE` waiting for `PENDING_CPU_ON[my_id]`

### Phase 4: PSCI CPU_ON

```
Guest vCPU 0 on pCPU 0:
  HVC CPU_ON(target=1, entry, context)
    → trap to EL2
    → PENDING_CPU_ON[1] = { entry, context }  (atomic store-release)
    → SEV                                       (wake pCPU 1)
    → return SUCCESS to guest

pCPU 1 (idle loop):
  WFE wakes up
    → load-acquire PENDING_CPU_ON[1]
    → initialize vCPU 1 (set ELR, X0, SPSR)
    → enter run_vcpu(1)
```

## Per-pCPU Context

```rust
/// Per-physical-CPU context. Indexed by MPIDR Aff0.
/// Each pCPU only accesses its own entry — no locking needed.
pub struct PerCpuContext {
    /// Which vCPU this pCPU is running (fixed affinity: cpu_id == vcpu_id)
    pub vcpu_id: usize,
    /// Per-pCPU exception loop counter (replaces global EXCEPTION_COUNT)
    pub exception_count: u32,
}

/// Stored in a static array, indexed by pCPU ID.
/// Access pattern: MPIDR_EL1.Aff0 → PER_CPU[aff0]
static PER_CPU: [PerCpuContext; SMP_CPUS] = ...;
```

**How to read CPU ID at EL2**: `mrs x0, MPIDR_EL1; and x0, x0, #0xFF` gives Aff0 = physical CPU index.

## Cross-pCPU Interrupt Delivery

### Virtual SGI (guest-to-guest IPI)

```
vCPU 0 @ pCPU 0 writes ICC_SGI1R_EL1:
  1. Trap (TALL1=1) → EL2 handler on pCPU 0
  2. Decode TargetList[15:0], Aff1[23:16], INTID[27:24]
  3. For each target vCPU T:
     a. PENDING_SGIS[T].fetch_or(1 << intid, Release)
     b. If T != current_vcpu → send physical SGI to pCPU T
        (Write ICC_SGI1R_EL1 at EL2, targeting pCPU T's Aff0)
  4. If self-targeted: inject immediately into own LRs
  5. Return to guest
```

### Physical SGI Reception (target pCPU)

```
pCPU T running guest vCPU T:
  Physical SGI arrives → IRQ exception → VM exit to EL2
  EL2 IRQ handler:
    1. ACK physical SGI (ICC_IAR1_EL1 at EL2)
    2. EOI physical SGI (ICC_EOIR1_EL1 at EL2)
    3. Check PENDING_SGIS[T] → inject into ICH_LR
    4. Check PENDING_SPIS[T] → inject into ICH_LR
    5. Return to guest (ERET)
```

### Virtual SPI (device → guest)

```
Virtio-blk on pCPU 0 completes request, target = vCPU 2:
  1. PENDING_SPIS[2].fetch_or(1 << (intid - 32), Release)
  2. If pCPU 2 is running guest → send physical SGI to pCPU 2
  3. pCPU 2 exits guest, injects SPI, re-enters
```

### Reserved Physical SGI

Use a dedicated SGI INTID for hypervisor-to-hypervisor signaling:

- **INTID 0**: Reserved for EL2 cross-pCPU "kick" signal
- Guest SGIs (INTID 0-15) remain software-emulated via PENDING_SGIS atomics
- The physical SGI 0 only means "check your pending queues", not a specific guest interrupt

## Synchronization

### Spinlock Implementation

```rust
/// Ticket-based spinlock for bare-metal no_std.
/// Uses WFE/SEV for efficient spinning.
pub struct SpinLock<T> {
    next_ticket: AtomicU32,
    now_serving: AtomicU32,
    data: UnsafeCell<T>,
}
```

ARM memory ordering: `ldaxr`/`stlxr` for ticket acquire, `stlr` for release. WFE in spin loop to save power.

### Per-Device Lock Scope

| Device | Lock | Scope |
|--------|------|-------|
| VirtualGicd | `SpinLock<VirtualGicd>` | Protects shadow state (irouter, enabled, ctlr). Held during read/write emulation + physical write-through. |
| VirtioBlk | `SpinLock<VirtioMmioTransport<VirtioBlk>>` | Protects descriptor ring, used ring, device state. |
| VirtualUart | `SpinLock<VirtualUart>` | Protects TX/RX buffer state. |
| VirtualGicr | No lock | Per-vCPU state. Fixed affinity → only one pCPU accesses each vCPU's GICR state. |

### Atomics (Already Safe)

| Global | Access Pattern | Safe? |
|--------|---------------|-------|
| `PENDING_SGIS[vcpu_id]` | `fetch_or` (writer) / `swap(0)` (consumer) | Yes — per-vCPU, only one consumer (owner pCPU) |
| `PENDING_SPIS[vcpu_id]` | Same as above | Yes |
| `PENDING_CPU_ON[vcpu_id]` | Store-release / load-acquire | Yes — changed from single global to per-vCPU array |
| `VCPU_ONLINE_MASK` | `fetch_or` / read | Yes |

## Global State Changes

| Current | Multi-pCPU |
|---------|-----------|
| `CURRENT_VCPU_ID: AtomicUsize` | **Removed** — use `MPIDR_EL1.Aff0` directly (1:1 affinity) |
| `PREEMPTION_EXIT: AtomicBool` | **Removed** — no preemptive scheduler |
| `PENDING_CPU_ON: PendingCpuOn` (single) | `PENDING_CPU_ON: [AtomicCpuOnReq; SMP_CPUS]` (per-vCPU) |
| `EXCEPTION_COUNT: AtomicU32` (global) | Per-pCPU in `PerCpuContext` |
| `DEVICES: UnsafeCell<DeviceManager>` | Per-device spinlocks wrapping each Device |

## Components Removed

| Component | Reason |
|-----------|--------|
| `Scheduler` (round-robin) | 1:1 affinity, no scheduling needed |
| `PREEMPTION_EXIT` | No time-slice sharing |
| CNTHP 10ms preemption timer | No preemption |
| `run_smp()` main loop | Replaced by per-pCPU `run_vcpu()` |
| TWI trap (WFI blocking) | Guest WFI → real WFI on pCPU (both idle) |

**Note**: `Scheduler` and `run_smp()` code should be kept behind `#[cfg(not(feature = "multi_pcpu"))]` for backwards compatibility with single-pCPU mode.

## WFI Handling Change

**Current** (single pCPU): WFI traps to EL2 (TWI=1), run_smp() marks vCPU blocked, switches to another.

**Multi-pCPU**: Clear TWI in HCR_EL2. Guest WFI executes as real WFI — pCPU halts until interrupt. When a cross-pCPU SGI or timer fires, the pCPU wakes and the guest handles the interrupt directly.

This is both simpler and more efficient: no trap overhead for WFI.

## Feature Flag

New Cargo feature: `multi_pcpu`

```toml
[features]
multi_pcpu = ["linux_guest"]  # Implies linux_guest (needs DynamicIdentityMapper)
```

Build: `make run-linux-smp` (or extend existing `make run-linux` with `--features multi_pcpu`)

The single-pCPU `run_smp()` path stays available without the feature flag.

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Spinlock deadlock in exception handler | System hang | Lock ordering discipline: GICD → Virtio → UART. Never hold two locks. |
| Physical SGI lost/delayed | Guest SGI not delivered | Use edge-triggered SGI + check pending on every VM entry |
| Secondary pCPU boot failure | Only 1 CPU works | Timeout in idle loop, fall back to single-pCPU mode |
| Heap allocator not thread-safe | Corruption | All heap allocation happens on pCPU 0 during init, before secondaries start |
| Stage-2 TLB stale entries | Guest memory fault | Not an issue: page tables are read-only after boot. No runtime remapping. |

## Acceptance Criteria

1. Linux boots to BusyBox shell with 4 vCPUs on 4 pCPUs
2. `smp: Brought up 1 node, 4 CPUs` in dmesg
3. No RCU stalls, no deadlocks, no watchdog lockups
4. SGI/IPI delivery works across physical CPUs
5. `virtio_blk virtio0: [vda]` probe succeeds
6. Unit tests still pass (`make run` without `multi_pcpu` feature)

## References

- [KVM ARM64 Virtualization (DeepWiki)](https://deepwiki.com/torvalds/linux/3.2-kvm-arm64-virtualization)
- [Hafnium Architecture](https://hafnium.readthedocs.io/en/latest/hypervisor/Architecture.html)
- [ARM VGICv3 Kernel Docs](https://docs.kernel.org/virt/kvm/devices/arm-vgic-v3.html)
- [Bao Hypervisor](https://github.com/bao-project/bao-hypervisor)
- [Static Partitioning Hypervisors Comparison](https://arxiv.org/pdf/2303.11186)
