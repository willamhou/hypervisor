# SPMC Robustness Hardening Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three FF-A v1.1 spec compliance gaps: range count validation, RXTX_UNMAP fragment cleanup, and MEM_LEND E2E testing.

**Architecture:** Validation at storage-layer entry points (`record_spmc_share()`, `record_share()`, `record_share_with_handle()`). Also remove upstream `.min()` truncations so validation reaches the chokepoint. Fragment cleanup on RXTX_UNMAP via existing `reset_nwd_frag_state()`. MEM_LEND BL33 test mirrors Test 13 (MEM_SHARE lifecycle E2E) pattern with `FFA_MEM_LEND_32`.

**Tech Stack:** Rust (no_std), ARM64 assembly, QEMU virt

**Spec:** `docs/superpowers/specs/2026-03-18-spmc-robustness-hardening-design.md`

**Existing coverage note:** `tests/test_spmc_handler.rs` already has T2 (MEM_LEND full lifecycle: FEATURES → LEND → RETRIEVE → RELINQUISH → RECLAIM, lines 490-565). This plan adds missing negative tests and proxy-side tests, not duplicates.

---

## Chunk 1: Range Count Validation

### Task 1: Add range count validation to `record_spmc_share()` + remove upstream truncation

**Files:**
- Modify: `src/spmc_handler.rs:271-300` (`record_spmc_share()`)
- Modify: `src/spmc_handler.rs:1879` (descriptor-based path in `handle_spmc_mem_share()`)
- Modify: `src/spmc_handler.rs:~1770` (fragment completion in `handle_spmc_mem_frag_tx()`)

- [ ] **Step 1: Make `record_spmc_share()` public and add validation**

Change `fn record_spmc_share(` to `pub fn record_spmc_share(` (needed for direct test access).

Add range count guard at the top:

```rust
pub fn record_spmc_share(
    sender_id: u16,
    receiver_id: u16,
    ranges: &[(u64, u32)],
    is_lend: bool,
) -> Option<u64> {
    if ranges.len() > MAX_SHARE_RANGES {
        return None;
    }
    // ... rest unchanged, but change:
    //   let count = ranges.len().min(MAX_SHARE_RANGES);
    // to:
    //   let count = ranges.len();
    // (guard above makes .min() redundant)
```

- [ ] **Step 2: Remove `.min()` in `handle_spmc_mem_share()` descriptor path**

At line 1879, change:
```rust
// OLD:
let count = desc.range_count.min(MAX_SHARE_RANGES);
// NEW:
let count = desc.range_count;
```

This ensures descriptors with `range_count > 4` are passed through to `record_spmc_share()` which rejects them, instead of being silently truncated before reaching the chokepoint.

Also add an early check right after `desc.range_count == 0` check (line 1876-1878):
```rust
if desc.range_count == 0 {
    return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
}
if desc.range_count > MAX_SHARE_RANGES {
    return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
}
```

- [ ] **Step 3: Add guard to `handle_spmc_mem_frag_tx()` inline record creation**

At the fragment completion path (~line 1770), the inline record creation also truncates with `.min(MAX_SHARE_RANGES)`. Add the same guard before the inline loop:

```rust
if desc.range_count > MAX_SHARE_RANGES {
    // Reset frag state before returning error
    frag.active = false;
    return make_error(ffa::FFA_INVALID_PARAMETERS as u64);
}
let count = desc.range_count; // no longer needs .min()
```

Note: Full refactoring to call `record_spmc_share()` from `handle_spmc_mem_frag_tx()` is deferred — the pre-assigned `frag_handle` makes this non-trivial. The inline guard provides equivalent protection.

- [ ] **Step 4: Verify build**

Run: `make check`
Expected: Compiles without errors.

- [ ] **Step 5: Commit**

```bash
git add src/spmc_handler.rs
git commit -m "feat: add range count validation to SPMC share paths"
```

### Task 2: Add range count validation to stub SPMC

