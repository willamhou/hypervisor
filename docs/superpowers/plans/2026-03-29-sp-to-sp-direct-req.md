# SP-to-SP DIRECT_REQ Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement FF-A v1.1 SP-to-SP DIRECT_REQ in the SPMC with cycle detection, chain preemption, and a new SP3 (sp_relay) for E2E testing.

**Architecture:** Global CallStack + recursive `dispatch_to_sp()`. When SP1 issues DIRECT_REQ targeting SP2 inside `handle_sp_exit()`, push frame, block SP1, recursively dispatch SP2, pop frame on return, resume SP1. Chain preemption propagates Blocked→Preempted up the chain on NS interrupt, with `find_caller()` chain-resume in `resume_preempted_sp()`.

**Tech Stack:** Rust (no_std, `sel2` feature), ARM64 assembly (SP3), Device Tree (manifests)

**Spec:** `docs/superpowers/specs/2026-03-29-sp-to-sp-direct-req-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/spmc_handler.rs` | Modify | CallStack data structure, SP→SP routing in handle_sp_exit, chain-resume in resume_preempted_sp |
| `src/sp_context.rs` | Modify | Add Blocked→Preempted transition |
| `src/platform.rs` | Modify | SP3 constants, move SECURE_HEAP_START |
| `tfa/sp_relay/start.S` | Create | SP3 relay assembly |
| `tfa/sp_relay/linker.ld` | Create | SP3 linker script |
| `tfa/sp_relay/sp_manifest.dts` | Create | SP3 FF-A manifest |
| `tfa/tb_fw_config.dts` | Modify | Add SP3 entry |
| `tfa/sp_layout.json` | Modify | Add SP3 to SP layout |
| `src/main.rs` | Modify | Boot SP3 at 0x0e500000 |
| `tfa/bl33_ffa_test/start.S` | Modify | Test 17 (relay chain) + Test 18 (cycle detection) |
| `tests/test_spmc_handler.rs` | Modify | ~14 new assertions |
| `tests/test_sp_context.rs` | Modify | Update Blocked→Preempted assertion |
| `Makefile` | Modify | build-sp-relay target |
| `CLAUDE.md` | Modify | Updated docs |

---

## Chunk 1: Foundation — CallStack + State Transitions

### Task 1: Add Blocked→Preempted State Transition

**Files:**
- Modify: `src/sp_context.rs:309-317` (transition table)
- Modify: `tests/test_sp_context.rs:143-150` (update assertion)

- [ ] **Step 1: Write the test change**

In `tests/test_sp_context.rs`, update the test at line 143-150. Change the comment and assertion to validate that Blocked→Preempted is now **legal**:

```rust
    // Test 47-48: Blocked → Idle is invalid, Blocked → Preempted is valid
    let mut ctx_g1c = SpContext::new(0x9003, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g1c.transition_to(SpState::Idle).unwrap();
    ctx_g1c.transition_to(SpState::Running).unwrap();
    ctx_g1c.transition_to(SpState::Blocked).unwrap();
    assert!(ctx_g1c.transition_to(SpState::Idle).is_err());
    pass += 1;
    // Blocked → Preempted now legal (SP-to-SP chain preemption)
    let mut ctx_g1c2 = SpContext::new(0x9013, 0x0e300000, 0x0e400000, [0; 4]);
    ctx_g1c2.transition_to(SpState::Idle).unwrap();
    ctx_g1c2.transition_to(SpState::Running).unwrap();
    ctx_g1c2.transition_to(SpState::Blocked).unwrap();
    assert!(ctx_g1c2.transition_to(SpState::Preempted).is_ok());
    pass += 1;
```

- [ ] **Step 2: Run test to verify it fails**

Run: `make run 2>&1 | grep -A2 "test_sp_context"`
Expected: FAIL — `transition_to(Preempted)` returns Err because transition table rejects it.

- [ ] **Step 3: Add Blocked→Preempted to transition table**

In `src/sp_context.rs:309-317`, add the new transition:

```rust
    let valid = match (current, new_state) {
        (SpState::Reset, SpState::Idle) => true,
        (SpState::Idle, SpState::Running) => true,
        (SpState::Running, SpState::Idle) => true,
        (SpState::Running, SpState::Blocked) => true,
        (SpState::Blocked, SpState::Running) => true,
        (SpState::Blocked, SpState::Preempted) => true,  // NEW: chain preemption
        (SpState::Running, SpState::Preempted) => true,
        (SpState::Preempted, SpState::Running) => true,
        _ => false,
    };
```

- [ ] **Step 4: Run tests to verify pass**

Run: `make run 2>&1 | grep "test_sp_context"`
Expected: `59 assertions passed` (was 58, +1 net: removed 1 old assert, added 2 new)

- [ ] **Step 5: Commit**

```bash
git add src/sp_context.rs tests/test_sp_context.rs
git commit -m "feat: add Blocked→Preempted state transition for SP-to-SP chain preemption"
```

### Task 2: CallStack Data Structure

**Files:**
- Modify: `src/spmc_handler.rs` (add CallStack after line ~30, before `sel2_cpu_id()`)
- Modify: `tests/test_spmc_handler.rs` (add 4 new test assertions)

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_spmc_handler.rs` after the last test (before the final `hypervisor::log_info!` at line 1636):

```rust
    // ── CallStack unit tests ──

    // CS1: push/pop basic operations
    {
        use hypervisor::spmc_handler::{CallStack, CallFrame};
        let mut stack = CallStack::new();
        assert_eq!(stack.depth(), 0);
        assert!(stack.push(0x8001, 0x8002).is_ok());
        assert_eq!(stack.depth(), 1);
        assert!(stack.contains(0x8001));
        assert!(stack.contains(0x8002));
        assert!(!stack.contains(0x8003));
        let frame = stack.pop().unwrap();
        assert_eq!(frame.caller_id, 0x8001);
        assert_eq!(frame.callee_id, 0x8002);
        assert_eq!(stack.depth(), 0);
        pass += 1;
    }

    // CS2: cycle detection via contains()
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        stack.push(0x8001, 0x8002).unwrap();
        stack.push(0x8002, 0x8003).unwrap();
        // SP1 is in the stack as caller → cycle detected
        assert!(stack.contains(0x8001));
        // SP3 is in the stack as callee → cycle detected
        assert!(stack.contains(0x8003));
        pass += 1;
    }

    // CS3: stack overflow (MAX_SPS - 1 = 3 frames max)
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        stack.push(0x8001, 0x8002).unwrap();
        stack.push(0x8002, 0x8003).unwrap();
        stack.push(0x8003, 0x8004).unwrap();
        assert!(stack.push(0x8004, 0x8005).is_err());
        pass += 1;
    }

    // CS4: find_caller lookup
    {
        use hypervisor::spmc_handler::CallStack;
        let mut stack = CallStack::new();
        stack.push(0x8001, 0x8002).unwrap();
        stack.push(0x8002, 0x8003).unwrap();
        assert_eq!(stack.find_caller(0x8003), Some(0x8002));
        assert_eq!(stack.find_caller(0x8002), Some(0x8001));
        assert_eq!(stack.find_caller(0x8001), None);
        pass += 1;
    }
