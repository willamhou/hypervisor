# Backlog: Actionable Items (Post M4.6 S2)

**Date**: 2026-03-04
**Status**: Backlog — prioritize as needed
**Prereq**: All 33 test suites pass, ~347 assertions, 14/14 BL33 tests, pKVM boots

---

## Quick Wins (< 1 hour each)

### QW-1: Replace bare `4096` with `PAGE_SIZE_4KB` in spmc_handler.rs
- **File**: `src/spmc_handler.rs` lines ~1524, 1540, 1592, 1608
- **Why**: Inconsistency — rest of codebase uses `PAGE_SIZE_4KB` constant
- **Risk**: None (cosmetic)

### QW-2: `is_valid_receiver()` ignores real SPs in `tfa_boot` mode
- **File**: `src/ffa/mod.rs:116`
- **Why**: `is_valid_receiver()` calls `stub_spmc::is_valid_sp()` which hardcodes 0x8001/0x8002. In `tfa_boot` with real SPs, dynamically booted SPs beyond those two IDs would be rejected.
- **Fix**: One-line: also check `sp_context::is_registered_sp()` when `cfg(feature = "sel2")`

### QW-3: Upgrade PSCI version claim from v0.2 to v1.0
- **File**: `src/arch/aarch64/hypervisor/exception.rs:1063`
- **Why**: We return `PSCI_VERSION_0_2 = 0x00000002` but already support PSCI 1.0 features (FEATURES, SYSTEM_RESET2)
- **Fix**: Change constant to `0x00010000`

### QW-4: Remove or annotate PROXY_TX_BUF
- **File**: `src/ffa/proxy.rs:33`
- **Why**: `#[allow(dead_code)]` 4KB buffer "reserved for future MEM_SHARE descriptor forwarding to SPMC" — never written. Either remove (saves 4KB BSS) or wire up in ME-4.

---

## Medium Effort (1-4 hours each)

### ME-1: Fix pKVM sched callback -95 (FFA_HOST_ID in notifications)
- **File**: `src/ffa/notifications.rs:75-84`
- **Why**: pKVM calls `FFA_NOTIFICATION_BITMAP_CREATE` with partition ID 0x0000 (`FFA_HOST_ID`). `endpoint_index()` returns `None` for 0x0000 → `FFA_INVALID_PARAMETERS` → `-95 EOPNOTSUPP`. The sched callback is the **last remaining pKVM -95 gap**.
- **Fix**: Add `0x0000 => Some(MAX_ENDPOINTS - 1)` case in `endpoint_index()`
- **Test**: N-new test: `BITMAP_CREATE(0x0000)` → SUCCESS
- **Verify**: `make run-pkvm` — no more `-95` in dmesg

### ME-2: Forward MEM_SHARE/RECLAIM to real SPMC in `tfa_boot` mode
- **Files**: `src/ffa/proxy.rs:617-638`, `PROXY_TX_BUF`
- **Why**: When `SPMC_PRESENT=true`, `handle_mem_share_or_lend()` still calls `stub_spmc::record_share()` instead of forwarding to real SPMC. Needed for `make run-tfa-linux-ffa` memory sharing.
- **Steps**:
  1. Copy parsed descriptor into `PROXY_TX_BUF`
  2. Call `forward_ffa_to_spmc()` with original x0-x7
  3. Forward RECLAIM similarly

### ME-3: SPMC-side MSG_SEND2 + MSG_WAIT (Sprint S3)
- **File**: `src/spmc_handler.rs`
- **Why**: `dispatch_ffa()` has no MSG_SEND2 handler → NOT_SUPPORTED. Needed for indirect NWd→SP messaging.
- **Steps**:
  1. Per-SP RXTX buffer registration in `SpContext`
  2. MSG_SEND2: read NWd TX → write target SP RX, set `msg_pending`
  3. MSG_WAIT: if `msg_pending`, return immediately with message