**Files:**
- Modify: `src/ffa/stub_spmc.rs:90-155` (`record_share()`, `record_share_with_handle()`)

- [ ] **Step 1: Add validation to `record_share()`**

At the top of `record_share()` (line ~92), before the loop, add:

```rust
if ranges.len() > MAX_SHARE_RANGES {
    return None;
}
```

Then change `let count = ranges.len().min(MAX_SHARE_RANGES);` to `let count = ranges.len();`.

- [ ] **Step 2: Add validation to `record_share_with_handle()`**

At the top of `record_share_with_handle()` (line ~133), add:

```rust
if ranges.len() > MAX_SHARE_RANGES {
    return false;
}
```

Then change `let count = ranges.len().min(MAX_SHARE_RANGES);` to `let count = ranges.len();`.

- [ ] **Step 3: Verify build**

Run: `make check`
Expected: Compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src/ffa/stub_spmc.rs
git commit -m "feat: add range count validation to stub SPMC share records"
```

### Task 3: Add range count validation to proxy

**Files:**
- Modify: `src/ffa/proxy.rs:614-650` (in `handle_mem_share_or_lend()`, after `parse_share_descriptor()` returns)

- [ ] **Step 1: Add post-parse check in the caller**

In `handle_mem_share_or_lend()` (proxy.rs:614), after `parse_share_descriptor()` returns `Ok(info)` at line 622, add a range count check before the tuple is destructured and passed to `finalize_mem_share()`:

```rust
match parse_share_descriptor(context, mbox, is_lend) {
    Ok(info) => {
        // Validate range count fits storage (MAX_SHARE_RANGES = 4)
        if info.3 > stub_spmc::MAX_SHARE_RANGES {
            ffa_error(context, FFA_INVALID_PARAMETERS);
            return true;
        }
        info
    },
    // ... rest unchanged
}
```

Where `info.3` is `range_count` (4th element of the tuple). Or destructure first:
```rust
Ok((sender_id, receiver_id, ranges, range_count, total_page_count)) => {
    if range_count > stub_spmc::MAX_SHARE_RANGES {
        ffa_error(context, FFA_INVALID_PARAMETERS);
        return true;
    }
    (sender_id, receiver_id, ranges, range_count, total_page_count)
},
```

- [ ] **Step 2: Verify build**

Run: `make check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/ffa/proxy.rs
git commit -m "feat: add range count validation to proxy share path"
```

### Task 4: Add range overflow unit tests

**Files:**
- Modify: `tests/test_spmc_handler.rs`

- [ ] **Step 1: Add SPMC range overflow test via `record_spmc_share()` direct call**

After the existing T2 MEM_LEND tests (around line 565), add:

```rust
// ── Range count overflow → None (record_spmc_share rejects > MAX_SHARE_RANGES) ──
{
    let too_many: [(u64, u32); 5] = [
        (0xA000_0000, 1), (0xA000_1000, 1), (0xA000_2000, 1),
        (0xA000_3000, 1), (0xA000_4000, 1),
    ];
    let result = crate::spmc_handler::record_spmc_share(0x0001, 0x8001, &too_many, false);
    assert!(result.is_none());
    pass += 1;
}
```

This works because Task 1 Step 1 made `record_spmc_share` `pub`.

- [ ] **Step 2: Run tests**

Run: `make run`
Expected: All 34 suites pass, test_spmc_handler assertion count increases by 1.

- [ ] **Step 3: Commit**

```bash
git add tests/test_spmc_handler.rs
git commit -m "test: add range count overflow validation test"
```

## Chunk 2: RXTX_UNMAP Fragment Cleanup

### Task 5: Add fragment cleanup to SPMC `handle_rxtx_unmap()`

**Files:**
- Modify: `src/spmc_handler.rs:1544-1564` (`handle_rxtx_unmap()`)

- [ ] **Step 1: Add cleanup calls**

After line 1552 (`nwd.mapped = false;`), before constructing the success response:

```rust
    nwd.mapped = false;

    // Clean up any in-flight fragment state (FF-A spec: RXTX_UNMAP invalidates transfers)
    drop(nwd); // release NWD_RXTX lock before acquiring NWD_FRAG lock
    reset_nwd_frag_state(); // defined at line 208, first production caller
    {
        let mut frag_rx = NWD_FRAG_RX.lock();
        frag_rx.active = false;
    }

    SmcResult8 { ... }
