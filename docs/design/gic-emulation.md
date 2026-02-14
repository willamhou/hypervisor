# GIC Emulation Architecture

This document describes how the hypervisor virtualizes the ARM GICv3 interrupt controller for the guest.

## Strategy Overview

The hypervisor uses a hybrid approach — some GIC components are passed through to hardware, others are trapped and emulated:

| Component | Address | Strategy | Reason |
|-----------|---------|----------|--------|
| GICD | 0x08000000 | Trap + write-through | Guest writes trapped, forwarded to physical GICD + shadow state |
| GICR 0-3 | 0x080A0000+ | Trap-and-emulate | Per-vCPU state must be multiplexed on single pCPU |
| ICC regs | System regs | Virtual redirect | ICH_HCR_EL2.En=1 redirects to ICV_* at EL1 |
| ICC_SGI1R | System reg | Trapped (TALL1) | Decoded for cross-vCPU IPI delivery |

## GICD Shadow State

`VirtualGicd` (`src/devices/gic/distributor.rs`) intercepts GICD MMIO writes via Stage-2 Data Abort trap and maintains shadow copies of key registers:

### Tracked Registers

- **GICD_IROUTER[N]** (0x6100+): SPI N routing. Aff0 field maps to target vCPU ID.
- **GICD_ISENABLER[N]**: SPI enable state (write-1-to-set).
- **GICD_ICENABLER[N]**: SPI disable state (write-1-to-clear).

### SPI Routing

When `inject_spi(intid)` is called:
1. Look up `GICD_IROUTER[intid]` in shadow state
2. Extract Aff0 field → target vCPU ID
3. Queue in `PENDING_SPIS[target_vcpu]` atomic

If IROUTER is 0 or not set, SPI defaults to vCPU 0.

## GICR Trap-and-Emulate

### Why Trap?

Each physical GICR frame is wired to a specific physical CPU. With 4 vCPUs on 1 pCPU, the guest expects 4 independent GICRs. Passthrough would mean all 4 vCPUs see the same physical GICR0 state, causing:
- Enable/disable conflicts between vCPUs
- Wrong GICR_TYPER (Aff0 always = 0)
- WAKER state confusion

### Stage-2 Setup

In `vm.rs:init_memory_dynamic()`:
1. Map entire GIC region (0x08000000, 16MB) as DEVICE
2. Use `DynamicIdentityMapper::unmap_4kb_page()` to unmap all 4 GICRs (0-3)
3. Each GICR = 128KB = 32 x 4KB pages

Guest access to unmapped pages → Stage-2 Data Abort → `handle_mmio_abort()` → `DeviceManager` → `VirtualGicr`.

### VirtualGicr Implementation

`VirtualGicr` (`src/devices/gic/redistributor.rs`) maintains per-vCPU state:

```rust
pub struct GicrState {
    igroupr0: u32,          // Interrupt Group (SGIs/PPIs)
    isenabler0: u32,        // Interrupt Set-Enable
    icenabler0: u32,        // Interrupt Clear-Enable (shadow)
    ipriorityr: [u8; 32],   // Priority for INTIDs 0-31
    icfgr0: u32,            // SGI edge/level config
    icfgr1: u32,            // PPI edge/level config
}
```

State array: `states: [GicrState; SMP_CPUS]`

### Address Decoding

GICR index from IPA:
```
frame_offset = ipa - GICR0_RD_BASE
gicr_index = frame_offset / GICR_FRAME_SIZE  (128KB per GICR)
is_sgi_frame = (frame_offset % GICR_FRAME_SIZE) >= 0x10000
register_offset = frame_offset % 0x10000
```

### Key Register Emulation

**RD Frame (offset 0x00000):**

| Register | Offset | Read | Write |
|----------|--------|------|-------|
| GICR_CTLR | 0x0000 | Returns 0 | Ignored |
| GICR_IIDR | 0x0004 | Returns 0 (JEP106=0) | N/A |
| GICR_TYPER | 0x0008 | Aff0=vcpu_id, Last=1 for final | N/A |
| GICR_WAKER | 0x0014 | Returns 0 (awake) | Ignored |

**SGI Frame (offset 0x10000):**

