# SP-to-SP DIRECT_REQ Design

## Goal

Implement FF-A v1.1 SP-to-SP DIRECT_REQ in the SPMC, enabling Secure Partitions to send synchronous messages to other SPs. Supports multi-layer chain calls (SP1→SP2→SP3) with cycle detection. Includes a new SP3 (sp_relay) for E2E integration testing.

## Background

Current state: the SPMC only supports NWd→SP DIRECT_REQ. If an SP issues DIRECT_REQ targeting another SP in `handle_sp_exit()`, it hits the unexpected-exit path and returns FFA_DENIED. The `Blocked` state in SpContext exists but is never driven.

FF-A v1.1 (DEN0077A §8.3.1) requires the SPMC to support SP-to-SP direct messaging when both endpoints are managed by the same SPMC.

## Architecture

**Approach**: Global call stack + recursive `dispatch_to_sp()`.

When SP1 issues DIRECT_REQ targeting SP2 inside `handle_sp_exit()`:
1. Validate source, destination, cycle detection
2. Push `CallFrame{SP1, SP2}` onto the global call stack
3. Transition SP1 from Running → Blocked
4. Recursively call `dispatch_to_sp(req, SP2)` — enters SP2 via ERET
5. On normal return (DIRECT_RESP): pop stack frame, SP1 Blocked → Running, write response to SP1's regs
6. On FFA_INTERRUPT return (callee preempted): SP1 also marked Preempted, unwind Rust stack, return FFA_INTERRUPT to NWd. FFA_RUN resumes the chain (see Section 5).
7. Continue SP1's `handle_sp_exit()` loop (re-enter SP1 via ERET)

Recursive depth bounded by MAX_SPS - 1 (currently 3 with 4 SPs). S-EL2 stack is 16KB — each recursion level adds ~256B of register saves, well within budget.

## Components

### 1. CallStack Data Structure

Location: `src/spmc_handler.rs` (alongside existing SPMC globals).

```rust
pub struct CallFrame {
    pub caller_id: u16,
    pub callee_id: u16,
}

pub struct CallStack {
    frames: [Option<CallFrame>; MAX_SPS - 1],  // use constant, not literal
    depth: usize,
}
```

Operations:
- `push(caller, callee) -> Result<(), ()>`: Add frame. Fails if stack full.
- `pop() -> Option<CallFrame>`: Remove top frame, return it.
- `contains(sp_id) -> bool`: Check if sp_id appears as caller or callee anywhere in the stack. Used for cycle detection.
- `depth() -> usize`: Current nesting depth.
- `find_caller(callee_id) -> Option<u16>`: Find the caller that is waiting for a given callee. Used by `resume_preempted_sp()` to chain-resume blocked callers.

Protected by `SpinLock<CallStack>` (same pattern as `SPMC_SHARES`, `NWD_RXTX`).

**Lock ordering**: CALL_STACK → SP_STORE_LOCK. Never acquire CALL_STACK while holding SP_STORE_LOCK. The `is_registered_sp()` check (which acquires SP_STORE_LOCK) must be done **before** acquiring CALL_STACK, since SP registration is static after boot.

### 2. handle_sp_exit() — SP→SP DIRECT_REQ Routing

Add `FFA_MSG_SEND_DIRECT_REQ_32` and `FFA_MSG_SEND_DIRECT_REQ_64` to the whitelist in `handle_sp_exit()`.

Validation sequence (order matters for lock safety):
1. **source_id == current sp_id** — prevent spoofing. Else FFA_INVALID_PARAMETERS.
2. **dest_id != sp_id** — no self-calls. Else FFA_INVALID_PARAMETERS.
3. **is_registered_sp(dest_id)** — destination must exist. Else FFA_INVALID_PARAMETERS. Done **before** acquiring CALL_STACK lock.
4. **!CALL_STACK.contains(dest_id)** — cycle detection. Else FFA_BUSY.
5. **CALL_STACK.push()** succeeds — stack not full. Else FFA_BUSY.

Steps 4-5 are atomic under CALL_STACK lock.