```

Important: `drop(nwd)` before acquiring `NWD_FRAG` to avoid lock ordering issues.

- [ ] **Step 2: Verify build**

Run: `make check`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/spmc_handler.rs
git commit -m "fix: clean up fragment state on RXTX_UNMAP (spec compliance)"
```

### Task 6: Add UNMAP fragment cleanup test

**Files:**
- Modify: `tests/test_spmc_handler.rs`

- [ ] **Step 1: Write the test**

The test must **explicitly activate fragment state** before UNMAP to verify cleanup.
Register-based MEM_SHARE does NOT set fragment state, so we must set it directly:

```rust
// ── RXTX_UNMAP clears fragment state ──
{
    // Step 1: RXTX_MAP
    let mut req = zero_req(ffa::FFA_RXTX_MAP);
    req.x1 = 0x4200_0000;
    req.x2 = 0x4200_1000;
    req.x3 = 1;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);

    // Step 2: Manually activate NWD_FRAG to simulate in-flight fragment transfer
    {
        use crate::spmc_handler::NWD_FRAG;
        let mut frag = NWD_FRAG.lock();
        frag.active = true;
        frag.handle = 0xDEAD;
        frag.total_length = 4096;
        frag.received = 1024;
    }

    // Step 3: RXTX_UNMAP — should clear fragment state
    let resp = dispatch_ffa(&zero_req(ffa::FFA_RXTX_UNMAP));
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);

    // Step 4: Verify fragment state was cleared
    {
        use crate::spmc_handler::NWD_FRAG;
        let frag = NWD_FRAG.lock();
        assert!(!frag.active);
    }
    pass += 1;

    // Step 5: RXTX_MAP again + MEM_SHARE should succeed (no leftover FFA_BUSY)
    let mut req = zero_req(ffa::FFA_RXTX_MAP);
    req.x1 = 0x4200_0000;
    req.x2 = 0x4200_1000;
    req.x3 = 1;
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
}
```

Note: `NWD_FRAG` must be accessible from tests. Check if it's `pub` — if not, make it `pub(crate)` in spmc_handler.rs.

- [ ] **Step 2: Run tests**

Run: `make run`
Expected: All 34 suites pass, assertion count increases.

- [ ] **Step 3: Commit**

```bash
git add tests/test_spmc_handler.rs
git commit -m "test: verify RXTX_UNMAP clears fragment state"
```

## Chunk 3: MEM_LEND Negative Tests + Proxy Tests

### Task 7: Add missing MEM_LEND negative tests to SPMC

**Files:**
- Modify: `tests/test_spmc_handler.rs`

Existing T2 covers the happy path (LEND → RETRIEVE → RELINQUISH → RECLAIM). Add negative cases:

- [ ] **Step 1: Add LEND double-RETRIEVE test**

After existing T2 (around line 565), add:

```rust
// ── LEND: double RETRIEVE → DENIED ──
{
    // LEND a page
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_LEND_32,
        x1: 0x0001 << 16,
        x2: 0,
        x3: 0xD000_0000,
        x4: 1,
        x5: 0x8001,
        x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    let h = resp.x2 | (resp.x3 << 32);

    // First RETRIEVE → ok
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
        x1: h & 0xFFFF_FFFF,
        x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);

    // Second RETRIEVE → DENIED
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    pass += 1;

    // Cleanup: RELINQUISH + RECLAIM
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RELINQUISH,
        x1: h & 0xFFFF_FFFF, x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    dispatch_ffa(&req);
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RECLAIM,
        x1: h & 0xFFFF_FFFF, x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    dispatch_ffa(&req);
}
```

