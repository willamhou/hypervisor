# GICv3 Virtual Interrupt Implementation

**Version**: v0.7.0
**Last updated**: 2026-02-14
**Status**: Verified — Linux 6.12.12 boots to BusyBox shell with 4 vCPUs, no RCU stalls

---

## Overview

The hypervisor uses ARM GICv3 hardware virtualization to provide interrupt services to the guest. The implementation combines three strategies:

| Component | Strategy | Purpose |
|-----------|----------|---------|
| GICD (0x08000000) | Passthrough + shadow | Guest writes directly; VirtualGicd shadows IROUTER for SPI routing |
| GICR (0x080A0000+) | Trap-and-emulate | Stage-2 unmapped (4KB pages); VirtualGicr emulates per-vCPU state |
| ICC system regs | Virtual redirect | ICH_HCR_EL2.En=1 redirects ICC_* to ICV_* at EL1 |
| ICC_SGI1R_EL1 | Trapped (TALL1) | Decoded for IPI emulation across vCPUs |
| ICH_LR_EL2 | Direct | 4 List Registers for virtual interrupt injection |

## List Register Injection

### LR Format (64-bit)

```
Bits [63:62] - State: 00=Invalid, 01=Pending, 10=Active, 11=Pending+Active
Bit  [61]    - HW: 1=physical-virtual linkage (pINTID in [41:32])
Bit  [60]    - Group: 1=Group 1
Bits [55:48] - Priority (0x00 = highest)
Bits [41:32] - pINTID (physical INTID, when HW=1)
Bits [31:0]  - vINTID (virtual INTID)
```

### Injection Paths

1. **SGI (INTID 0-15)**: Queued in `PENDING_SGIS[vcpu_id]` atomics, injected into `arch_state.ich_lr[]` before `vcpu.run()`.
2. **SPI (INTID 32+)**: Queued in `PENDING_SPIS[vcpu_id]` atomics, injected before run or flushed immediately via `flush_pending_spis_to_hardware()`.
3. **Virtual Timer (INTID 27)**: Injected with **HW=1** (pINTID=27) in IRQ handler. Guest EOI auto-deactivates physical interrupt.
4. **Direct**: `GicV3VirtualInterface::inject_interrupt()` writes hardware LRs from exception handler context.

### EOImode=1

ICC_CTLR_EL1.EOImode=1 is set at EL2, splitting EOI into:
- **EOIR** (priority drop): Guest writes ICC_EOIR1_EL1
- **DIR** (deactivation): Hypervisor calls `GicV3SystemRegs::write_dir()` for non-HW interrupts

For HW=1 interrupts (vtimer), the guest's virtual EOI automatically deactivates the physical interrupt — no DIR needed.

## GICR Trap-and-Emulate (Phase 7)

### Architecture

Each GICv3 Redistributor (GICR) has two 64KB frames:
- **RD frame** (offset 0x00000): GICR_CTLR, GICR_WAKER, GICR_TYPER, etc.
- **SGI frame** (offset 0x10000): GICR_IGROUPR0, GICR_ISENABLER0, GICR_ICENABLER0, etc.

The hypervisor unmaps all 4 GICRs (0-3) via Stage-2 4KB page unmapping (32 pages per GICR = 128KB). Guest accesses trap as Data Aborts to EL2, where `VirtualGicr` emulates the registers.

### VirtualGicr State

```rust
pub struct GicrState {
    pub igroupr0: u32,      // Interrupt Group Register
    pub isenabler0: u32,    // Interrupt Set-Enable
    pub icenabler0: u32,    // Interrupt Clear-Enable (shadow)
    pub ipriorityr: [u8; 32], // Priority for INTIDs 0-31
    pub icfgr0: u32,        // SGI configuration
    pub icfgr1: u32,        // PPI configuration
}
```

Per-vCPU state array: `states: [GicrState; SMP_CPUS]`. GICR index computed from IPA:
```
gicr_index = (ipa - GICR0_RD_BASE) / GICR_FRAME_SIZE
vcpu_id = gicr_index  (identity: GICR N → vCPU N)
```

### Key Emulated Registers

| Register | Offset | Behavior |
|----------|--------|----------|
| GICR_CTLR | 0x0000 | Returns 0 (RWP=0, no LPIs) |
| GICR_WAKER | 0x0014 | Returns 0 (ProcessorSleep=0, ChildrenAsleep=0) |
| GICR_TYPER | 0x0008 | Returns per-vCPU Aff0, Last bit for final GICR |
| GICR_IGROUPR0 | 0x10080 | Tracked per-vCPU, read/write |
| GICR_ISENABLER0 | 0x10100 | Write-1-to-set semantics |
| GICR_ICENABLER0 | 0x10180 | Write-1-to-clear semantics |
| GICR_IPRIORITYR | 0x10400-0x1041F | Per-interrupt priority, byte access |
| GICR_ICFGR0/1 | 0x10C00/0x10C04 | Edge/level configuration |

## SGI/IPI Emulation

### Trap Mechanism

ICH_HCR_EL2.TALL1=1 traps guest writes to ICC_SGI1R_EL1 as MSR exceptions (EC=0x18).

### ICC_SGI1R_EL1 Bit Fields

