# SPMC Robustness Hardening — Spec Compliance

**Date**: 2026-03-18
**Scope**: FF-A v1.1 spec compliance fixes — MEM_LEND E2E, fragment cleanup, range validation
**Threat model**: Spec compliance first (not adversarial hardening)

## Background

Phase 4.7 completed security hardening (cross-SP isolation, IPA validation, stress tests).
This sprint focuses on three FF-A spec compliance gaps discovered during review:

1. MEM_LEND lacks E2E test coverage (code paths exist but are unverified)
2. SPMC `RXTX_UNMAP` does not clean up in-flight fragment state
3. Descriptor range count exceeding storage limits is silently truncated

**Important constants**:
- `MAX_ADDR_RANGES = 16` (descriptors.rs) — parser-level array capacity
- `MAX_SHARE_RANGES = 4` (spmc_handler.rs) — SPMC storage per share record
- `MAX_SHARE_RANGES = 4` (stub_spmc.rs) — stub SPMC storage per share record

The parser accepts up to 16 ranges but SPMC/stub storage only holds 4. Truncation happens
at the storage layer, not the parser layer.

## Sub-item 1: MEM_LEND E2E Validation

### Current State

- **NS proxy sender side** (proxy.rs:727-731): Correctly differentiates SHARE vs LEND
  - SHARE → sender S2AP = RO (read-only, sender retains read)
  - LEND → sender S2AP = NONE (sender loses all access)
- **SPMC receiver side** (spmc_handler.rs:2022-2028 direct path, 2034-2045 `with_sp_locked` fallback):
  RETRIEVE maps with S2AP=RW for both SHARE and LEND.
  This is correct per FF-A spec: receiver gets full access in both cases.
  Both mapping paths must be covered by E2E testing.
- **`SpmcShareRecord.is_lend`**: Already stored and returned by `lookup_spmc_share()`
- **Gap**: No E2E test verifies the LEND lifecycle (LEND → RETRIEVE → write → RELINQUISH → RECLAIM)

### Changes

#### Proxy unit tests (test_ffa.rs)