- [ ] **Step 2: Add LEND RECLAIM-while-retrieved test**

```rust
// ── LEND: RECLAIM without RELINQUISH → DENIED ──
{
    // LEND a page
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_LEND_32,
        x1: 0x0001 << 16,
        x2: 0,
        x3: 0xD100_0000,
        x4: 1,
        x5: 0x8001,
        x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_SUCCESS_32);
    let h = resp.x2 | (resp.x3 << 32);

    // RETRIEVE → ok
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RETRIEVE_REQ_32,
        x1: h & 0xFFFF_FFFF, x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_MEM_RETRIEVE_RESP);

    // RECLAIM (without RELINQUISH) → DENIED
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RECLAIM,
        x1: h & 0xFFFF_FFFF, x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    let resp = dispatch_ffa(&req);
    assert_eq!(resp.x0, ffa::FFA_ERROR);
    pass += 1;

    // Cleanup: RELINQUISH then RECLAIM
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RELINQUISH,
        x1: h & 0xFFFF_FFFF, x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    dispatch_ffa(&req);
    let req = SmcResult8 {
        x0: ffa::FFA_MEM_RECLAIM,
        x1: h & 0xFFFF_FFFF, x2: h >> 32,
        x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
    };
    dispatch_ffa(&req);
}
```

- [ ] **Step 3: Run tests**

Run: `make run`
Expected: All 34 suites pass, test_spmc_handler assertion count increases by 2.

- [ ] **Step 4: Commit**

```bash
git add tests/test_spmc_handler.rs
git commit -m "test: add MEM_LEND negative tests (double RETRIEVE, RECLAIM while retrieved)"
```

### Task 8: Add proxy-side MEM_LEND test

**Files:**
- Modify: `tests/test_ffa.rs`

- [ ] **Step 1: Add LEND_BASIC test**

Find the existing MEM_SHARE test in `test_ffa.rs` (search for `FFA_MEM_SHARE_32`). Add a parallel MEM_LEND test using the same pattern:

```rust
// MEM_LEND basic — register-based
{
    // Adapt to match existing test pattern (make_context / handle_ffa_call / assert)
    // Use FFA_MEM_LEND_32 instead of FFA_MEM_SHARE_32
    // Use different IPA (e.g., 0xD000_0000) to avoid collision
    // x4=1 page, x5=0x8001 receiver
    // Assert: x0 == FFA_SUCCESS_32
    pass += 1;
}
```

Read the existing MEM_SHARE test pattern first and mirror it exactly.

- [ ] **Step 2: Run tests**

Run: `make run`
Expected: All 34 suites pass, test_ffa assertion count increases.

- [ ] **Step 3: Commit**

```bash
git add tests/test_ffa.rs
git commit -m "test: add proxy MEM_LEND unit test"
```

## Chunk 4: BL33 Test 16 (MEM_LEND E2E)

### Task 9: Add BL33 Test 16

**Files:**
- Modify: `tfa/bl33_ffa_test/start.S`

- [ ] **Step 1: Add `FFA_MEM_LEND_32` constant**

After `.equ FFA_MEM_SHARE_32, 0x84000073` (line 32), add:

```asm
.equ FFA_MEM_LEND_32,     0x84000072
```

- [ ] **Step 2: Add test string**

After `str_t15:` (line 898), before `str_pass:`, add:

```asm
str_t16:
    .asciz "  Test 16: MEM_LEND lifecycle E2E .. "
```

- [ ] **Step 3: Write Test 16 body**

Change `b .Ldone` at end of Test 15 pass path (line 837) to `b .Ltest_16`.
Insert before `.Ldone:` (line 842):