**CRITICAL** — these differ from some documentation:

| Field | Bits | Description |
|-------|------|-------------|
| TargetList | [15:0] | Bitmap of target PEs (bit N = Aff0=N) |
| Aff1 | [23:16] | Affinity level 1 |
| INTID | [27:24] | SGI interrupt ID (0-15) |
| Aff2 | [39:32] | Affinity level 2 |
| IRM | [40] | 1=target all PEs except self |
| RS | [47:44] | Range Selector |
| Aff3 | [55:48] | Affinity level 3 |

### SGI Flow

```
Guest writes ICC_SGI1R_EL1
  → TALL1 trap to EL2
  → handle_sgi_trap() decodes TargetList, INTID, IRM
  → Self-targeting: inject directly via hardware LR
  → Cross-vCPU: queue in PENDING_SGIS[target_vcpu] atomic
  → run_smp() loop: wake_pending_vcpus() unblocks targets
  → inject_pending_sgis() drains queue into arch_state.ich_lr[]
  → vcpu.run() → arch_state.restore() → hardware LRs set
  → ERET → guest receives SGI
```

## GICD Shadow State

`VirtualGicd` intercepts GICD writes to track:

- **GICD_IROUTER[N]**: SPI N routing affinity (Aff0 field → target vCPU ID)
- **GICD_ISENABLER[N]**: SPI enable state

Used by `inject_spi()` to route SPIs to the correct vCPU's `PENDING_SPIS` array based on IROUTER Aff0.

## Virtual Timer (INTID 27)

1. Physical timer fires → IRQ trap to EL2 (HCR_EL2.IMO=1)
2. `handle_irq_exception()` acknowledges via ICC_IAR1_EL1
3. `mask_guest_vtimer()` disables timer to stop re-firing
4. `inject_hw_interrupt(27, 27, priority)` writes LR with HW=1, pINTID=27
5. Guest acknowledges via ICV_IAR1_EL1 (virtual) → LR state: Pending→Active
6. Guest EOIs via ICV_EOIR1_EL1 → hardware auto-deactivates physical INTID 27
7. Timer unmasks on next guest timer write

## Preemption Timer (INTID 26)

CNTHP_EL2 (EL2 physical timer) fires every 10ms for preemptive scheduling:

1. `arm_preemption_timer()` sets CNTHP_CVAL and enables CNTHP_CTL
2. Physical IRQ → INTID 26 → `handle_irq_exception()`
3. Sets `PREEMPTION_EXIT=true` → returns false → exits to scheduler
4. `ensure_cnthp_enabled()` re-enables INTID 26 in GICR before every vCPU entry (guest may disable it via GICR writes)

## Source Files

| File | Role |
|------|------|
| `src/arch/aarch64/peripherals/gicv3.rs` | GicV3SystemRegs, GicV3VirtualInterface, LR management |
| `src/devices/gic/distributor.rs` | VirtualGicd — GICD trap-and-emulate, IROUTER shadow |
| `src/devices/gic/redistributor.rs` | VirtualGicr — GICR trap-and-emulate, per-vCPU state |
| `src/arch/aarch64/vcpu_arch_state.rs` | Per-vCPU ICH_LR/VMCR/HCR save/restore |
| `src/vm.rs` | inject_pending_sgis/spis, wake_pending_vcpus, ensure_cnthp_enabled |
| `src/arch/aarch64/hypervisor/exception.rs` | handle_irq_exception, handle_sgi_trap, flush_pending_spis |
| `src/global.rs` | PENDING_SGIS, PENDING_SPIS, inject_spi() |

## Implementation Checklist

### Core GICv3 (Sprint 1.6)
- [x] ICC system register interface (ICC_IAR1, ICC_EOIR1, ICC_PMR, ICC_IGRPEN1)
- [x] ICH virtual interface (ICH_VTR, ICH_HCR, ICH_VMCR, ICH_LR0-3)
- [x] List Register injection (`inject_interrupt`, `inject_hw_interrupt`)
- [x] EOImode=1 (split priority drop / deactivation)
- [x] HW=1 for virtual timer (physical-virtual EOI linkage)
- [x] GICv3 availability detection (ID_AA64PFR0_EL1)

### Multi-vCPU GIC (Phase 7 / M2)
- [x] Per-vCPU LR save/restore (VcpuArchState)
- [x] Per-vCPU ICH_VMCR/HCR save/restore
- [x] TALL1 SGI trap (ICC_SGI1R_EL1 emulation)
- [x] PENDING_SGIS atomic queuing and injection
- [x] PENDING_SPIS atomic queuing and injection
- [x] flush_pending_spis_to_hardware() (low-latency SPI delivery)
- [x] GICR trap-and-emulate (VirtualGicr, 4KB unmap)
- [x] GICD shadow state (VirtualGicd, IROUTER tracking)
- [x] SPI routing via GICD_IROUTER Aff0
- [x] GICR WAKER management for secondary CPUs
- [x] ensure_cnthp_enabled() (re-enable INTID 26)
- [x] CNTHP preemption timer (10ms, INTID 26)

### Verified
- [x] Linux 6.12.12 boots with 4 vCPUs, no RCU stalls
- [x] Virtio-blk detected and functional
- [x] BusyBox shell interactive