- **Reference**: Mirror `handle_msg_send2()`/`handle_msg_wait()` from proxy.rs (~80 LOC each)

### ME-4: Concurrent safety hardening (SpinLock migration)
- **Files**: `src/spmc_handler.rs`, `src/ffa/notifications.rs`, `src/sp_context.rs`
- **Why**: `NWD_RXTX`, `SPMC_SHARES`, `NotifStateArray` all use `UnsafeCell` with "SPMC event loop serialized" safety comments. With 4-CPU pKVM, two CPUs can enter S-EL2 event loop simultaneously.
- **Fix**: Wrap each in `SpinLock<T>` (or at minimum add `AtomicBool` guards)
- **Risk**: Medium — must not break existing 95 + 58 test assertions

### ME-5: FFA_MEM_FRAG_TX/RX fragmentation support
- **Files**: `src/ffa/proxy.rs`, `src/spmc_handler.rs`
- **Why**: Constants defined but never handled. Both enforce `total_length == fragment_length`. Linux FF-A driver may send fragmented descriptors for large shares.
- **Fix**: Accumulation buffer (up to 4KB), reassemble before parsing

### ME-6: probe_spmc() — enable FFA_VERSION probe for real hardware
- **File**: `src/ffa/smc_forward.rs:181`
- **Why**: Always returns `false` because QEMU's EL3 crashes on FFA_VERSION. Needs TF-A detection heuristic.
- **Fix**: Check PSCI version minor field (TF-A returns specific patterns) or add compile-time `tfa_present` flag

---

## Large Effort (> 4 hours each)

### LE-1: SecureStage2Walker for SPMC memory (M4.6 Sprint S1 core)
- **Effort**: 4-6 hours
- **Why**: Current `Stage2Walker` reads `VTTBR_EL2` (NS side). For SPMC, need `SecureStage2Walker` reading `VSTTBR_EL2`. PA is NS PA of NWd-shared page (not identity-mapped IPA).
- **New file**: `src/ffa/secure_stage2_walker.rs`
- **Prereq**: ME-4 (concurrent safety)

### LE-2: Unify proxy.rs / spmc_handler.rs duplication
- **Effort**: 4-8 hours
- **Why**: `proxy.rs` (1179 LOC) and `spmc_handler.rs` (1679 LOC) have parallel implementations for share records, descriptor parsing, notification dispatch, error handling. Core gap: proxy mutates `VcpuContext` directly, SPMC returns `SmcResult8`.
- **Approach**: Trait-based or function-pointer abstraction layer
- **Risk**: High — 33 test suites must pass after refactor

### LE-3: M5 — RME & CCA Realm Manager
- **Effort**: 16-20 weeks
- **Status**: Not started. Requires:
  - Sprint 5.1: Granule Protection Table (GPT), four-world memory isolation
  - Sprint 5.2: Realm Translation Tables (RTT), RMI_REALM_CREATE
  - Sprint 5.3: Realm execution (RMI_REC_ENTER), RSI interface
  - Sprint 5.4: Linux guest inside Realm VM
- **Prereq**: RME-capable platform (FVP or real silicon). QEMU virt does not support RME.
- **Reference**: See DEVELOPMENT_PLAN.md Milestone 5

---

## Suggested Priority Order

1. **ME-1** (sched callback -95) — last pKVM gap, high user-visible impact
2. **QW-1 + QW-2 + QW-3** — batch quick cosmetic fixes
3. **ME-4** (SpinLock migration) — prerequisite for correctness under pKVM 4-CPU
4. **ME-2** (forward MEM_SHARE to real SPMC) — needed for real E2E sharing
5. **ME-3** (MSG_SEND2) — needed for indirect messaging
6. **LE-1** (SecureStage2Walker) — core SPMC memory sharing upgrade
7. **LE-2** (dedup refactor) — code quality, not blocking features
8. **LE-3** (RME) — next major milestone
