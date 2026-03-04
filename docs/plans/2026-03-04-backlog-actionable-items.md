# Backlog: Actionable Items (Post M4.6 S2)

**Date**: 2026-03-04
**Updated**: 2026-03-04
**Status**: Backlog — prioritize as needed
**Prereq**: All 33 test suites pass, ~347 assertions, 14/14 BL33 tests, pKVM boots

---

## Quick Wins (< 1 hour each) — ALL DONE

### ~~QW-1: Replace bare `4096` with `PAGE_SIZE_4KB` in spmc_handler.rs~~ ✅
- **Commit**: `f1793a7`
- Replaced 5 bare `4096` with `PAGE_SIZE_4KB` constant

### ~~QW-2: `is_valid_receiver()` ignores real SPs in `tfa_boot` mode~~ ✅
- **Commit**: `f1793a7`
- Added `sp_context::is_registered_sp()` check under `#[cfg(feature = "sel2")]`

### ~~QW-3: Upgrade PSCI version claim from v0.2 to v1.0~~ ✅
- **Commit**: `f1793a7`
- Changed constant to `0x00010000`, Linux detects `PSCIv1.0`

### ~~QW-4: Remove or annotate PROXY_TX_BUF~~ ✅
- **Commit**: `f1793a7`
- Removed stale `#[allow(dead_code)]` (buffer IS used for RXTX_MAP)

---

## Medium Effort (1-4 hours each)

### ~~ME-1: Fix pKVM BITMAP_CREATE -22 (FFA_HOST_ID in notifications)~~ ✅
- **Commit**: `b19b7f0`
- **Fix**: Added `0x0000 => Some(FFA_MAX_VMS + 2)` case in `endpoint_index()`
- **Result**: BITMAP_CREATE `-22` eliminated. Remaining `-95` messages are **informational** (`pr_info`):
  - `Notification setup failed -95, not enabled` — SRI/NPI not implemented (FFA_FEATURES x1=1/2)
  - `Failed to register driver sched callback -95` — cascade from above
- **Note**: SRI (Schedule Receiver Interrupt) requires SPMC donating SGIs to NWd via GIC — tracked as ME-7

### ~~ME-2: Forward MEM_SHARE/RECLAIM to real SPMC in `tfa_boot` mode~~ ✅
- **Commit**: `de9526c`
- Dual record (local + forward) with SPMC handle, TX buffer relay, RECLAIM ordering

### ME-3: SPMC-side MSG_SEND2 + MSG_WAIT (Sprint S3)
- **File**: `src/spmc_handler.rs`
- **Why**: `dispatch_ffa()` has no MSG_SEND2 handler → NOT_SUPPORTED. Needed for indirect NWd→SP messaging.
- **Steps**:
  1. Per-SP RXTX buffer registration in `SpContext`
  2. MSG_SEND2: read NWd TX → write target SP RX, set `msg_pending`
  3. MSG_WAIT: if `msg_pending`, return immediately with message
- **Reference**: Mirror `handle_msg_send2()`/`handle_msg_wait()` from proxy.rs (~80 LOC each)

### ~~ME-4: Concurrent safety hardening (SpinLock migration)~~ ✅
- **Commit**: `0ad9fbe`
- Replaced `UnsafeCell` with `SpinLock` for NWD_RXTX, SPMC_SHARES, NOTIF_STATE
- Removed 3 `unsafe impl Sync`, added proper lock ordering (copy-then-drop)
- All 33 test suites pass, pKVM 4-CPU regression verified

### ME-5: FFA_MEM_FRAG_TX/RX fragmentation support
- **Files**: `src/ffa/proxy.rs`, `src/spmc_handler.rs`
- **Why**: Constants defined but never handled. Both enforce `total_length == fragment_length`. Linux FF-A driver may send fragmented descriptors for large shares.
- **Fix**: Accumulation buffer (up to 4KB), reassemble before parsing

### ME-6: probe_spmc() — enable FFA_VERSION probe for real hardware
- **File**: `src/ffa/smc_forward.rs:181`
- **Why**: Always returns `false` because QEMU's EL3 crashes on FFA_VERSION. Needs TF-A detection heuristic.
- **Fix**: Check PSCI version minor field (TF-A returns specific patterns) or add compile-time `tfa_present` flag

### ME-7: SRI/NPI — Schedule Receiver Interrupt + Notification Pending Interrupt
- **Files**: `src/spmc_handler.rs` (FFA_FEATURES), GIC SGI configuration
- **Why**: Linux FF-A driver calls `FFA_FEATURES(x1=1)` (NPI) and `FFA_FEATURES(x1=2)` (SRI) to get donated SGI INTIDs from SPMC. Our SPMC returns NOT_SUPPORTED → informational `-95` in dmesg.
- **Impact**: pKVM boots fine without this — messages are `pr_info()`, not fatal
- **Steps**:
  1. SPMC allocates two SGI INTIDs (e.g. SGI 8 for NPI, SGI 9 for SRI)
  2. FFA_FEATURES(x1=1/2) returns donated INTID in w2
  3. GIC configuration: route SGIs from Secure to Non-Secure world
  4. Trigger SRI/NPI SGIs from S-EL2 when notifications are set/pending

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

1. ~~**QW-1 + QW-2 + QW-3 + QW-4**~~ ✅ Done (commit `f1793a7`)
2. ~~**ME-4** (SpinLock migration)~~ ✅ Done (commit `0ad9fbe`)
3. ~~**ME-2** (forward MEM_SHARE to real SPMC)~~ ✅ Done (commit `de9526c`)
4. ~~**ME-1** (BITMAP_CREATE -22)~~ ✅ Done (BITMAP_CREATE fixed, SRI/NPI deferred to ME-7)
5. **ME-3** (MSG_SEND2) — needed for indirect messaging
6. **ME-7** (SRI/NPI) — eliminates informational pKVM `-95` messages
7. **LE-1** (SecureStage2Walker) — core SPMC memory sharing upgrade
8. **LE-2** (dedup refactor) — code quality, not blocking features
9. **LE-3** (RME) — next major milestone