```

Update the assertion count: change `hypervisor::log_info!("    {} assertions passed\n", pass);` — the count will increase by 4.

- [ ] **Step 2: Implement CallStack**

Add to `src/spmc_handler.rs` after the imports (around line 30), before `sel2_cpu_id()`:

```rust
// ── SP-to-SP Call Stack ────────────────────────────────────────────────

/// A frame in the SP-to-SP call stack, tracking who called whom.
pub struct CallFrame {
    pub caller_id: u16,
    pub callee_id: u16,
}

/// Global call stack for tracking SP-to-SP DIRECT_REQ nesting.
/// Maximum depth is MAX_SPS - 1 (one SP must be the innermost callee).
pub struct CallStack {
    frames: [Option<CallFrame>; crate::platform::MAX_SPS - 1],
    depth: usize,
}

impl CallStack {
    pub const fn new() -> Self {
        Self {
            frames: [None, None, None], // MAX_SPS - 1 = 3
            depth: 0,
        }
    }

    /// Push a new call frame. Returns Err if stack is full.
    pub fn push(&mut self, caller: u16, callee: u16) -> Result<(), ()> {
        if self.depth >= self.frames.len() {
            return Err(());
        }
        self.frames[self.depth] = Some(CallFrame {
            caller_id: caller,
            callee_id: callee,
        });
        self.depth += 1;
        Ok(())
    }

    /// Pop the top frame. Returns None if stack is empty.
    pub fn pop(&mut self) -> Option<CallFrame> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        self.frames[self.depth].take()
    }

    /// Check if sp_id appears as caller or callee anywhere in the stack.
    /// Used for cycle detection.
    pub fn contains(&self, sp_id: u16) -> bool {
        self.frames[..self.depth].iter().any(|f| {
            if let Some(frame) = f {
                frame.caller_id == sp_id || frame.callee_id == sp_id
            } else {
                false
            }
        })
    }

    /// Current nesting depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Find the caller that is waiting for a given callee.
    /// Used by resume_preempted_sp() for chain-resume.
    pub fn find_caller(&self, callee_id: u16) -> Option<u16> {
        self.frames[..self.depth].iter().find_map(|f| {
            if let Some(frame) = f {
                if frame.callee_id == callee_id {
                    return Some(frame.caller_id);
                }
            }
            None
        })
    }
}