On success:
- Caller SP: Running → Blocked (registers already saved by `enter_guest()` return)
- Recursive `dispatch_to_sp(exit_regs_as_req, dest_id)`
- **If callee completes normally** (returns DIRECT_RESP): CALL_STACK.pop(), caller SP: Blocked → Running, write response to caller's x0-x7, fall through to re-entry code
- **If callee is preempted** (returns FFA_INTERRUPT): caller SP: Blocked → Preempted, do NOT pop stack, return FFA_INTERRUPT upward (see Section 5)

On any error: write FFA_ERROR + error code into caller SP's registers, continue loop (caller stays Running, no state change).

**Re-entry path**: After Blocked → Running, the match arm falls through to the existing re-entry code in `handle_sp_exit()` which does Running → Idle → Running + `restore_el1_state()` + `enter_guest()`. This adds 2 redundant state transitions per SP-to-SP call but is functionally correct and avoids a separate re-entry path.

**Note**: `try_lock_sp()` uses atomic CAS (non-blocking) — if the callee's lock is contended, it returns FFA_BUSY rather than spinning, preventing deadlock even in TOCTOU races between the call stack check and dispatch lock acquisition on different CPUs.

### 3. dispatch_to_sp() Adaptation

Minimal changes needed. `dispatch_to_sp()` already:
- Acquires SP dispatch lock (per-SP `SP_DISPATCH_LOCK[index]`)
- Transitions SP from Idle → Running
- Calls `enter_guest()` → `handle_sp_exit()` loop
- Returns SmcResult8

When called recursively from `handle_sp_exit()`, the **outer** SP's dispatch lock is still held. This is safe because each SP has its own independent dispatch lock — SP1's lock doesn't conflict with SP2's lock.

**EL1 sysreg window**: Between recursive `dispatch_to_sp()` return and caller re-entry, hardware EL1 sysregs reflect the callee's last-saved state. This is harmless since SPMC code at S-EL2 does not read EL1 sysregs. The caller's EL1 state is restored by `restore_el1_state()` before re-entering the caller via ERET.

### 4. Error Recovery

Every error path must ensure:
1. Call stack is cleaned up (pop if pushed)
2. Caller SP is not stuck in Blocked (restore to Running or Preempted)
3. Error code is written to caller's registers

If callee SP crashes during execution (unexpected exit → soft recovery to Idle, returns FFA_DENIED):
- `dispatch_to_sp()` returns FFA_DENIED
- Caller receives this as the "response"
- Stack frame is popped
- Caller resumes from Blocked → Running

### 5. Interrupt Interaction

#### NS Interrupt During Nested SP Execution

**Problem**: When SP2 is running in a chain (NWd→SP3→SP2) and NS IRQ arrives, `dispatch_to_sp(SP2)` returns FFA_INTERRUPT to SP3's `handle_sp_exit()`. But `resume_preempted_sp()` has no way to deliver SP2's eventual DIRECT_RESP back to SP3 — it returns to NWd.

**Solution (Hafnium-compatible chain preemption)**:

When a callee is preempted during SP-to-SP, **propagate preemption up the entire chain**:

1. SP2 exits with FFA_INTERRUPT → `dispatch_to_sp(SP2)` returns FFA_INTERRUPT
2. SP3's DIRECT_REQ match arm detects FFA_INTERRUPT return:
   - SP3: Blocked → Preempted (NOT Blocked → Running)
   - Do NOT pop the call stack frame `{SP3, SP2}`
   - Return FFA_INTERRUPT upward
3. If SP3 was called by NWd, FFA_INTERRUPT reaches NWd. **FFA_INTERRUPT carries SP2's partition ID** (the innermost preempted SP), not SP3's. NWd must use SP2's ID for the subsequent FFA_RUN.
4. `resume_preempted_sp(SP2)`:
   - Resumes SP2 (Preempted → Running)
   - SP2 completes with DIRECT_RESP
   - **New logic**: check CALL_STACK for a caller waiting on SP2
   - `find_caller(SP2)` returns SP3
   - Pop frame `{SP3, SP2}`
   - SP3: Preempted → Running, write SP2's response to SP3's registers
   - **Chain-resume via recursive `dispatch_to_sp()`-style re-entry**: call `enter_guest()` + `handle_sp_exit()` loop for SP3 within `resume_preempted_sp()`. If SP3 also has a caller in the stack, chain-resume continues recursively.
   - SP3 completes with DIRECT_RESP → returns to NWd

