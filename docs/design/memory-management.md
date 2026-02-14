# Memory Management

This document describes the hypervisor's Stage-2 page table implementation and memory layout.

## Memory Layout

| Region | Address | Size | Purpose |
|--------|---------|------|---------|
| Hypervisor code | 0x40000000 | ~1MB | Linker base (`arch/aarch64/linker.ld`) |
| Heap | 0x41000000 | 16MB | Page table allocation, `BumpAllocator` |
| DTB | 0x47000000 | ~64KB | Device tree blob (loaded by QEMU) |
| Kernel | 0x48000000 | ~25MB | Linux Image (loaded by QEMU) |
| Initramfs | 0x54000000 | ~2MB | BusyBox initramfs (loaded by QEMU) |
| Disk image | 0x58000000 | 2MB | Virtio-blk backing store (loaded by QEMU) |
| Guest RAM end | 0x68000000 | | 512MB from 0x48000000 |

## Stage-2 Translation

Stage-2 translates guest IPAs (Intermediate Physical Addresses) to HPAs (Host Physical Addresses). This hypervisor uses **identity mapping** (IPA == HPA), simplifying address translation.

### Page Table Structure (4KB granule)

```
L0 table (1 entry)
  └─ L1 table (512 entries, each covers 1GB)
       └─ L2 entries: 2MB blocks (identity-mapped RAM/DEVICE)
            └─ L3 table (512 entries, 4KB pages — only where needed)
```

- **L0**: Single entry covering the 0x00000000-0x3FFFFFFFFF range
- **L1**: Index = IPA[38:30]. Covers 1GB regions.
- **L2**: Index = IPA[29:21]. Default: 2MB block entries. Splits to L3 table for 4KB granularity (GICR trap setup).
- **L3**: Index = IPA[20:12]. 4KB page entries. Created by `split_2mb_block()`.

### VTCR_EL2 Configuration

- T0SZ = 24 (40-bit IPA space, 1TB)
- SL0 = 1 (start at L1)
- IRGN0/ORGN0 = Write-back cacheable
- SH0 = Inner Shareable
- TG0 = 4KB granule

### Memory Attributes (Stage-2)

```
MemoryAttributes::NORMAL:
  MemAttr[3:0] = 0b1111 (Normal, Write-back)
  S2AP[1:0]    = 0b11   (Read-Write)
  SH[1:0]      = 0b11   (Inner Shareable)
  AF           = 1

MemoryAttributes::DEVICE:
  MemAttr[3:0] = 0b0000 (Device-nGnRnE)
  S2AP[1:0]    = 0b11   (Read-Write)
  SH[1:0]      = 0b00   (Non-shareable)
  AF           = 1
```

## Two Mapper Implementations

### IdentityMapper (Static)

Used by: `make run` (unit tests, no `linux_guest` feature)

- **Storage**: Static arrays on stack/BSS — no heap allocation
- **Granularity**: 2MB blocks only (no 4KB pages)
- **L0/L1/L2 tables**: Fixed `[S2PageTableEntry; 512]` arrays
- **No unmap support**

Simple and sufficient for unit tests where GICR trap-and-emulate isn't needed.

### DynamicIdentityMapper (Heap-allocated)

Used by: `make run-linux` (`linux_guest` feature)

- **Storage**: Heap-allocated via `BumpAllocator` (Box, Vec)
- **Granularity**: 2MB blocks + 4KB pages
- **Supports `unmap_4kb_page()`**: Required for GICR trap setup
- **`split_2mb_block()`**: Converts a 2MB block entry to an L3 table with 512 x 4KB page entries

#### split_2mb_block()

When we need 4KB granularity within a 2MB region:

1. Allocate a new L3 table (4KB-aligned, 512 entries)
2. Fill all 512 entries as 4KB pages mapping the same physical addresses
3. Replace the L2 block entry with a table entry pointing to L3
4. Now individual 4KB pages can be unmapped

#### unmap_4kb_page()

1. Walk L0→L1→L2 to find the 2MB region containing the target address
2. If L2 is a block: call `split_2mb_block()` first
3. Index into L3 table: `(addr >> 12) & 0x1FF`
4. Set L3 entry to invalid (0)
5. TLB invalidate: `TLBI IPAS2E1IS` + `DSB ISH` + `TLBI VMALLE1IS` + `DSB ISH` + `ISB`

Used to unmap 32 pages per GICR (128KB each) for GICR0, GICR1, GICR3 trap-and-emulate.

## Heap Gap Protection

The hypervisor heap (0x41000000, 16MB) lies within the guest's Stage-2 address range (0x40000000-0x68000000). If mapped as Normal memory, the guest could corrupt heap-allocated page tables.

**Solution**: `init_memory_dynamic()` splits guest RAM mapping into two regions, skipping the heap:

```
Map: 0x40000000 → 0x41000000  (before heap)
Gap: 0x41000000 → 0x42000000  (heap, unmapped)
Map: 0x42000000 → 0x68000000  (after heap)
```

The guest kernel never accesses the gap because its declared memory starts at 0x48000000 (from DTB `memory@48000000`).

## BumpAllocator

Located in `src/mm/allocator.rs`.

A simple page allocator for the 16MB heap region:

- **Allocation**: Returns 4KB-aligned pages from a bump pointer
- **Free list**: Freed pages go on a linked list for reuse
- **Thread safety**: Not needed (single pCPU, no concurrency in allocator)

Used by the global allocator (`#[global_allocator]`) for `Box`, `Vec`, etc.

## GIC Region Mapping

The GIC region (0x08000000, 16MB) is mapped as DEVICE in Stage-2:

```rust
mapper.map_region(GIC_REGION_BASE, GIC_REGION_SIZE, MemoryAttribute::Device);
```

Then GICR pages are selectively unmapped for trap-and-emulate:
- GICR0: 32 pages at 0x080A0000
- GICR1: 32 pages at 0x080C0000
- GICR3: 32 pages at 0x08100000
- GICR2: **not unmapped** (QEMU bug workaround)

GICD (0x08000000) stays mapped — VirtualGicd uses shadow state, not full trap.

## UART Region

The UART (0x09000000) is **not mapped** in Stage-2 at all. All guest UART accesses trap as Data Aborts and are handled by `VirtualUart`.

## Stage-2 Installation

After building the page tables:

```rust
let config = mapper.config();  // Returns (VTTBR_EL2, VTCR_EL2)
init_stage2_from_config(&config);
```

`init_stage2_from_config()`:
1. Writes VTTBR_EL2 (page table base address + VMID)
2. Writes VTCR_EL2 (translation control)
3. Sets HCR_EL2.VM=1 (enable Stage-2 translation)
4. ISB + TLB invalidate

The mapper is then `core::mem::forget()`-ed to prevent deallocation of heap-allocated page tables.

## Source Files

| File | Role |
|------|------|
| `src/arch/aarch64/mm/mmu.rs` | `S2PageTableEntry`, `MemoryAttributes`, `PageTable`, `DynamicIdentityMapper`, `IdentityMapper` |
| `src/mm/allocator.rs` | `BumpAllocator` — page allocation for heap |
| `src/mm/heap.rs` | Global allocator setup (`#[global_allocator]`) |
| `src/vm.rs` | `init_memory_dynamic()`, `init_memory_static()` — Stage-2 setup |
| `src/arch/aarch64/defs.rs` | PTE constants, address masks, page sizes |
| `src/platform.rs` | Address constants (HEAP_START, GIC bases, GICR offsets) |
| `arch/aarch64/linker.ld` | Hypervisor memory layout, base address 0x40000000 |