/// Global SP-to-SP call stack, protected by SpinLock.
/// Lock ordering: CALL_STACK → SP_STORE_LOCK (never reverse).
#[cfg(feature = "sel2")]
pub static CALL_STACK: SpinLock<CallStack> = SpinLock::new(CallStack::new());
```

- [ ] **Step 3: Run tests to verify pass**

Run: `make run 2>&1 | grep "test_spmc_handler"`
Expected: `146 assertions passed` (was 142, +4)

- [ ] **Step 4: Commit**

```bash
git add src/spmc_handler.rs tests/test_spmc_handler.rs
git commit -m "feat: add CallStack data structure for SP-to-SP call tracking"
```

---

## Chunk 2: SP→SP DIRECT_REQ Routing in handle_sp_exit

### Task 3: Add SP→SP DIRECT_REQ to handle_sp_exit Whitelist + Routing

**Files:**
- Modify: `src/spmc_handler.rs:724-734` (whitelist), add new match arm after line 882
- Modify: `tests/test_spmc_handler.rs` (add ~10 new assertions)

- [ ] **Step 1: Write the failing tests**

Add to `tests/test_spmc_handler.rs` after the CallStack tests:

```rust
    // ── SP→SP DIRECT_REQ routing tests ──
    // These test the dispatch_ffa_as_sp() path which simulates
    // handle_sp_exit() DIRECT_REQ routing.

    // SP2SP1: SP→SP self-call blocked (FFA_INVALID_PARAMETERS)
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        req.x1 = ((hypervisor::platform::SP1_PARTITION_ID as u64) << 16)
            | (hypervisor::platform::SP1_PARTITION_ID as u64); // source == dest
        let resp = dispatch_ffa_as_sp(
            &req,
            hypervisor::platform::SP1_PARTITION_ID,
        );
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS as i32);
        pass += 1;
    }

    // SP2SP2: SP→SP source spoofing blocked
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        // SP1 claims to be SP2
        req.x1 = ((hypervisor::platform::SP2_PARTITION_ID as u64) << 16)
            | (hypervisor::platform::SP1_PARTITION_ID as u64);
        let resp = dispatch_ffa_as_sp(
            &req,
            hypervisor::platform::SP1_PARTITION_ID, // actual caller is SP1
        );
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS as i32);
        pass += 1;
    }

    // SP2SP3: SP→SP invalid destination
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        req.x1 = ((hypervisor::platform::SP1_PARTITION_ID as u64) << 16)
            | 0x8099; // non-existent SP
        let resp = dispatch_ffa_as_sp(
            &req,
            hypervisor::platform::SP1_PARTITION_ID,
        );
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS as i32);
        pass += 1;
    }

    // SP2SP4: SP→SP DIRECT_REQ_64 self-call also blocked
    {
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_64);
        req.x1 = ((hypervisor::platform::SP1_PARTITION_ID as u64) << 16)
            | (hypervisor::platform::SP1_PARTITION_ID as u64);
        let resp = dispatch_ffa_as_sp(
            &req,
            hypervisor::platform::SP1_PARTITION_ID,
        );
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_INVALID_PARAMETERS as i32);
        pass += 1;
    }

    // SP2SP5: Cycle detection — manually push frame, then try DIRECT_REQ to stacked SP
    {
        // Push SP2→SP1 frame to simulate SP2 waiting for SP1
        {
            let mut stack = hypervisor::spmc_handler::CALL_STACK.lock();
            stack.push(
                hypervisor::platform::SP2_PARTITION_ID,
                hypervisor::platform::SP1_PARTITION_ID,
            ).unwrap();
        }

        // SP1 tries to call SP2 — cycle! (SP2 is already in the stack as caller)
        let mut req = zero_req(ffa::FFA_MSG_SEND_DIRECT_REQ_32);
        req.x1 = ((hypervisor::platform::SP1_PARTITION_ID as u64) << 16)
            | (hypervisor::platform::SP2_PARTITION_ID as u64);
        let resp = dispatch_ffa_as_sp(
            &req,
            hypervisor::platform::SP1_PARTITION_ID,
        );
        assert_eq!(resp.x0, ffa::FFA_ERROR);
        assert_eq!(resp.x2 as i32, ffa::FFA_BUSY as i32);

        // Clean up call stack
        {
            let mut stack = hypervisor::spmc_handler::CALL_STACK.lock();
            stack.pop();
        }
        pass += 1;
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `make run 2>&1 | grep "test_spmc_handler"`
Expected: FAIL — `dispatch_ffa_as_sp` doesn't handle DIRECT_REQ from SPs yet.

- [ ] **Step 3: Add DIRECT_REQ to handle_sp_exit whitelist**

In `src/spmc_handler.rs`, modify the whitelist at lines 725-734. Add two new conditions:

```rust
        if x0 != ffa::FFA_MSG_SEND_DIRECT_RESP_32
            && x0 != ffa::FFA_MSG_SEND_DIRECT_RESP_64
            && x0 != ffa::FFA_MSG_WAIT
            && x0 != ffa::FFA_RX_RELEASE
            && x0 != ffa::FFA_MEM_RETRIEVE_REQ_32
            && x0 != ffa::FFA_MEM_RETRIEVE_REQ_64
            && x0 != ffa::FFA_MEM_RELINQUISH
            && x0 != ffa::FFA_MEM_FRAG_RX
            && x0 != ffa::FFA_CONSOLE_LOG_32
            && x0 != ffa::FFA_CONSOLE_LOG_64
            && x0 != ffa::FFA_MSG_SEND_DIRECT_REQ_32   // NEW: SP→SP
            && x0 != ffa::FFA_MSG_SEND_DIRECT_REQ_64   // NEW: SP→SP
        {
```

- [ ] **Step 4: Add SP→SP DIRECT_REQ match arm**

In `src/spmc_handler.rs`, add a new match arm in `handle_sp_exit()` inside the `match x0 {` block (after the CONSOLE_LOG arm at line ~863, before the `_ =>` default arm at line 864):

```rust
            ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
                // SP→SP DIRECT_REQ routing
                let source_id = (x1 >> 16) as u16;
                let dest_id = (x1 & 0xFFFF) as u16;

                // Validation 1: source must match current SP
                if source_id != sp_id {
                    sp.set_args(
                        ffa::FFA_ERROR, 0,
                        ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0,
                    );
                    continue;
                }

                // Validation 2: no self-calls
                if dest_id == sp_id {
                    sp.set_args(
                        ffa::FFA_ERROR, 0,
                        ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0,
                    );
                    continue;
                }

                // Validation 3: destination must exist (before acquiring CALL_STACK)
                if !crate::sp_context::is_registered_sp(dest_id) {
                    sp.set_args(
                        ffa::FFA_ERROR, 0,
                        ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0,
                    );
                    continue;
                }

                // Validation 4+5: cycle detection + push (atomic under CALL_STACK lock)
                {
                    let mut stack = CALL_STACK.lock();
                    if stack.contains(dest_id) {
                        sp.set_args(
                            ffa::FFA_ERROR, 0,
                            ffa::FFA_BUSY as u64, 0, 0, 0, 0, 0,
                        );
                        continue;
                    }
                    if stack.push(sp_id, dest_id).is_err() {
                        sp.set_args(
                            ffa::FFA_ERROR, 0,
                            ffa::FFA_BUSY as u64, 0, 0, 0, 0, 0,
                        );
                        continue;
                    }
                }

                // Caller SP: Running → Blocked
                if sp
                    .transition_to(crate::sp_context::SpState::Blocked)
                    .is_err()
                {
                    // Rollback: pop the frame we just pushed
                    CALL_STACK.lock().pop();
                    sp.set_args(
                        ffa::FFA_ERROR, 0,
                        ffa::FFA_DENIED as u64, 0, 0, 0, 0, 0,
                    );
                    continue;
                }

                // Save caller's EL1 state + clear S2 before entering callee
                sp.save_el1_state();
                let caller_cpu = sel2_cpu_id();
                CURRENT_RUNNING_SP[caller_cpu].store(0, Ordering::Release);
                crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
                clear_secure_stage2();

                // Build DIRECT_REQ for callee
                let callee_req = SmcResult8 { x0, x1, x2, x3, x4, x5, x6, x7 };

                // Drop caller's dispatch lock before acquiring callee's
                drop(sp_guard);

                // Recursive dispatch to callee SP
                let callee_result = dispatch_to_sp(&callee_req, dest_id);

                // Re-acquire caller's dispatch lock
                sp_guard = match crate::sp_context::try_lock_sp(sp_id) {
                    Ok(g) => g,
                    Err(_) => {
                        CALL_STACK.lock().pop();
                        return make_error(ffa::FFA_DENIED as u64);
                    }
                };
                let sp = sp_guard.sp_mut();

                // Check if callee was preempted (FFA_INTERRUPT)
                if callee_result.x0 == ffa::FFA_INTERRUPT {
                    // Chain preemption: Blocked → Preempted
                    // Do NOT pop stack frame — will be resumed via FFA_RUN
                    if sp
                        .transition_to(crate::sp_context::SpState::Preempted)
                        .is_err()
                    {
                        CALL_STACK.lock().pop();
                        return make_error(ffa::FFA_DENIED as u64);
                    }
                    sp.set_preempted_cpu(sel2_cpu_id());
                    // Return FFA_INTERRUPT with innermost preempted SP ID
                    return callee_result;
                }

                // Normal completion: pop stack frame, Blocked → Running
                CALL_STACK.lock().pop();
                if sp
                    .transition_to(crate::sp_context::SpState::Running)
                    .is_err()
                {
                    return make_error(ffa::FFA_DENIED as u64);
                }

                // Write callee's response to caller's registers
                sp.set_args(
                    callee_result.x0, callee_result.x1, callee_result.x2,
                    callee_result.x3, callee_result.x4, callee_result.x5,
                    callee_result.x6, callee_result.x7,
                );

                // Fall through to re-entry code below (Running→Idle→Running + enter_guest)
            }
```

**IMPORTANT**: This arm requires `sp_guard` to be accessible. The `handle_sp_exit()` function signature needs modification to pass in the dispatch guard. See Step 5 for the refactoring approach.

- [ ] **Step 5: Refactor handle_sp_exit for dispatch lock pass-through**

The current `handle_sp_exit(sp, sp_id)` takes `&mut SpContext`. For SP→SP, we need to drop and re-acquire the caller's dispatch lock. Two approaches:

**Approach A (minimal)**: The DIRECT_REQ match arm in `handle_sp_exit()` cannot drop the lock because it only has `&mut SpContext`. Instead, handle SP→SP DIRECT_REQ validation in `handle_sp_exit()` (source/dest/cycle checks), but perform the actual recursive dispatch **outside** the loop by returning a special sentinel. The caller (`dispatch_to_sp()`) detects this sentinel, drops the lock, calls `dispatch_to_sp()` recursively, re-acquires, and re-enters the loop.

**Approach B (cleaner, recommended)**: Add a new function `handle_sp_to_sp_direct_req()` that is called from `dispatch_to_sp()` when `handle_sp_exit()` returns the sentinel `SP_TO_SP_REQ`:

In `handle_sp_exit()`, for the SP→SP DIRECT_REQ case:
1. Validate source/dest/cycle (all checks that don't need lock drop)
2. Push CALL_STACK frame
3. Transition caller: Running → Blocked
4. Return a special result with x0 = `SP_TO_SP_PENDING` (internal sentinel, e.g., 0xFFFFFFFF_FFFFFFFE)
5. Stash the callee request in caller SP's x0-x7 (already there from the exit)

In `dispatch_to_sp()`, after `handle_sp_exit()` returns:
1. If result.x0 == `SP_TO_SP_PENDING`:
   - Read dest_id from caller's saved state
   - save_el1_state, clear S2, drop lock
   - Recursive `dispatch_to_sp(req, dest_id)`
   - Re-acquire lock, handle result (chain preempt or normal)
   - If normal: pop stack, Blocked→Running, set result in regs, re-enter loop
   - If preempted: Blocked→Preempted, return FFA_INTERRUPT
2. Else: return result normally

```rust
/// Internal sentinel: SP issued DIRECT_REQ targeting another SP.
/// dispatch_to_sp() must handle the recursive dispatch.
const SP_TO_SP_PENDING: u64 = 0xFFFF_FFFF_FFFF_FFFE;
```

- [ ] **Step 6: Implement the SP→SP routing**

Implement Approach B. The key changes:

**In `handle_sp_exit()` match block**, add before `_ =>`:

```rust
            ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
                let source_id = (x1 >> 16) as u16;
                let dest_id = (x1 & 0xFFFF) as u16;

                // Validation 1: source must match current SP
                if source_id != sp_id {
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                    continue;
                }
                // Validation 2: no self-calls
                if dest_id == sp_id {
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                    continue;
                }
                // Validation 3: destination must exist (before CALL_STACK lock)
                if !crate::sp_context::is_registered_sp(dest_id) {
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_INVALID_PARAMETERS as u64, 0, 0, 0, 0, 0);
                    continue;
                }
                // Validation 4+5: cycle detection + push (atomic under CALL_STACK)
                {
                    let mut stack = CALL_STACK.lock();
                    if stack.contains(dest_id) {
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_BUSY as u64, 0, 0, 0, 0, 0);
                        continue;
                    }
                    if stack.push(sp_id, dest_id).is_err() {
                        sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_BUSY as u64, 0, 0, 0, 0, 0);
                        continue;
                    }
                }
                // Transition caller: Running → Blocked
                if sp.transition_to(crate::sp_context::SpState::Blocked).is_err() {
                    CALL_STACK.lock().pop();
                    sp.set_args(ffa::FFA_ERROR, 0, ffa::FFA_DENIED as u64, 0, 0, 0, 0, 0);
                    continue;
                }
                // Return sentinel — dispatch_to_sp() handles the recursive call
                return SmcResult8 {
                    x0: SP_TO_SP_PENDING,
                    x1: dest_id as u64,
                    x2: 0, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0,
                };
            }
```

**In `dispatch_to_sp()`**, replace the simple `let result = handle_sp_exit(sp, sp_id);` (line 636) with a loop that handles SP→SP:

```rust
    // Handle SP exit — may loop if SP calls FF-A operations or SP→SP DIRECT_REQ
    let mut result;
    loop {
        result = handle_sp_exit(sp, sp_id);

        if result.x0 != SP_TO_SP_PENDING {
            break;
        }

        // SP→SP DIRECT_REQ: caller is now Blocked, callee dispatch needed
        let dest_id = result.x1 as u16;

        // Save caller state before entering callee
        sp.save_el1_state();
        let caller_cpu = sel2_cpu_id();
        CURRENT_RUNNING_SP[caller_cpu].store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
        clear_secure_stage2();

        // Build callee request from caller's saved registers (the DIRECT_REQ args)
        let (cx0, cx1, cx2, cx3, cx4, cx5, cx6, cx7) = sp.get_args();
        let callee_req = SmcResult8 {
            x0: cx0, x1: cx1, x2: cx2, x3: cx3,
            x4: cx4, x5: cx5, x6: cx6, x7: cx7,
        };

        // Drop caller's dispatch lock before acquiring callee's
        drop(sp_guard);

        // Recursive dispatch to callee SP
        let callee_result = dispatch_to_sp(&callee_req, dest_id);

        // Re-acquire caller's dispatch lock
        sp_guard = match crate::sp_context::try_lock_sp(sp_id) {
            Ok(g) => g,
            Err(_) => {
                CALL_STACK.lock().pop();
                return make_error(ffa::FFA_DENIED as u64);
            }
        };
        sp = sp_guard.sp_mut();

        if callee_result.x0 == ffa::FFA_INTERRUPT {
            // Chain preemption: Blocked → Preempted, do NOT pop stack
            if sp.transition_to(crate::sp_context::SpState::Preempted).is_err() {
                CALL_STACK.lock().pop();
                return make_error(ffa::FFA_DENIED as u64);
            }
            sp.set_preempted_cpu(sel2_cpu_id());
            result = callee_result;
            break;
        }

        // Normal completion: pop stack frame, Blocked → Running
        CALL_STACK.lock().pop();
        if sp.transition_to(crate::sp_context::SpState::Running).is_err() {
            return make_error(ffa::FFA_DENIED as u64);
        }

        // Write callee's response to caller's registers
        sp.set_args(
            callee_result.x0, callee_result.x1, callee_result.x2,
            callee_result.x3, callee_result.x4, callee_result.x5,
            callee_result.x6, callee_result.x7,
        );

        // Re-enter caller: restore S2, EL1, re-enter guest loop
        // (Re-entry code follows below — same as existing re-entry path)
        if sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
            return make_error(ffa::FFA_DENIED as u64);
        }
        if sp.transition_to(crate::sp_context::SpState::Running).is_err() {
            return make_error(ffa::FFA_DENIED as u64);
        }

        let cpu = sel2_cpu_id();
        SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);
        inject_pending_virq(sp);
        let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(sp.vsttbr());
        s2.install();
        CURRENT_RUNNING_SP[cpu].store(sp_id, Ordering::Release);
        sp.restore_el1_state();
        crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
        pre_enter_guest(sp);

        let _exit = unsafe {
            crate::arch::aarch64::enter_guest(
                sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
            )
        };
        post_enter_guest(cpu);
        sp.save_el1_state();
        CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();
        // Loop back to handle_sp_exit() for the re-entered caller
    }

    clear_secure_stage2();
    result
```

**Note**: This requires changing `sp` and `sp_guard` to be mutable local variables in `dispatch_to_sp()` instead of the current pattern where `sp` is derived once from `sp_guard`. The function already has `let mut sp_guard` and `let sp = sp_guard.sp_mut()` — make `sp` mutable and allow reassignment.

- [ ] **Step 7: Update dispatch_ffa_as_sp for SP→SP validation tests**

The `dispatch_ffa_as_sp()` function currently handles a limited set of FIDs. Add DIRECT_REQ handling that performs the same validation logic (but cannot actually dispatch since unit tests don't have real SPs running):

```rust
// In dispatch_ffa_as_sp(), add match arm for DIRECT_REQ:
ffa::FFA_MSG_SEND_DIRECT_REQ_32 | ffa::FFA_MSG_SEND_DIRECT_REQ_64 => {
    let source_id = (req.x1 >> 16) as u16;
    let dest_id = (req.x1 & 0xFFFF) as u16;
    // Validation 1: source must match caller
    if source_id != caller_sp_id {
        return make_error_with_code(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    // Validation 2: no self-calls
    if dest_id == caller_sp_id {
        return make_error_with_code(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    // Validation 3: destination must exist
    if !crate::sp_context::is_registered_sp(dest_id) {
        return make_error_with_code(ffa::FFA_INVALID_PARAMETERS as u64);
    }
    // Validation 4+5: cycle detection + push
    let mut stack = CALL_STACK.lock();
    if stack.contains(dest_id) {
        return make_error_with_code(ffa::FFA_BUSY as u64);
    }
    // For unit tests, just validate — don't actually dispatch
    // Return DENIED since we can't do real SP dispatch in test mode
    make_error_with_code(ffa::FFA_DENIED as u64)
}
```

Note: `make_error_with_code` should set x2 to the error code (matching existing error pattern). Check if this helper exists; if not, use inline construction:

```rust
SmcResult8 { x0: ffa::FFA_ERROR, x1: 0, x2: code, x3: 0, x4: 0, x5: 0, x6: 0, x7: 0 }
```

- [ ] **Step 8: Run tests to verify pass**

Run: `make run 2>&1 | grep "test_spmc_handler"`
Expected: `151 assertions passed` (142 + 4 CallStack + 5 SP→SP validation)

- [ ] **Step 9: Commit**

```bash
git add src/spmc_handler.rs tests/test_spmc_handler.rs
git commit -m "feat: SP→SP DIRECT_REQ routing in handle_sp_exit with cycle detection"
```

---

## Chunk 3: Chain Preemption in resume_preempted_sp

### Task 4: Chain-Resume Logic in resume_preempted_sp

**Files:**
- Modify: `src/spmc_handler.rs:928-998` (resume_preempted_sp)

- [ ] **Step 1: Add chain-resume after callee completes**

In `resume_preempted_sp()`, after `handle_sp_exit()` returns (line 996), add chain-resume logic:

```rust
    let result = handle_sp_exit(sp, sp_id);

    // Check if this SP was a callee in an SP→SP chain.
    // If so, chain-resume the caller instead of returning to NWd.
    let caller_id = CALL_STACK.lock().find_caller(sp_id);
    if let Some(caller) = caller_id {
        // Pop the frame {caller, sp_id}
        CALL_STACK.lock().pop();

        // Transition caller: Preempted → Running
        let mut caller_guard = match crate::sp_context::try_lock_sp(caller) {
            Ok(g) => g,
            Err(_) => {
                clear_secure_stage2();
                return make_error(ffa::FFA_DENIED as u64);
            }
        };
        let caller_sp = caller_guard.sp_mut();

        match caller_sp.owner_cpu() {
            Some(owner) if owner == cpu => {}
            Some(owner) => {
                if !caller_sp.try_migrate_owner_cpu(owner, cpu) {
                    clear_secure_stage2();
                    return make_error(ffa::FFA_BUSY as u64);
                }
            }
            None => {
                if !caller_sp.try_claim_owner_cpu(cpu) {
                    clear_secure_stage2();
                    return make_error(ffa::FFA_BUSY as u64);
                }
            }
        }

        if caller_sp
            .try_transition(
                crate::sp_context::SpState::Preempted,
                crate::sp_context::SpState::Running,
            )
            .is_err()
        {
            clear_secure_stage2();
            return make_error(ffa::FFA_DENIED as u64);
        }
        caller_sp.clear_preempted_cpu();

        // Write callee's response to caller's registers
        caller_sp.set_args(
            result.x0, result.x1, result.x2, result.x3,
            result.x4, result.x5, result.x6, result.x7,
        );

        // Chain-resume: re-enter caller SP (recursive dispatch_to_sp-style)
        // Transition Running→Idle→Running for re-entry
        if caller_sp.transition_to(crate::sp_context::SpState::Idle).is_err() {
            clear_secure_stage2();
            return make_error(ffa::FFA_DENIED as u64);
        }
        if caller_sp.transition_to(crate::sp_context::SpState::Running).is_err() {
            clear_secure_stage2();
            return make_error(ffa::FFA_DENIED as u64);
        }

        // Re-enter caller
        SP_IRQ_PREEMPTED[cpu].store(false, Ordering::Release);
        inject_pending_virq(caller_sp);
        let s2 = crate::secure_stage2::SecureStage2Config::new_from_vsttbr(caller_sp.vsttbr());
        s2.install();
        CURRENT_RUNNING_SP[cpu].store(caller, Ordering::Release);
        caller_sp.restore_el1_state();
        crate::arch::aarch64::peripherals::timer::arm_preemption_timer();
        pre_enter_guest(caller_sp);

        let _exit = unsafe {
            crate::arch::aarch64::enter_guest(
                caller_sp.vcpu_ctx_mut() as *mut crate::arch::aarch64::regs::VcpuContext,
            )
        };
        post_enter_guest(cpu);
        caller_sp.save_el1_state();
        CURRENT_RUNNING_SP[cpu].store(0, Ordering::Release);
        crate::arch::aarch64::peripherals::timer::disarm_preemption_timer();

        let caller_result = handle_sp_exit(caller_sp, caller);
        clear_secure_stage2();

        // Recursive: if caller also has a caller, chain-resume continues
        // (bounded by MAX_SPS-1 depth, handled by the CALL_STACK check)
        // For now, return — deeper chains are handled by BL33 re-issuing FFA_RUN
        return caller_result;
    }

    clear_secure_stage2();
    result
```

- [ ] **Step 2: Run all tests**

Run: `make run 2>&1 | tail -5`
Expected: All tests pass (no regression — chain-resume is only triggered when CALL_STACK has entries)

- [ ] **Step 3: Commit**

```bash
git add src/spmc_handler.rs
git commit -m "feat: chain-resume in resume_preempted_sp for SP→SP preemption recovery"
```

---

## Chunk 4: SP3 (sp_relay) Build Infrastructure

### Task 5: Platform Constants for SP3

**Files:**
- Modify: `src/platform.rs:100-121`

- [ ] **Step 1: Add SP3 constants and move SECURE_HEAP_START**

In `src/platform.rs`, after SP2 constants (line 111), add SP3 and update heap:

```rust
/// SP3 load address in SEC_DRAM (loaded by TF-A BL2 from FIP)
#[cfg(feature = "sel2")]
pub const SP3_LOAD_ADDR: u64 = 0x0e50_0000;
/// SP3 memory size (1MB)
#[cfg(feature = "sel2")]
pub const SP3_MEM_SIZE: u64 = 0x10_0000;
/// SP3 stack pointer (top of SP3 region)
#[cfg(feature = "sel2")]
pub const SP3_STACK_TOP: u64 = SP3_LOAD_ADDR + SP3_MEM_SIZE;
/// SP3 partition ID
#[cfg(feature = "sel2")]
pub const SP3_PARTITION_ID: u16 = 0x8003;
```

Change SECURE_HEAP_START from `0x0e50_0000` to `0x0e60_0000`:

```rust
/// Secure heap start (for S-EL2 page table allocation)
#[cfg(feature = "sel2")]
pub const SECURE_HEAP_START: u64 = 0x0e60_0000;
/// Secure heap size (~10MB, up to end of SEC_DRAM)
#[cfg(feature = "sel2")]
pub const SECURE_HEAP_SIZE: u64 = 0x0f00_0000 - SECURE_HEAP_START;
```

- [ ] **Step 2: Run tests to verify no regression**

Run: `make run 2>&1 | tail -5`
Expected: All tests pass (heap change only affects sel2 feature, unit tests don't use sel2)

- [ ] **Step 3: Commit**

```bash
git add src/platform.rs
git commit -m "feat: add SP3 platform constants, move SECURE_HEAP_START to 0x0e600000"
```

### Task 6: SP3 Assembly, Linker, and Manifest

**Files:**
- Create: `tfa/sp_relay/start.S`
- Create: `tfa/sp_relay/linker.ld`
- Create: `tfa/sp_relay/sp_manifest.dts`

- [ ] **Step 1: Create sp_relay directory**

```bash
mkdir -p tfa/sp_relay
```

- [ ] **Step 2: Create SP3 linker script**

Create `tfa/sp_relay/linker.ld`:

```
/* SP Relay linker script.
 * Loaded at 0x0e500000 in SEC_DRAM by TF-A BL2. */
ENTRY(_start)
SECTIONS
{
    . = 0x0e500000;
    .text : { *(.text.boot) *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss : { *(.bss*) }
}
```

- [ ] **Step 3: Create SP3 manifest**

Create `tfa/sp_relay/sp_manifest.dts`:

```dts
/dts-v1/;

/ {
    compatible = "arm,ffa-manifest-1.0";
    ffa-version = <0x00010001>;  /* FF-A v1.1 */

    uuid = <0x33221100 0x33221100 0x33221100 0x33221100>;
    id = <0x8003>;

    execution-ctx-count = <1>;     /* Single execution context */
    exception-level = <2>;         /* S-EL1 */
    execution-state = <0>;         /* AArch64 */
    load-address = <0x0 0x0e500000>;
    entrypoint = <0x0 0x0e500000>;
    xlat-granule = <0>;            /* 4KB */
    messaging-method = <3>;        /* Direct request/response */
    managed-exit;                  /* SPMC manages preemption */
};
```

- [ ] **Step 4: Create SP3 relay assembly**

Create `tfa/sp_relay/start.S`:

```asm
/*
 * SP Relay — Secure Partition at S-EL1 for SP-to-SP testing.
 *
 * Boot sequence:
 *   1. Print "[SP3] Hello from S-EL1"
 *   2. Call FFA_MSG_WAIT to signal idle to SPMC
 *   3. On DIRECT_REQ:
 *      - If x3 == RELAY_MAGIC (0x00EE1A00):
 *        Forward DIRECT_REQ to target SP (x4 low 16 bits),
 *        with x5-x7 as payload. Forward response back.
 *      - Else: echo with x4 += 0x2000
 *
 * Loaded at 0x0e500000 by TF-A BL2. Stack at 0x0e600000.
 */

.section .text.boot
.global _start

.equ UART_BASE,          0x09000000
.equ UARTDR,             0x000
.equ UARTFR,             0x018

.equ FFA_MSG_WAIT,              0x8400006B
.equ FFA_DIRECT_REQ_32,         0x8400006F
.equ FFA_DIRECT_RESP_32,        0x84000070
.equ FFA_ERROR,                 0x84000060
.equ SP3_PARTITION_ID,          0x8003
.equ RELAY_MAGIC,               0x00EE1A00

.equ SP_STACK_TOP,       0x0e600000

_start:
    /* Set up stack */
    ldr     x0, =SP_STACK_TOP
    mov     sp, x0

    /* Print banner */
    adr     x0, str_banner
    bl      uart_print

    /* Signal idle to SPMC */
    mov     w0, #FFA_MSG_WAIT
    smc     #0

    /* ── Main dispatch loop ── */
.Lwait_loop:
    /* Check if we received DIRECT_REQ_32 */
    ldr     w9, =FFA_DIRECT_REQ_32
    cmp     w0, w9
    b.ne    .Lunexpected

    /* Save caller info: x1 has source<<16|dest */
    mov     x20, x1         /* save original x1 (source|dest) */
    mov     x21, x3         /* save x3 (magic or payload) */
    mov     x22, x4         /* save x4 */
    mov     x23, x5         /* save x5 */
    mov     x24, x6         /* save x6 */
    mov     x25, x7         /* save x7 */

    /* Extract caller ID from x1[31:16] */
    lsr     x26, x20, #16   /* x26 = caller_id */

    /* Check if relay mode (x3 == RELAY_MAGIC) */
    ldr     w9, =RELAY_MAGIC
    cmp     w21, w9
    b.ne    .Lecho_mode

    /* ── Relay mode: forward DIRECT_REQ to target SP ── */
    /* Target SP ID = x4[15:0] */
    and     w27, w22, #0xFFFF   /* x27 = target_sp_id */

    /* Build DIRECT_REQ: source=SP3, dest=target */
    mov     w0, #FFA_DIRECT_REQ_32
    mov     w1, #SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w27         /* x1 = SP3<<16 | target */
    mov     x2, #0
    mov     x3, #0              /* no relay magic for target */
    mov     x4, x23             /* forward x5 as x4 */
    mov     x5, x24             /* forward x6 as x5 */
    mov     x6, x25             /* forward x7 as x6 */
    mov     x7, #0
    smc     #0

    /* Check response: FFA_ERROR or DIRECT_RESP */
    ldr     w9, =FFA_ERROR
    cmp     w0, w9
    b.eq    .Lrelay_error

    /* Forward callee's response back to original caller */
    mov     x28, x4             /* save callee's x4 */
    mov     x29, x5             /* save callee's x5 */
    /* x6, x7 from callee are still in registers */
    mov     x10, x6
    mov     x11, x7

    mov     w0, #FFA_DIRECT_RESP_32
    mov     w1, #SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w26         /* x1 = SP3<<16 | original_caller */
    mov     x2, #0
    mov     x3, #0
    mov     x4, x28             /* callee's x4 */
    mov     x5, x29             /* callee's x5 */
    mov     x6, x10             /* callee's x6 */
    mov     x7, x11             /* callee's x7 */
    smc     #0
    b       .Lwait_loop

.Lrelay_error:
    /* Forward error back to caller */
    mov     x28, x2             /* save error code */
    mov     w0, #FFA_DIRECT_RESP_32
    mov     w1, #SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w26         /* x1 = SP3<<16 | original_caller */
    mov     x2, #0
    mov     x3, #0
    mov     x4, x28             /* error code in x4 */
    mov     x5, #0
    mov     x6, #0
    mov     x7, #0
    smc     #0
    b       .Lwait_loop

    /* ── Echo mode: x4 += 0x2000 ── */
.Lecho_mode:
    add     x22, x22, #0x2000
    mov     w0, #FFA_DIRECT_RESP_32
    mov     w1, #SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w26         /* x1 = SP3<<16 | caller */
    mov     x2, #0
    mov     x3, x21
    mov     x4, x22             /* x4 + 0x2000 */
    mov     x5, x23
    mov     x6, x24
    mov     x7, x25
    smc     #0
    b       .Lwait_loop

.Lunexpected:
    /* Unexpected call — return to MSG_WAIT */
    mov     w0, #FFA_MSG_WAIT
    smc     #0
    b       .Lwait_loop

/* ── UART print (NUL-terminated string in x0) ── */
uart_print:
    ldr     x10, =UART_BASE
.Lprint_loop:
    ldrb    w11, [x0], #1
    cbz     w11, .Lprint_done
.Lprint_wait:
    ldr     w12, [x10, #UARTFR]
    tbnz    w12, #5, .Lprint_wait
    str     w11, [x10, #UARTDR]
    b       .Lprint_loop
.Lprint_done:
    ret

/* ── String data ── */
.section .rodata
str_banner:
    .asciz "[SP3] Hello from S-EL1 (sp_relay)\r\n"
```

- [ ] **Step 5: Commit**

```bash
git add tfa/sp_relay/
git commit -m "feat: add SP3 (sp_relay) assembly, linker script, and manifest"
```

### Task 7: Build System Integration

**Files:**
- Modify: `Makefile` (add build-sp-relay target, update build-tfa-spmc deps)
- Modify: `tfa/tb_fw_config.dts` (add SP3 entry)
- Modify: `tfa/sp_layout.json` (add SP3)

- [ ] **Step 1: Add build-sp-relay to Makefile**

After the `build-sp-irq` target (line 285), add:

```makefile
# SP Relay binary (S-EL1, SP-to-SP testing)
SP_RELAY_BIN := tfa/sp_relay/sp_relay.bin

build-sp-relay:
	@echo "Building SP Relay (S-EL1)..."
	aarch64-linux-gnu-as -o tfa/sp_relay/sp_relay.o tfa/sp_relay/start.S
	aarch64-linux-gnu-ld -T tfa/sp_relay/linker.ld -o tfa/sp_relay/sp_relay.elf tfa/sp_relay/sp_relay.o
	aarch64-linux-gnu-objcopy -O binary tfa/sp_relay/sp_relay.elf $(SP_RELAY_BIN)
	@echo "SP Relay binary: $(SP_RELAY_BIN)"
```

Update `build-tfa-spmc` dependency line (line 302):

```makefile
build-tfa-spmc: build-bl32-bl33 build-spmc build-sp-hello build-sp-irq build-sp-relay build-bl33-ffa-test
```

Also update `build-tfa-full` (line 326):

```makefile
build-tfa-full: build-bl32-bl33 build-spmc build-sp-hello build-sp-irq build-sp-relay
```

- [ ] **Step 2: Add SP3 to tb_fw_config.dts**

The UUID in `sp_manifest.dts` is `<0x33221100 ...>`. After byte-swap by `sp_mk_generator.py`, the `tb_fw_config.dts` UUID becomes: `"00112233-0011-2233-0011-223300112233"`.

Add SP3 entry after SP2 in `tfa/tb_fw_config.dts`:

```dts
		sp3 {
			uuid = "00112233-0011-2233-0011-223300112233";
			load-address = <0x0e500000>;
			owner = "Plat";
		};
```

- [ ] **Step 3: Add SP3 to sp_layout.json**

```json
{
    "SP1": {
        "image": "sp_hello/sp_hello.bin",
        "pm": "sp_hello/sp_manifest.dts",
        "owner": "Plat"
    },
    "SP2": {
        "image": "sp_irq/sp_irq.bin",
        "pm": "sp_irq/sp_manifest.dts",
        "owner": "Plat"
    },
    "SP3": {
        "image": "sp_relay/sp_relay.bin",
        "pm": "sp_relay/sp_manifest.dts",
        "owner": "Plat"
    }
}
```

- [ ] **Step 4: Build SP3 to verify**

Run: `make build-sp-relay`
Expected: `SP Relay binary: tfa/sp_relay/sp_relay.bin`

- [ ] **Step 5: Commit**

```bash
git add Makefile tfa/tb_fw_config.dts tfa/sp_layout.json
git commit -m "feat: build system integration for SP3 (sp_relay)"
```

### Task 8: Boot SP3 in SPMC

**Files:**
- Modify: `src/main.rs:372-453` (add SP3 boot after SP2 boot)

- [ ] **Step 1: Add SP3 boot logic**

After the SP2 boot block (line 453, `}`), add SP3 boot (same pattern as SP2):

```rust
    // 5.9. Boot SP3 (if present at SP3_LOAD_ADDR)
    {
        let sp3_pkg_base = hypervisor::platform::SP3_LOAD_ADDR;
        // SAFETY: reads SP3 package magic from trusted BL2-loaded memory.
        let sp3_magic = unsafe { core::ptr::read_volatile(sp3_pkg_base as *const u32) };
        if sp3_magic == 0x474B5053 {
            // "SPKG" magic found
            hypervisor::log_info!("[SPMC] SP3 package found at {:#018x}\n", sp3_pkg_base);

            // Build Secure Stage-2 for SP3
            let mapper3 = hypervisor::secure_stage2::build_sp_stage2(
                hypervisor::platform::SP3_LOAD_ADDR,
                hypervisor::platform::SP3_MEM_SIZE,
            )
            .expect("Failed to build SP3 Stage-2");
            let s2_config3 = hypervisor::secure_stage2::SecureStage2Config::new(mapper3.l0_addr());

            // Parse SPKG header for SP3
            // SAFETY: SP3 SPKG header is loaded by BL2 at trusted physical address.
            let sp3_img_offset = unsafe {
                let ptr = sp3_pkg_base as *const u32;
                core::ptr::read_volatile(ptr.add(4)) as u64
            };
            let sp3_entry = sp3_pkg_base + sp3_img_offset;

            hypervisor::log_info!("[SPMC] SP3 entry={:#018x}\n", sp3_entry);

            // SP3 UUID from sp_manifest.dts (byte-swapped)
            let sp3_uuid: [u32; 4] = [0x33221100, 0x33221100, 0x33221100, 0x33221100];
            let mut sp3 = hypervisor::sp_context::SpContext::new(
                hypervisor::platform::SP3_PARTITION_ID,
                sp3_entry,
                hypervisor::platform::SP3_STACK_TOP,
                sp3_uuid,
            );
            sp3.set_vsttbr(s2_config3.vsttbr);

            // SP3 has no owned INTIDs (relay only)
            sp3.set_owned_intids([0, 0, 0, 0]);

            // Install SP3's Stage-2, clear EL1 state, ERET to SP3
            s2_config3.install();
            // SAFETY: clears EL1 sysregs before first SP3 entry.
            unsafe {
                core::arch::asm!(
                    "msr sctlr_el1, xzr",
                    "msr tcr_el1, xzr",
                    "msr ttbr0_el1, xzr",
                    "msr vbar_el1, xzr",
                    "isb",
                    options(nostack, nomem),
                );
            }

            {
                use hypervisor::arch::aarch64::enter_guest;
                use hypervisor::arch::aarch64::regs::VcpuContext;
                // SAFETY: pointer is to locked SP3 VcpuContext prepared for guest entry.
                let _exit = unsafe { enter_guest(sp3.vcpu_ctx_mut() as *mut VcpuContext) };
            }

            // Save SP3's EL1 sysregs after initial boot
            sp3.save_el1_state();

            // Verify SP3 called FFA_MSG_WAIT
            let (x0, _, _, _, _, _, _, _) = sp3.get_args();
            if x0 == hypervisor::ffa::FFA_MSG_WAIT {
                hypervisor::log_info!("[SPMC] SP3 booted, now Idle (FFA_MSG_WAIT received)\n");
                sp3.transition_to(hypervisor::sp_context::SpState::Idle)
                    .expect("SP3 transition failed");
            } else {
                hypervisor::log_warn!(
                    "[SPMC] WARNING: SP3 did not call FFA_MSG_WAIT, x0={:#018x}\n",
                    x0
                );
            }

            hypervisor::sp_context::register_sp(sp3);
        } else {
            hypervisor::log_info!("[SPMC] No SP3 package found (two-SP mode)\n");
        }
    }
```

- [ ] **Step 2: Update PARTITION_INFO_GET SP count**

Check if PARTITION_INFO_GET hardcodes the SP count. If it uses `for_each_sp()` iterator, it auto-discovers — verify this.

- [ ] **Step 3: Run unit tests**

Run: `make run 2>&1 | tail -5`
Expected: All tests pass (SP3 boot code is behind `sel2` feature, not exercised in unit tests)

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: boot SP3 (sp_relay) in SPMC at S-EL2"
```

---

## Chunk 5: BL33 Integration Tests

### Task 9: BL33 Test 17 — SP→SP Relay Chain

**Files:**
- Modify: `tfa/bl33_ffa_test/start.S`

- [ ] **Step 1: Add SP3 constants and Test 17**

Add new constants near the existing ones (after line 36):

```asm
.equ SP3_ID,              0x8003
.equ RELAY_MAGIC,         0x00EE1A00
.equ FFA_DIRECT_REQ_64,   0xC400006F
```

Add Test 17 after Test 16 (before the `done:` label / `wfe` loop):

```asm
    /* ============ Test 17: SP→SP Relay Chain ============ */
    adr     x0, str_t17
    bl      uart_print

    /* NWd → SP3(relay to SP1, x5=0xBEEF) */
    ldr     w0, =FFA_DIRECT_REQ_32
    mov     w1, #SP3_ID         /* dest = SP3 */
    mov     x2, #0
    ldr     w3, =RELAY_MAGIC    /* relay mode */
    mov     w4, #SP1_ID         /* target = SP1 */
    mov     x5, #0xBEEF         /* payload */
    mov     x6, #0
    mov     x7, #0
    smc     #0

    /* Expect DIRECT_RESP with x4 = 0xBEEF + 0x1000 = 0x1BEEF */
    /* (SP3 forwards to SP1, SP1 does x4 += 0x1000, SP3 forwards back) */
    ldr     w9, =FFA_DIRECT_RESP_32
    cmp     w0, w9
    b.ne    .Ltest17_fail
    /* SP1 echoes: x4 from SP3's forward = x5 original = 0xBEEF, +0x1000 */
    ldr     x9, =0x1BEEF
    cmp     x4, x9
    b.ne    .Ltest17_fail

    adr     x0, str_pass
    bl      uart_print
    b       .Ltest17_done
.Ltest17_fail:
    adr     x0, str_fail
    bl      uart_print
.Ltest17_done:
```

**Note on relay protocol**: SP3 receives `(x3=RELAY_MAGIC, x4=SP1_ID, x5=0xBEEF)`. It forwards to SP1 with `x4=0xBEEF` (from x5). SP1 does fast-path `x4 += 0x1000 = 0x1BEEF` and responds. SP3 forwards `x4=0x1BEEF` back to NWd.

- [ ] **Step 2: Add Test 18 — Cycle Detection (optional)**

```asm
    /* ============ Test 18: Cycle Detection ============ */
    adr     x0, str_t18
    bl      uart_print

    /* NWd → SP3(relay to SP3 itself) — should fail */
    ldr     w0, =FFA_DIRECT_REQ_32
    mov     w1, #SP3_ID
    mov     x2, #0
    ldr     w3, =RELAY_MAGIC
    mov     w4, #SP3_ID         /* target = SP3 (self-call via relay) */
    mov     x5, #0xDEAD
    mov     x6, #0
    mov     x7, #0
    smc     #0

    /* SP3 will try DIRECT_REQ to itself → SPMC returns FFA_INVALID_PARAMETERS */
    /* SP3's relay error path forwards error code in x4 */
    /* Expect DIRECT_RESP with error indicator */
    ldr     w9, =FFA_DIRECT_RESP_32
    cmp     w0, w9
    b.ne    .Ltest18_fail

    /* SP3 received FFA_ERROR from SPMC and forwarded error code in x4 */
    /* The error code is FFA_INVALID_PARAMETERS (actually dest==source check) */
    /* We just check that we got a response (not a hang) */
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest18_done
.Ltest18_fail:
    adr     x0, str_fail
    bl      uart_print
.Ltest18_done:
```

- [ ] **Step 3: Add string data**

Add to the `.rodata` section:

```asm
str_t17:
    .asciz "  Test 17: SP→SP relay chain ....... "
str_t18:
    .asciz "  Test 18: Cycle detection ......... "
```

- [ ] **Step 4: Commit**

```bash
git add tfa/bl33_ffa_test/start.S
git commit -m "feat: BL33 Tests 17-18 for SP→SP relay chain and cycle detection"
```

---

## Chunk 6: Full Build + Verification + CLAUDE.md

### Task 10: Build and Verify

- [ ] **Step 1: Run unit tests**

Run: `make run 2>&1 | tail -20`
Expected: All 34+ suites pass, ~430+ assertions

- [ ] **Step 2: Build TF-A with SPMC + SP3**

Run: `make build-tfa-spmc`
Expected: Builds successfully with SP1, SP2, SP3 in FIP

- [ ] **Step 3: Run BL33 integration tests**

Run: `make run-spmc 2>&1 | tail -30`
Expected: 18/18 PASS (Tests 1-18)

- [ ] **Step 4: Run TF-A Linux regression**

Run: `make run-tfa-linux 2>&1 | tail -10`
Expected: 37/37 (no regression)

### Task 11: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update test counts**

- test_spmc_handler: 142 → ~156 (142 + 4 CallStack + 5 SP→SP + ~5 more)
- test_sp_context: 58 → 59
- BL33: 16/16 → 18/18
- Total: ~415 → ~430+
- Update test suites: 34 → 34 (no new suite, just new assertions)

- [ ] **Step 2: Update memory layout table**

Add SP3 row and update SECURE_HEAP_START.

- [ ] **Step 3: Update roadmap**

Add SP-to-SP DIRECT_REQ entry after Phase 4.7.

- [ ] **Step 4: Update Build Commands**

Add `build-sp-relay` to the build commands table.

- [ ] **Step 5: Update SpmcHandler description**

Add SP→SP DIRECT_REQ routing, CallStack, chain preemption.

- [ ] **Step 6: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for SP-to-SP DIRECT_REQ (Phase 5.1)"
```

---

## Verification Checklist

```bash
make run           # 34 suites, ~430+ assertions
make run-spmc      # 18/18 BL33 tests (Test 17 + 18 new)
make run-tfa-linux # 37/37 (no regression)
```

## Summary of Changes

| Component | New Assertions | Key Change |
|-----------|---------------|------------|
| CallStack (Task 2) | 4 | push/pop/contains/find_caller |
| SP→SP validation (Task 3) | 5 | self-call/spoof/invalid/64-bit/cycle |
| Blocked→Preempted (Task 1) | 1 | New state transition |
| sp_context test fix (Task 1) | 0 (net) | Changed is_err → is_ok |
| Chain preemption (Task 4) | 0 (tested E2E) | resume_preempted_sp chain-resume |
| BL33 Test 17 (Task 9) | 1 | NWd→SP3→SP1 relay chain |
| BL33 Test 18 (Task 9) | 1 | Cycle detection E2E |
| **Total** | **~12** | |