Chain-resume recursion is bounded by CALL_STACK depth (MAX_SPS-1 levels = 3 max), consuming the same ~256B per level as initial dispatch. Total worst case: 3 × 256B = 768B, well within the 16KB S-EL2 stack.

**Key change to `resume_preempted_sp()`**: After the callee finishes, check if CALL_STACK has a pending caller. If yes, chain-resume the caller instead of returning to NWd. This can itself recurse (SP1→SP2→SP3, SP3 preempted, resume SP3 → SP2 resumes → SP1 resumes).

**New state transition**: Blocked → Preempted (needed for chain preemption). Must be added to sp_context.rs transition table. Note: `test_sp_context` likely has an assertion that `Blocked → Preempted` is illegal — this test must be updated to assert it is now legal.

**New state transition**: Preempted → Running already exists and is reused.

#### Secure vIRQ for Blocked SP

If a Secure IRQ arrives for an SP that is currently Blocked (waiting for a callee):
- `dispatch_interrupt_to_sp()` checks target SP state **after acquiring dispatch lock, before transition**
- If target is Blocked: queue as `pending_irq` via `set_pending_irq_for()`, do not preempt
- IRQ will be delivered when SP returns to Running (after callee completes)

```rust
// In dispatch_interrupt_to_sp(), after acquiring dispatch lock:
if target_sp.state() == Blocked {
    target_sp.queue_pending_irq(intid);
    return; // don't preempt, wait for unblock
}
```

### 6. SP3 (sp_relay)

**Purpose**: Dedicated relay SP for testing SP-to-SP DIRECT_REQ. Does not pollute SP1/SP2 logic.

**Location**: `tfa/sp_relay/`

**Partition ID**: 0x8003

**Load address**: 0x0e500000 (current SECURE_HEAP_START moves to 0x0e600000)

**Behavior**:
```
boot:
  FFA_MSG_WAIT → Idle

on DIRECT_REQ:
  if x3 == RELAY_MAGIC (0x00EE1A00):
    target_sp = x4 (low 16 bits)
    FFA_MSG_SEND_DIRECT_REQ → target_sp, x5-x7 as payload
    on DIRECT_RESP from target:
      FFA_MSG_SEND_DIRECT_RESP → original caller, forward x4-x7
    on FFA_ERROR from SPMC:
      FFA_MSG_SEND_DIRECT_RESP → original caller, forward error
  else:
    echo: x4 += 0x2000, FFA_MSG_SEND_DIRECT_RESP
```

**Build**:
- `Makefile`: `build-sp-relay` target (same pattern as `build-sp-hello`, `build-sp-irq`)
- `tb_fw_config.dts`: add SP3 entry with UUID and load-address 0x0e500000
- `sp_relay_manifest.dts`: SP3 manifest with new UUID
- `build-tfa-spmc`: include SP3 in FIP

**SPMC boot changes**:
- `rust_main_sel2()`: detect SPKG magic at `SP3_LOAD_ADDR` (0x0e500000), boot SP3
- `platform.rs`: `SP3_LOAD_ADDR`, `SP3_PARTITION_ID`, `SECURE_HEAP_START` = 0x0e600000
- Build Secure Stage-2 for SP3

### 7. Memory Layout (Updated)

| Region | Address | Purpose |
|--------|---------|---------|
| SPMC code | 0x0e100000 | S-EL2 linker base (BL32) |
| SP1 (sp_hello) | 0x0e300000 | 1MB, partition 0x8001 |
| SP2 (sp_irq) | 0x0e400000 | 1MB, partition 0x8002 |
| SP3 (sp_relay) | 0x0e500000 | 1MB, partition 0x8003 |
| Secure heap | 0x0e600000 | S-EL2 page table allocation (was 0x0e500000) |

