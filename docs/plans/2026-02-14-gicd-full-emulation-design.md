# GICD Full Trap-and-Emulate

**Date**: 2026-02-14
**Status**: Approved
**Goal**: Fix SPI routing (always returns vCPU 0) + proper guest/hypervisor GICD isolation

## Problem

The GICD at 0x08000000 is mapped as DEVICE passthrough in Stage-2. Guest writes go directly to physical hardware, bypassing VirtualGicd. This means:

1. `VirtualGicd.irouter[]` is never populated — `route_spi()` always returns 0
2. Guest can see/modify physical GIC state — no isolation
3. SPI affinity changes by the guest (e.g., `/proc/irq/*/smp_affinity`) have no effect on hypervisor routing

## Approach

Unmap the GICD 64KB region (16 x 4KB pages) in Stage-2, same as GICR0/1/3. Guest accesses trap as Data Aborts and route to the existing VirtualGicd via DeviceManager.

### What changes

- `src/vm.rs` `init_memory_dynamic()`: Add 16 `unmap_4kb_page()` calls for 0x08000000-0x0800FFFF

### What stays the same

- VirtualGicd register emulation (already complete)
- DeviceManager routing (VirtualGicd already registered)
- `route_spi()` / `inject_spi()` logic
- `enable_physical_uart_irq()` (EL2 bypasses Stage-2)
- GICR trap-and-emulate

### Risk

- QEMU external abort bug: GICD pages are L3[0-15], far from GICR2 L3[224-255]. Not affected.
- Performance: ~100-200 GICD accesses at boot, rare after. Negligible.

## Verification

1. `make` builds
2. `make run-linux`: 4 vCPUs, BusyBox shell, no RCU stalls
3. Virtio-blk detected, UART interactive