- **LEND_BASIC**: MEM_LEND with register-based protocol → verify `FFA_SUCCESS` + handle returned
- **LEND_SENDER_S2AP**: MEM_LEND → verify sender pages marked S2AP=NONE (vs SHARE's S2AP_RO)

#### SPMC unit tests (test_spmc_handler.rs)

- **LEND_LIFECYCLE**: MEM_LEND → RETRIEVE → RELINQUISH → RECLAIM full lifecycle
- **LEND_RETRIEVE_WHILE_ACTIVE**: LEND → RETRIEVE → second RETRIEVE → FFA_DENIED
- **LEND_RECLAIM_WHILE_RETRIEVED**: LEND → RETRIEVE → RECLAIM (without RELINQUISH) → FFA_DENIED

#### BL33 integration test (Test 16)

Full E2E: NWd MEM_LEND → SP RETRIEVE → SP write to lent page → SP RELINQUISH → NWd RECLAIM.
Mirrors existing Test 14 (MEM_SHARE E2E) but with `FFA_MEM_LEND` function ID.

SP Hello needs a LEND test command (e.g., x3=0xABCD0002) that behaves identically to the
existing SHARE test command (0xABCD0001): RETRIEVE → write → RELINQUISH.

BL33 Test 16 is a composite assertion (LEND ok + SP RETRIEVE ok + SP write ok +
SP RELINQUISH ok + NWd verify data + RECLAIM ok), counted as 1 integration test pass/fail.

## Sub-item 2: RXTX_UNMAP Fragment State Cleanup

### Current State

- **NS proxy** (proxy.rs:394-396): `handle_rxtx_unmap()` resets `FRAG_STATE[vm_id]` — correct
- **SPMC** (spmc_handler.rs:1544-1564): `handle_rxtx_unmap()` clears RXTX addresses but does
  NOT reset `NWD_FRAG` or `NWD_FRAG_RX` — if a fragment transfer was in progress, state remains
  `active=true`, blocking all future fragments with `FFA_BUSY`

### Changes

#### spmc_handler.rs — `handle_rxtx_unmap()`

After clearing RXTX addresses, add:

```rust
// Clean up any in-flight fragment state (spec: RXTX_UNMAP invalidates ongoing transfers)
reset_nwd_frag_state();  // defined at line 208, currently unused (dead code)
{
    let mut frag_rx = NWD_FRAG_RX.lock();
    frag_rx.active = false;
}
```

`reset_nwd_frag_state()` exists (spmc_handler.rs:208) but has zero call sites — it is dead code.
This change gives it its first production caller.

#### SPMC unit test (test_spmc_handler.rs)

- **UNMAP_CLEARS_FRAG**: RXTX_MAP → start FRAG_TX (partial, don't complete) → RXTX_UNMAP →
  RXTX_MAP again → new MEM_SHARE must return `FFA_SUCCESS` with valid handle (not `FFA_BUSY`)

## Sub-item 3: Range Count Overflow → Error

### Current State

Two-layer truncation with different limits:

1. **Parser layer** (descriptors.rs:170): `count = range_count.min(MAX_ADDR_RANGES)` — caps at 16
2. **Storage layer** — multiple sites silently truncate to `MAX_SHARE_RANGES` (4):
   - `spmc_handler.rs:282` — `record_spmc_share()`: `ranges.len().min(MAX_SHARE_RANGES)`
   - `spmc_handler.rs:1778` — inside `handle_spmc_mem_frag_tx()` (fragment completion): `desc.range_count.min(MAX_SHARE_RANGES)`
   - `spmc_handler.rs:1879` — inside `handle_spmc_mem_share()` (descriptor-based non-fragmented path): same pattern
   - `stub_spmc.rs:101-102` — `record_share()`: `ranges.len().min(MAX_SHARE_RANGES)`
   - `stub_spmc.rs:139-140` — `record_share_with_handle()`: `ranges.len().min(MAX_SHARE_RANGES)`

The proxy's `parse_share_descriptor()` does not do secondary truncation — it passes through
the `ParsedMemRegion` from `descriptors::parse_mem_region()` directly.

### Changes

#### Validation logic

Replace silent truncation with explicit error at **storage entry points** (where
`MAX_SHARE_RANGES` is enforced). The parser layer (`MAX_ADDR_RANGES=16`) is left unchanged
since it serves as a reasonable upper bound for descriptor parsing.

```rust
if range_count > MAX_SHARE_RANGES {
    return error (FFA_INVALID_PARAMETERS);
}
```

Apply in these storage-layer sites:
1. `spmc_handler.rs` — `record_spmc_share()` (guards all SPMC share paths — single chokepoint)
2. `stub_spmc.rs` — `record_share()` and `record_share_with_handle()` (stub paths)
3. `proxy.rs` — after `parse_mem_region()` returns, before calling `complete_share()`,
   check `parsed.range_count > MAX_SHARE_RANGES`

Note: `handle_spmc_mem_share()` already calls `record_spmc_share()`, but
`handle_spmc_mem_frag_tx()` currently has inline record creation (lines 1779-1804).
Implementation must refactor `handle_spmc_mem_frag_tx()` to call `record_spmc_share()`
so that validation in the single chokepoint covers both paths.

The parser in `descriptors.rs` keeps `MAX_ADDR_RANGES=16` truncation as a safety net —
no change needed there.

#### Unit tests

- **Proxy**: Construct descriptor with range_count=5 → verify `FFA_INVALID_PARAMETERS`
- **SPMC**: Same test for SPMC register-based path

## Test Summary

| Location | New Tests | New Assertions (est.) |
|----------|-----------|----------------------|
| test_ffa.rs (proxy) | LEND_BASIC, LEND_SENDER_S2AP, RANGE_OVERFLOW | ~4 |
| test_spmc_handler.rs | LEND_LIFECYCLE, LEND_RETRIEVE_ACTIVE, LEND_RECLAIM_RETRIEVED, UNMAP_CLEARS_FRAG, RANGE_OVERFLOW | ~6 |
| BL33 integration | Test 16 (MEM_LEND E2E) | 1 |
| **Total** | **~9 tests** | **~11 assertions** |

Post-change targets:
- `make run`: 34 suites, ~410 assertions
- `make run-spmc`: 16/16 BL33 tests
- `make run-tfa-linux`: 37/37

## Files Modified

| File | Changes |
|------|---------|
| `src/spmc_handler.rs` | RXTX_UNMAP fragment cleanup, range count validation in `record_spmc_share()` |
| `src/ffa/proxy.rs` | Range count validation after `parse_mem_region()` |
| `src/ffa/stub_spmc.rs` | Range count validation in `record_share()` / `record_share_with_handle()` |
| `tests/test_ffa.rs` | MEM_LEND tests, range overflow test |
| `tests/test_spmc_handler.rs` | MEM_LEND lifecycle, UNMAP fragment cleanup, range overflow |
| `tfa/sp_hello/sp_hello.S` | LEND test command handler (x3=0xABCD0002) |
| BL33 test in `src/spmc_handler.rs` or test harness | Test 16: MEM_LEND E2E |

## Non-goals

- Per-SP share quotas (deferred to adversarial hardening sprint)
- Fragment timeout mechanism (deferred — RXTX_UNMAP cleanup is sufficient per spec)
- Descriptor parser `MAX_ADDR_RANGES` change (16 is a safe upper bound for parsing)
- `stub_spmc.rs` truncation sites are included in scope (not deferred)
- MEM_DONATE implementation (explicitly NOT_SUPPORTED per design)
