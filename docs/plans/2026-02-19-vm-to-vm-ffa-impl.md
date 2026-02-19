# VM-to-VM FF-A Memory Sharing Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable FF-A v1.1 memory sharing between normal world VMs using dynamic Stage-2 page mapping with MEM_RETRIEVE_REQ and MEM_RELINQUISH, forward-compatible with TF-A EL3 SPMC.

**Architecture:** Extend `Stage2Walker` with `map_page()`/`unmap_page()` that allocate L2/L3 tables from heap. MEM_SHARE restricts sender's S2AP. MEM_RETRIEVE_REQ creates new PTE in receiver's Stage-2 (same IPA, identity mapping). MEM_RELINQUISH removes receiver's mapping. MEM_RECLAIM restores sender's access. Cross-VM Stage-2 access via `PER_VM_VTTBR` global.

**Tech Stack:** Rust (no_std), ARM64 Stage-2 page tables, FF-A v1.1 (DEN0077A), `crate::mm::heap::alloc_page()`

---

## 7 Tasks

### Task 1: Add PER_VM_VTTBR Global + Store at Boot

**Files:** `src/global.rs`, `src/vm.rs`

Add `PER_VM_VTTBR: [AtomicU64; MAX_VMS]` to `global.rs`. Store `config.vttbr & PTE_ADDR_MASK` in `vm.rs:setup_linux_memory()` after `self.vttbr = config.vttbr`.

### Task 2: Extend Stage2Walker with map_page() and unmap_page()

**Files:** `src/ffa/stage2_walker.rs`

Add `map_page(ipa, s2ap, sw_bits)` — walks L0→L1→L2→L3, allocates L2/L3 from heap as needed, writes 4KB page PTE with Normal attrs + specified S2AP + SW bits. Add `unmap_page(ipa)` — zeroes the leaf PTE + TLBI.

### Task 3: Extend Receiver Validation and Share Records

**Files:** `src/ffa/mod.rs`, `src/ffa/stub_spmc.rs`

Add `is_valid_receiver()`, `is_vm_partition()`. Remove dead_code from RETRIEVE/RELINQUISH constants. Add `retrieved: bool` to `MemShareRecord`, plus `lookup_share_full()`, `mark_retrieved()`, `mark_relinquished()`.

### Task 4: Implement RETRIEVE + RELINQUISH + VM Receivers

**Files:** `src/ffa/proxy.rs`

Replace `is_valid_sp()` with `is_valid_receiver()` in share handler. Add dispatch for RETRIEVE/RELINQUISH. Implement `handle_mem_retrieve_req()` (uses `map_page` on receiver's Stage-2) and `handle_mem_relinquish()` (uses `unmap_page`). Guard `handle_mem_reclaim()` against active retrieval.

### Task 5: Add VM-to-VM Tests

**Files:** `tests/test_ffa.rs`

9 new test cases: is_valid_receiver, SHARE→VM, RETRIEVE, double-RETRIEVE denied, RELINQUISH, RECLAIM after relinquish, RECLAIM while retrieved denied, RETRIEVE by wrong VM denied, FEATURES support.

### Task 6: Update Documentation

**Files:** `CLAUDE.md`

Update FF-A section, Global State table, test counts.

### Task 7: Integration Test

Verify `make run`, `make run-linux`, `make run-multi-vm`, `make clippy`, `make fmt`.

---

See design doc for full details: `docs/plans/2026-02-19-vm-to-vm-ffa-design.md`
