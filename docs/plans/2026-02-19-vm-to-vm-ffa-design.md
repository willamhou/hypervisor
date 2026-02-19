# VM-to-VM FF-A Memory Sharing Design

**Date:** 2026-02-19
**Status:** Approved (v2 — Approach A')
**Sprint:** M3 Sprint 3.2

## Goal

Enable FF-A v1.1 memory sharing between normal world VMs (VM-to-VM), extending the existing VM-to-SP proxy to support `FFA_MEM_RETRIEVE_REQ`, `FFA_MEM_RELINQUISH`, and cross-VM Stage-2 page table manipulation. Designed for forward compatibility with TF-A EL3 SPMC integration (Sprint 3.3). Direct messaging remains stub-only (echo).

## Architecture

### Dynamic Page Mapping (Approach A')

Extend `Stage2Walker` with `map_page()` and `unmap_page()` methods that allocate intermediate page tables from the heap. When a receiver calls `FFA_MEM_RETRIEVE_REQ`, the hypervisor creates a new 4KB PTE entry in the receiver's Stage-2 at the same IPA (identity mapping: GPA==HPA). No fixed shared memory window needed.

**Advantages over fixed window:**
- Any IPA range can be shared (not limited to a pre-allocated region)
- Compatible with TF-A SPMC protocol (SP expects arbitrary IPAs)
- No wasted memory for unused shared regions
- Same-IPA identity mapping simplifies address translation

### TF-A SPMC Forward Compatibility

| Aspect | Sprint 3.2 (VM-to-VM) | Sprint 3.3 (TF-A) |
|--------|----------------------|-------------------|
| VM-to-VM | Local: hypervisor manages both Stage-2s | Same (hypervisor is NWd partition manager) |
| VM-to-SP | Stub SPMC echo | Forward to EL3 SPMC |
| Handle namespace | `HANDLE_VM_BIT (1<<63)` marks VM-to-VM | EL3 SPMC manages SP handles |
| RETRIEVE protocol | Register-based (handle in x1/x2) | Add descriptor support in TX buffer |

### FF-A Protocol Flow

```
VM 0 (sender)                    Hypervisor                      VM 1 (receiver)
    |                                |                                |
    |-- FFA_MEM_SHARE ------------->|                                |
    |   IPA=0x50000000, 1 page      | Validate Owned in VM0 Stage-2  |
    |   receiver=VM1 (part_id=2)    | VM0: SharedOwned + S2AP_RO     |
    |                                | Record share(h, sender=1,recv=2)|
    |<-- SUCCESS(handle=H) ---------|                                |
    |                                |                                |
    |  [VM0 sends handle to VM1 via direct msg or shared memory]     |
    |                                |                                |
    |                                |<-- FFA_MEM_RETRIEVE_REQ(h=H) --|
    |                                |   Validate handle, recv==VM1   |
    |                                |   map_page(0x50000000) in VM1  |
    |                                |   VM1 PTE: S2AP_RW, SharedBorrowed |
    |                                |-- FFA_MEM_RETRIEVE_RESP ------>|
    |                                |                                |
    |                                |  [VM1 reads/writes 0x50000000] |
    |                                |                                |
    |                                |<-- FFA_MEM_RELINQUISH(h=H) ---|
    |                                |   unmap_page(0x50000000) in VM1|
    |                                |   Mark share as not-retrieved   |
    |                                |-- SUCCESS -------------------->|
    |                                |                                |
    |-- FFA_MEM_RECLAIM(handle=H) ->|                                |
    |                                |   Verify share not retrieved    |
    |                                |   VM0: Owned + S2AP_RW         |
    |                                |   Delete share record           |
    |<-- SUCCESS -------------------|                                |
```

### S2AP State Transitions

| Operation | Sender S2AP | Sender SW bits | Receiver S2AP | Receiver PTE |
|-----------|-------------|----------------|---------------|--------------|
| SHARE     | RO          | SharedOwned    | (no entry)    | (no entry)   |
| LEND      | NONE        | SharedOwned    | (no entry)    | (no entry)   |
| RETRIEVE  | (unchanged) | (unchanged)    | RW (new entry)| SharedBorrowed |
| RELINQUISH| (unchanged) | (unchanged)    | (entry removed)| (entry removed) |
| RECLAIM   | RW          | Owned          | (unchanged)   | (unchanged)  |

### Cross-VM Stage-2 Access

`PER_VM_VTTBR: [AtomicU64; MAX_VMS]` stores each VM's VTTBR L0 PA at boot time. `Stage2Walker::new(per_vm_vttbr[target_vm])` constructs a walker for any VM's page table without that VM being active.

### Stage2Walker Extensions

```rust
impl Stage2Walker {
    /// Map a 4KB page in this VM's Stage-2 (identity mapping: IPA == PA).
    /// Allocates L2/L3 intermediate tables from heap as needed.
    pub fn map_page(&self, ipa: u64, s2ap: u8, sw_bits: u8) -> Result<(), &'static str> {
        // Walk L0 → L1 (must exist) → get_or_create L2 → get_or_create L3
        // Write page entry with specified S2AP + SW bits + TLB invalidate
    }

    /// Remove a 4KB page mapping from this VM's Stage-2.
    /// Zeroes the L3 PTE entry + TLB invalidation.
    pub fn unmap_page(&self, ipa: u64) -> Result<(), &'static str> {
        // Walk to L3 entry, zero it, TLB invalidate
    }
}
```

Key design points:
- Uses `crate::mm::heap::alloc_page()` for L2/L3 table allocation
- L0→L1 link always exists (created by `DynamicIdentityMapper::new()`)
- L2 tables may not exist for IPAs outside the original VM's range — `map_page()` creates them
- Break-before-make not needed for new entries (writing to previously-invalid PTE)
- `unmap_page()` only needs to zero the L3 entry + TLBI (no deallocation — leaked tables are fine)

## File Changes

### `src/ffa/stage2_walker.rs` — Add `map_page()` + `unmap_page()`

New methods on `Stage2Walker`. ~60 lines of page table manipulation code.

### `src/global.rs` — Add `PER_VM_VTTBR`

```rust
pub static PER_VM_VTTBR: [AtomicU64; MAX_VMS] = [AtomicU64::new(0), AtomicU64::new(0)];
```

### `src/vm.rs` — Store VTTBR at boot

After `self.vttbr = config.vttbr` in `setup_linux_memory()`:
```rust
crate::global::PER_VM_VTTBR[self.id].store(config.vttbr & PTE_ADDR_MASK, Ordering::Release);
```

### `src/ffa/mod.rs` — Receiver validation

- `is_valid_receiver()`: accepts VM partition IDs + SP IDs
- `is_vm_partition()`: checks if partition ID is a VM
- Remove `#[allow(dead_code)]` from `partition_id_to_vm_id`, `FFA_MEM_RETRIEVE_*`, `FFA_MEM_RELINQUISH`

### `src/ffa/stub_spmc.rs` — Extended share records

- `retrieved: bool` field in `MemShareRecord`
- `ShareInfoFull` struct (includes sender_id, receiver_id, retrieved)
- `lookup_share_full()`, `mark_retrieved()`, `mark_relinquished()`

### `src/ffa/proxy.rs` — New handlers + modified dispatch

- Dispatch `FFA_MEM_RETRIEVE_REQ_32/64` → `handle_mem_retrieve_req()`
- Dispatch `FFA_MEM_RELINQUISH` → `handle_mem_relinquish()`
- `handle_mem_share_or_lend()`: accept VM receivers via `is_valid_receiver()`
- `handle_mem_reclaim()`: block if share is still retrieved
- `handle_features()`: add RETRIEVE/RELINQUISH to supported list

### `tests/test_ffa.rs` — 9 new test cases

| Test | Description |
|------|-------------|
| 19 | `is_valid_receiver` accepts VMs and SPs |
| 20 | MEM_SHARE to VM1 returns handle |
| 21 | MEM_RETRIEVE_REQ by VM1 succeeds |
| 22 | Double RETRIEVE denied |
| 23 | MEM_RELINQUISH by VM1 succeeds |
| 24 | MEM_RECLAIM after RELINQUISH succeeds |
| 25 | RECLAIM while retrieved → DENIED |
| 26 | RETRIEVE by wrong VM → DENIED |
| 27 | FEATURES reports RETRIEVE/RELINQUISH supported |

## Constraints

- **Single receiver**: Each share has exactly one receiver (no multi-cast)
- **Identity mapping only**: Receiver maps shared pages at same IPA as sender
- **No fragmentation**: FFA_MEM_FRAG_RX/TX not implemented
- **No descriptor-based RETRIEVE**: Uses register-based protocol (Sprint 3.3 adds descriptors)
- **Direct messaging**: Remains stub-only (echo). VM-to-VM synchronous messaging deferred.
- **Stage-2 in unit tests**: Tests run without real Stage-2 (VTTBR=0), so `map_page()`/`unmap_page()` are skipped — validation uses `lookup_share_full()` only