Secure heap shrinks from 11MB to 10MB (0x0e600000-0x0f000000). Each SP's Secure Stage-2 uses ~8KB (L1+L2+L3). With 3 SPs, total S2 allocation is ~24KB, well within 10MB.

## Testing

### Unit Tests (test_spmc_handler.rs, ~14 new assertions)

| Test | Description | Expected |
|------|-------------|----------|
| CallStack push/pop | Basic stack operations | Correct depth, returns |
| CallStack contains | Cycle detection lookup | True for stacked IDs |
| CallStack overflow | Push beyond MAX_SPS-1 | Returns Err |
| CallStack find_caller | Lookup caller by callee ID | Returns correct caller |
| SP→SP basic | SP1 DIRECT_REQ → SP2 (via handle_sp_exit path) | DIRECT_RESP, x4 += 0x1000 |
| SP→SP cycle | SP1→SP2→SP1 chain | SP2 receives FFA_BUSY |
| SP→SP self-call | SP1→SP1 | FFA_INVALID_PARAMETERS |
| SP→SP invalid dest | SP1→0x8099 | FFA_INVALID_PARAMETERS |
| SP→SP source spoof | SP1 claims source=SP2 | FFA_INVALID_PARAMETERS |
| SP→SP dest not Idle | SP2 already Running | FFA_BUSY |
| SP→SP callee crash | Callee hits unexpected exit | FFA_DENIED to caller, caller resumes |
| Blocked SP vIRQ queue | IRQ for Blocked SP | Queued, not preempted |
| SP→SP 32+64 variants | Both DIRECT_REQ_32 and _64 | Both work |
| NS preempt during chain | SP2 preempted in SP1→SP2 chain | FFA_INTERRUPT propagated, stack preserved |

Testing approach: need a new helper that simulates the SP-side `handle_sp_exit()` DIRECT_REQ path. Existing `dispatch_ffa_as_sp()` only tests SPMC-local handlers, not the SP→SP routing through `handle_sp_exit()`.

### BL33 Integration Test (Test 17)

```
NWd → DIRECT_REQ(SP3, x3=RELAY_MAGIC, x4=SP1_ID, x5=0xBEEF)
  SP3 → DIRECT_REQ(SP1, x4=0xBEEF)
    SP1 → DIRECT_RESP(x4=0xBEEF+0x1000=0x1BEEF)
  SP3 → DIRECT_RESP(x4=0x1BEEF)
NWd verify: x4 == 0x1BEEF
```

Validates full NWd→SP3→SP1→response chain through real ERET and SMC.

### BL33 Integration Test (Test 18, optional)

Cycle detection E2E: NWd → SP3(relay to SP3 itself) → FFA_INVALID_PARAMETERS (self-call blocked at SPMC level). Full cycle detection (SP1→SP2→SP1) is covered by unit tests since it requires two SPs that both support relay.

## Verification

```bash
make run           # 34 suites, ~430+ assertions (14 new)
make run-spmc      # 17-18/17-18 BL33 tests (Test 17 + optional 18)
make run-tfa-linux # 37/37 (no regression)
```

## Files Modified

| File | Change |
|------|--------|
| `src/spmc_handler.rs` | CallStack, handle_sp_exit whitelist, SP→SP routing, resume_preempted_sp chain-resume |
| `src/sp_context.rs` | Add Blocked → Preempted transition to transition table; update test_sp_context |
| `src/platform.rs` | SP3 constants, SECURE_HEAP_START moved to 0x0e600000 |
| `tfa/sp_relay/start.S` | New SP3 relay assembly |
| `tfa/sp_relay/sp_relay_manifest.dts` | New SP3 manifest |
| `tfa/sp_relay/linker.ld` | New SP3 linker script |
| `tfa/tb_fw_config.dts` | SP3 entry |
| `tfa/bl33_ffa_test/start.S` | Test 17 (+18 optional) |
| `tests/test_spmc_handler.rs` | ~14 new assertions |
| `Makefile` | build-sp-relay target |
| `CLAUDE.md` | Updated docs |