```asm
    /* ============ Test 16: MEM_LEND lifecycle E2E ============ */
.Ltest_16:
    adr     x0, str_t16
    bl      uart_print

    /* Clear _tx_page before test */
    adr     x8, _tx_page
    str     wzr, [x8]

    /* Step 1: MEM_LEND _tx_page with SP1 — get handle */
    ldr     x0, =FFA_MEM_LEND_32
    movz    x1, #0x0000
    movk    x1, #0x0001, lsl #16   /* sender = 0x0001 */
    mov     x2, xzr
    adr     x3, _tx_page           /* IPA of lent page */
    mov     x4, #1                 /* 1 page */
    movz    x5, #SP1_ID            /* receiver = SP1 */
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* Verify: x0 == FFA_SUCCESS_32 */
    ldr     x9, =FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    .Lfail_16

    mov     x24, x2                /* handle_lo */
    mov     x25, x3                /* handle_hi */

    /* Step 2: DIRECT_REQ to SP1 with MEM_TEST_MAGIC.
     * SP1 handler: RETRIEVE → write SHARED_PAGE_MAGIC → RELINQUISH → respond.
     * Same handler for both SHARE and LEND — SP doesn't distinguish. */
    ldr     x0, =FFA_DIRECT_REQ_32
    movz    x1, #SP1_ID
    movk    x1, #0x0001, lsl #16
    mov     x2, xzr
    ldr     x3, =MEM_TEST_MAGIC
    mov     x4, x24
    mov     x5, x25
    adr     x6, _tx_page
    mov     x7, xzr
    smc     #0

    ldr     x9, =FFA_DIRECT_RESP_32
    cmp     x0, x9
    b.ne    .Lfail_16

    add     x9, x24, #0x4000
    cmp     x4, x9
    b.ne    .Lfail_16

    ldr     x9, =SHARED_PAGE_MAGIC
    cmp     x5, x9
    b.ne    .Lfail_16

    /* Step 3: Verify lent page has SP's write */
    adr     x8, _tx_page
    ldr     w9, [x8]
    ldr     w10, =SHARED_PAGE_MAGIC
    cmp     w9, w10
    b.ne    .Lfail_16

    /* Step 4: MEM_RECLAIM */
    ldr     x0, =FFA_MEM_RECLAIM
    mov     x1, x24
    mov     x2, x25
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    ldr     x9, =FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    .Lfail_16

    adr     x0, str_pass
    bl      uart_print
    b       .Ldone
.Lfail_16:
    adr     x0, str_fail
    bl      uart_print
```

Key insight: SP Hello's `MEM_TEST_MAGIC` handler calls RETRIEVE → write → RELINQUISH.
This works identically for LEND — the SP doesn't need a separate LEND handler.
The SPMC `is_lend` flag in the share record handles LEND semantics.

- [ ] **Step 4: Build TF-A**

Run: `make build-tfa-spmc`
Expected: Builds successfully.

- [ ] **Step 5: Run BL33 integration tests**

Run: `make run-spmc`
Expected: 16/16 tests PASS (including new Test 16).

- [ ] **Step 6: Commit**

```bash
git add tfa/bl33_ffa_test/start.S
git commit -m "feat: add BL33 Test 16 — MEM_LEND lifecycle E2E"
```

## Chunk 5: Final Verification

### Task 10: Run all test suites and update docs

- [ ] **Step 1: Run unit tests**

Run: `make run`
Expected: 34 suites, ~415+ assertions, all pass.

- [ ] **Step 2: Run BL33 integration tests**

Run: `make run-spmc`
Expected: 16/16 tests PASS.

- [ ] **Step 3: Run TF-A Linux tests (if applicable)**

Run: `make run-tfa-linux`
Expected: 37/37 tests pass.

- [ ] **Step 4: Update CLAUDE.md**

Update:
- Test table: `test_ffa` and `test_spmc_handler` assertion counts
- BL33 test count: 15 → 16
- Add MEM_LEND E2E to feature list
- Mention range count validation and RXTX_UNMAP fragment cleanup in SpmcHandler description
- Update total assertion count

- [ ] **Step 5: Final commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for SPMC robustness hardening"
```