| Register | Offset | Read | Write |
|----------|--------|------|-------|
| GICR_IGROUPR0 | 0x0080 | Shadow state | Stored |
| GICR_ISENABLER0 | 0x0100 | Shadow isenabler0 | OR into isenabler0 |
| GICR_ICENABLER0 | 0x0180 | Shadow isenabler0 | Clear bits in isenabler0 |
| GICR_IPRIORITYR | 0x0400-0x041F | Priority bytes | Stored per-byte |
| GICR_ICFGR0 | 0x0C00 | Shadow icfgr0 | Stored |
| GICR_ICFGR1 | 0x0C04 | Shadow icfgr1 | Stored |

## SGI/IPI Emulation

### ICC_SGI1R_EL1 Encoding

```
[55:48] Aff3    [47:44] RS      [40] IRM
[39:32] Aff2    [27:24] INTID   [23:16] Aff1
[15:0]  TargetList
```

**Common mistake**: TargetList is bits [15:0], NOT [23:16]. INTID is bits [27:24], NOT [3:0].

### Emulation Flow (handle_sgi_trap)

```rust
let target_list = (value & 0xFFFF) as u32;       // bits [15:0]
let intid = ((value >> 24) & 0xF) as u32;        // bits [27:24]
let irm = (value >> 40) & 1;                      // bit [40]
```

- **IRM=0**: Iterate TargetList bits. Bit N = vCPU with Aff0=N.
  - Self-targeting → inject directly via hardware LR
  - Cross-vCPU → queue in `PENDING_SGIS[target]`
- **IRM=1**: Target all online vCPUs except self.

## List Register Management

### 4 LRs per vCPU

Saved in `VcpuArchState.ich_lr[0..3]`. Restored to hardware (ICH_LR0-3_EL2) by `arch_state.restore()` before guest entry.

### Injection Priority

1. **Exception handler context** (hardware LRs live): `flush_pending_spis_to_hardware()` writes directly
2. **run_smp() context** (software state): `inject_pending_sgis/spis()` write to `arch_state.ich_lr[]`

### LR Overflow

When all 4 LRs are occupied, the interrupt is re-queued:
```rust
if !injected {
    PENDING_SGIS[vcpu_id].fetch_or(1 << sgi, Ordering::Relaxed);
}
```

### HW=1 for Virtual Timer

Virtual timer (INTID 27) uses HW=1 with pINTID=27:
```
LR = State=Pending | HW=1 | Group1 | Priority | pINTID=27 | vINTID=27
```

Guest EOI on ICV_EOIR1_EL1 automatically deactivates the physical INTID 27 — no hypervisor DIR needed.

## Virtual Interface Control

### ICH_HCR_EL2

Per-vCPU, saved/restored in `VcpuArchState.ich_hcr`:
- Bit 0 (En): Enable virtual CPU interface
- Bit 13 (TALL1): Trap ICC_SGI1R_EL1 writes

Default: `(1 << 13) | 1` = TALL1 + En

### ICH_VMCR_EL2

Per-vCPU, saved/restored in `VcpuArchState.ich_vmcr`:
- Bits [31:24] (VPMR): Virtual Priority Mask = 0xFF (allow all)
- Bit 1 (VENG1): Virtual Group 1 enable = 1

Default: `(0xFF << 24) | (1 << 1)`

## Source Files

| File | Role |
|------|------|
| `src/arch/aarch64/peripherals/gicv3.rs` | GicV3SystemRegs, GicV3VirtualInterface, LR read/write |
| `src/devices/gic/distributor.rs` | VirtualGicd — IROUTER shadow, SPI routing |
| `src/devices/gic/redistributor.rs` | VirtualGicr — per-vCPU GICR state emulation |
| `src/arch/aarch64/vcpu_arch_state.rs` | ICH_LR/VMCR/HCR save/restore |
| `src/arch/aarch64/hypervisor/exception.rs` | handle_sgi_trap, handle_irq_exception, flush_pending_spis |
| `src/vm.rs` | inject_pending_sgis/spis, GICR unmap setup |
| `src/global.rs` | PENDING_SGIS, PENDING_SPIS, inject_spi() |
