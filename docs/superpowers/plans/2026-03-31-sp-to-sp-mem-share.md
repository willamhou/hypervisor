# SP-to-SP MEM_SHARE Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable Secure Partitions to share memory with each other via FF-A MEM_SHARE/LEND through the SPMC, with full lifecycle (SHARE->RETRIEVE->RELINQUISH->RECLAIM).

**Architecture:** SP-initiated MEM_SHARE/LEND calls are caught in `handle_sp_exit()` and routed to validation + `record_spmc_share()`. The existing `handle_spmc_mem_retrieve()` and `handle_spmc_mem_relinquish()` already support SP-initiated calls and work without changes. SP-initiated MEM_RECLAIM is added so the sender can clean up. E2E test: NWd orchestrates SP1->SP2 sharing — SP1 shares a page from its own Secure DRAM, SP2 retrieves and reads it.

**Tech Stack:** Rust (no_std), ARM64 assembly (AArch64 GNU AS), FF-A v1.1 (DEN0077A)

**Key Design Decisions:**
- Shared pages live in Secure DRAM (SP's own address space), NOT NS memory
- NWd cannot read Secure memory; verification is via SP DIRECT_RESP return values
- SP1 uses a fixed page `0x0e3F0000` (within SP1's 1MB region) for sharing
- Register-based protocol only (x3=IPA, x4=page_count, x5=receiver_id)

---

## File Structure

| File | Responsibility |
|------|----------------|
| `src/spmc_handler.rs` | Add MEM_SHARE/LEND/RECLAIM to `handle_sp_exit()` whitelist + dispatch; add to `dispatch_ffa_as_sp()` for unit tests |
| `tests/test_spmc_handler.rs` | Unit tests for SP-to-SP MEM_SHARE lifecycle (17 new assertions) |
| `tfa/sp_hello/start.S` | New "share_test" command (x3=0xABCD0002): SP1 writes magic to own page, MEM_SHARE with receiver, returns handle |
| `tfa/sp_irq/start.S` | New "retrieve_test" command (x3=0xABCD0003): SP2 retrieves shared page, reads value, writes new value, relinquishes, returns read value |
| `tfa/bl33_ffa_test/start.S` | BL33 Test 19: SP-to-SP MEM_SHARE E2E — verifies SP2 reads SP1's data via DIRECT_RESP |

---

### Task 1: Add MEM_SHARE/LEND/RECLAIM to `handle_sp_exit()` + `dispatch_ffa_as_sp()`

**Files:**
- Modify: `src/spmc_handler.rs:910-917` (whitelist), `src/spmc_handler.rs:978` (match arm), `src/spmc_handler.rs:1446-1476` (dispatch_ffa_as_sp)

- [ ] **Step 1: Add MEM_SHARE/LEND/RECLAIM to the `handle_sp_exit()` SMC whitelist**

In `src/spmc_handler.rs`, the whitelist at line ~910. Add after `FFA_MEM_RELINQUISH`:

```rust
            && x0 != ffa::FFA_MEM_SHARE_32
            && x0 != ffa::FFA_MEM_SHARE_64
            && x0 != ffa::FFA_MEM_LEND_32
            && x0 != ffa::FFA_MEM_LEND_64
            && x0 != ffa::FFA_MEM_RECLAIM
```

- [ ] **Step 2: Add MEM_SHARE/LEND match arm in `handle_sp_exit()` dispatch**

Insert before the `ffa::FFA_MEM_FRAG_RX` match arm (line ~979):

```rust
            ffa::FFA_MEM_SHARE_32 | ffa::FFA_MEM_SHARE_64
            | ffa::FFA_MEM_LEND_32 | ffa::FFA_MEM_LEND_64 => {
                let is_lend = (x0 == ffa::FFA_MEM_LEND_32 || x0 == ffa::FFA_MEM_LEND_64);
                let sender_id = ((x1 >> 16) & 0xFFFF) as u16;
                let receiver_id = (x5 & 0xFFFF) as u16;
                if sender_id != sp_id {
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                } else if receiver_id == sp_id {
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                } else if !crate::sp_context::is_registered_sp(receiver_id) {
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                } else {
                    let ipa = x3;
                    let page_count = x4 as u32;
                    if page_count == 0 || page_count > 65536 || (ipa & 0xFFF) != 0 {
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                    } else if ipa.checked_add(page_count as u64 * PAGE_SIZE_4KB).is_none() {
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                    } else {
                        let ranges = [(ipa, page_count)];
                        match record_spmc_share(sender_id, receiver_id, &ranges, is_lend) {
                            Some(handle) => {
                                sp.set_args(ffa::FFA_SUCCESS_32, 0, handle & 0xFFFF_FFFF, handle >> 32, 0, 0, 0, 0);
                            }
                            None => {
                                sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_NO_MEMORY as u64, 0, 0, 0, 0, 0);
                            }
                        }
                    }
                }
            }
            ffa::FFA_MEM_RECLAIM => {
                let handle = (x1 & 0xFFFF_FFFF) | (x2 << 32);
                match lookup_spmc_share(handle) {
                    Some((sender, _, _, _, _, _)) if sender == sp_id => {
                        match reclaim_spmc_share(handle) {
                            Ok(()) => sp.set_args(ffa::FFA_SUCCESS_32, 0, 0, 0, 0, 0, 0, 0),
                            Err(code) => sp.set_args(ffa::FFA_ERROR, 0, code as u64, 0, 0, 0, 0, 0),
                        }
                    }
                    Some(_) => sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_DENIED as u64, 0, 0, 0, 0, 0),
                    None => sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0),
                }
            }
```

- [ ] **Step 3: Add MEM_SHARE/LEND/RECLAIM to `dispatch_ffa_as_sp()` for unit tests**

In `dispatch_ffa_as_sp()` (line ~1446), add new match arms before `_ => dispatch_ffa(req)`:

```rust
        ffa::FFA_MEM_SHARE_32 | ffa::FFA_MEM_SHARE_64
        | ffa::FFA_MEM_LEND_32 | ffa::FFA_MEM_LEND_64 => {
            let is_lend = (req.x0 == ffa::FFA_MEM_LEND_32 || req.x0 == ffa::FFA_MEM_LEND_64);
            let sender_from_req = ((req.x1 >> 16) & 0xFFFF) as u16;
            let receiver_id = (req.x5 & 0xFFFF) as u16;
            if sender_from_req != sp_id { return make_error(ffa::FFA_INVALID_PARAMETERS as u64); }
            if receiver_id == sp_id { return make_error(ffa::FFA_INVALID_PARAMETERS as u64); }
            if !crate::sp_context::is_registered_sp(receiver_id) { return make_error(ffa::FFA_INVALID_PARAMETERS as u64); }
            let ipa = req.x3;
            let page_count = req.x4 as u32;
            if page_count == 0 || page_count > 65536 || (ipa & 0xFFF) != 0 { return make_error(ffa::FFA_INVALID_PARAMETERS as u64); }
            if ipa.checked_add(page_count as u64 * PAGE_SIZE_4KB).is_none() { return make_error(ffa::FFA_INVALID_PARAMETERS as u64); }
            let ranges = [(ipa, page_count)];
            match record_spmc_share(sp_id, receiver_id, &ranges, is_lend) {
                Some(handle) => SmcResult8 { x0: ffa::FFA_SUCCESS_32, x1: 0, x2: handle & 0xFFFF_FFFF, x3: handle >> 32, x4: 0, x5: 0, x6: 0, x7: 0 },
                None => make_error(ffa::FFA_NO_MEMORY as u64),
            }
        }
        ffa::FFA_MEM_RECLAIM => {
            let handle = (req.x1 & 0xFFFF_FFFF) | (req.x2 << 32);
            match lookup_spmc_share(handle) {
                Some((sender, _, _, _, _, _)) if sender == sp_id => {
                    match reclaim_spmc_share(handle) { Ok(()) => make_success(), Err(code) => make_error(code as u64) }
                }
                Some(_) => make_error(ffa::FFA_DENIED as u64),
                None => make_error(ffa::FFA_INVALID_PARAMETERS as u64),
            }
        }
```

- [ ] **Step 4: Verify build**: `make check 2>&1 | tail -5`
- [ ] **Step 5: Commit**: `git commit -m "feat: SP-initiated MEM_SHARE/LEND/RECLAIM in handle_sp_exit + dispatch_ffa_as_sp"`

---

### Task 2: Unit tests for SP-to-SP MEM_SHARE lifecycle

**Files:**
- Modify: `tests/test_spmc_handler.rs`

- [ ] **Step 1: Add 17 test assertions** (SPMEM1-SPMEM16) at end of `test_spmc_handler()`:
  - SPMEM1-4: Full lifecycle SP1->SP2 (SHARE, RETRIEVE, RELINQUISH, RECLAIM)
  - SPMEM5: Self-sharing blocked
  - SPMEM6: Non-existent receiver blocked
  - SPMEM7: Source spoofing blocked
  - SPMEM8-10: Cross-SP RECLAIM blocked + cleanup (3 assertions)
  - SPMEM11-15: MEM_LEND lifecycle (LEND, RETRIEVE, RECLAIM-while-retrieved, RELINQUISH, RECLAIM)
  - SPMEM16: Zero page count blocked
  - SPMEM17: Misaligned IPA blocked

- [ ] **Step 2: Update assertion count** to 168 (151 + 17)
- [ ] **Step 3: Run tests**: `make run 2>&1 | grep -E "spmc_handler|assertions|panic"`
- [ ] **Step 4: Commit**: `git commit -m "test: SP-to-SP MEM_SHARE/LEND/RECLAIM unit tests (17 new assertions)"`

---

### Task 3: SP Hello "share_test" command

**Files:**
- Modify: `tfa/sp_hello/start.S`

- [ ] **Step 1: Add constants**

```asm
.equ FFA_MEM_SHARE_32,          0x84000073
.equ FFA_MEM_RECLAIM,           0x84000077
.equ SP_TO_SP_SHARE_MAGIC,      0xABCD0002
.equ SP1_PARTITION_ID,          0x8001
.equ SP1_SHARE_PAGE,            0x0e3F0000   /* within SP1's 1MB region */
.equ SP1_SHARE_WRITTEN,         0xFACE0001
```

- [ ] **Step 2: Add dispatch check** in `.Lmsg_loop` (before fast path):

```asm
    ldr     w8, =SP_TO_SP_SHARE_MAGIC
    cmp     w3, w8
    b.eq    .Lshare_test
```

- [ ] **Step 3: Implement `.Lshare_test` handler**

Flow: write 0xFACE0001 to SP1_SHARE_PAGE -> MEM_SHARE(receiver=x7, ipa=SP1_SHARE_PAGE, pages=1) -> return DIRECT_RESP(x4=handle_lo, x5=handle_hi, x6=SP1_SHARE_PAGE)

- [ ] **Step 4: Build**: `make build-sp-hello 2>&1 | tail -3`
- [ ] **Step 5: Commit**: `git commit -m "feat: SP Hello share_test command for SP-to-SP MEM_SHARE"`

---

### Task 4: SP IRQ "retrieve_test" command

**Files:**
- Modify: `tfa/sp_irq/start.S`

- [ ] **Step 1: Add constants**

```asm
.equ FFA_MEM_RETRIEVE_REQ_32,   0x84000074
.equ FFA_MEM_RETRIEVE_RESP,     0x84000075
.equ FFA_MEM_RELINQUISH,        0x84000076
.equ SP_RETRIEVE_MAGIC,         0xABCD0003
.equ SP2_SHARE_WRITTEN,         0xFACE0002
```

- [ ] **Step 2: Add dispatch check** in SP2 message loop
- [ ] **Step 3: Implement `.Lretrieve_test` handler**

Flow: MEM_RETRIEVE(handle) -> read page (save old value in x28) -> write 0xFACE0002 -> MEM_RELINQUISH(handle) -> return DIRECT_RESP(x4=handle_lo+0x5000, x5=old_value_read)

Key: x5 returns what SP2 read from the page (should be 0xFACE0001 written by SP1)

- [ ] **Step 4: Build**: `make build-sp-irq 2>&1 | tail -3`
- [ ] **Step 5: Commit**: `git commit -m "feat: SP IRQ retrieve_test command for SP-to-SP MEM_SHARE"`

---

### Task 5: BL33 Test 19 — SP-to-SP MEM_SHARE E2E

**Files:**
- Modify: `tfa/bl33_ffa_test/start.S`

- [ ] **Step 1: Add constants and Test 19**

After `.Ltest18_done`, before `.Ldone`:

```
Test 19 flow:
1. NWd -> SP1 DIRECT_REQ(x3=SP_TO_SP_SHARE_MAGIC, x7=SP2_ID)
   SP1 writes 0xFACE0001 to 0x0e3F0000
   SP1 MEM_SHARE(SP2, 0x0e3F0000, 1 page) -> handle
   SP1 returns DIRECT_RESP(x3=echo, x4=handle_lo, x5=handle_hi, x6=page_ipa)
   NWd verifies: got DIRECT_RESP, handle non-zero

2. NWd -> SP2 DIRECT_REQ(x3=SP_RETRIEVE_MAGIC, x4=handle_lo, x5=handle_hi, x6=page_ipa)
   SP2 MEM_RETRIEVE -> page mapped in SP2's Stage-2
   SP2 reads page -> gets 0xFACE0001 (SP1's data!)
   SP2 writes 0xFACE0002
   SP2 MEM_RELINQUISH
   SP2 returns DIRECT_RESP(x4=handle_lo+0x5000, x5=value_read)
   NWd verifies: x4 == handle_lo+0x5000, x5 == 0xFACE0001
```

- [ ] **Step 2: Add string** `str_t19: .asciz "  Test 19: SP-to-SP MEM_SHARE .... "`
- [ ] **Step 3: Commit**: `git commit -m "feat: BL33 Test 19 — SP-to-SP MEM_SHARE E2E"`

---

### Task 6: E2E verification

- [ ] **Step 1: Build**: `make build-tfa-spmc 2>&1 | tail -10`
- [ ] **Step 2: Run**: `timeout 120 make run-spmc 2>&1 | grep -E "Test|PASS|FAIL"`
  Expected: 19/19 PASS
- [ ] **Step 3: Run unit tests**: `make run 2>&1 | grep -E "spmc_handler|assertions|panic"`
  Expected: 168 assertions passed

---

### Task 7: Update CLAUDE.md

- [ ] **Step 1: Update** SpmcHandler description, test counts (168 assertions, 19/19 BL33), Phase 5.1 roadmap
- [ ] **Step 2: Commit**: `git commit -m "docs: update CLAUDE.md for SP-to-SP MEM_SHARE"`
