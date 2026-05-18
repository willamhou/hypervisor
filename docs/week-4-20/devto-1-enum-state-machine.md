---
title: "How Rust's match found two bugs in my hypervisor state machine — and what it didn't find"
published: true
description: "A real example from ARM64 bare-metal, with the nuances tutorials skip"
tags: rust, embedded, arm, systemsprogramming
canonical_url: https://willamhou.github.io/hypervisor/
---

> Publish date: 4/22

I've been writing an ARM64 bare-metal hypervisor in Rust for 10 weeks. It manages "Secure Partitions" (SPs) — lightweight VMs with their own lifecycle: Reset → Idle → Running → Blocked → Preempted.

Last week I added a new state transition (`Blocked → Preempted`, for chain preemption). The Rust compiler caught two bugs I hadn't noticed. This post walks through it — and what `match` *didn't* help with. Because the "Rust magically finds all your bugs" narrative is oversold.

## Real code, not tutorial code

The actual `SpState` in my codebase:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SpState {
    Reset = 0,
    Idle = 1,
    Running = 2,
    Blocked = 3,
    Preempted = 4,
}
```

This is a **C-style enum** — no variant fields. Why not the tutorial-recommended `Running { entry_pc: u64 }` pattern?

Because I need `AtomicU8` storage. The SPMC runs on multiple CPUs; multiple cores may simultaneously try to dispatch the same SP. State updates require CAS (compare-and-swap). Rust atomics only support integer types. Fielded enums don't have `#[repr(C)]` or a stable memory layout, and can't be CAS'd.

This is a daily tradeoff in systems programming: **type expressiveness vs. hardware operation constraints**.

## Where match *didn't* help

State transitions live in a centralized function:

```rust
fn valid_transition(from: SpState, to: SpState) -> bool {
    match (from, to) {
        (SpState::Reset, SpState::Idle) => true,
        (SpState::Idle, SpState::Running) => true,
        (SpState::Running, SpState::Idle) => true,
        (SpState::Running, SpState::Blocked) => true,
        (SpState::Running, SpState::Preempted) => true,
        (SpState::Blocked, SpState::Running) => true,
        (SpState::Preempted, SpState::Running) => true,
        _ => false,
    }
}
```

Honest admission — there's a `_ => false` fallback here. This is NOT exhaustive. Why? Because listing every illegal transition (5×5 = 25 combinations, 18 of which are illegal) would be noise.

**Exhaustive match doesn't help here.** Bugs in the transition table can only be caught by tests.

## Where match *did* help

The useful exhaustiveness check is on SP exit events:

```rust
fn handle_sp_exit(sp: &mut SpContext, exit: ExitReason) -> DispatchResult {
    match exit {
        ExitReason::DirectResp { x4, x5, x6, x7 } => { /* return to caller */ }
        ExitReason::MemRetrieve { handle } => { /* handle locally, re-enter */ }
        ExitReason::MemRelinquish { handle } => { /* handle locally, re-enter */ }
        ExitReason::MemShare { descriptor } => { /* record, re-enter */ }
        ExitReason::ConsoleLog { ref buf } => { /* print, re-enter */ }
        ExitReason::DirectReq { target, .. } => { /* dispatch to target SP */ }
    }
}
```

`ExitReason` is a fielded enum, each variant carrying its own payload. No `_ =>` fallback.

When I added chain preemption, I added a new variant `IrqPreempt { saved_pc: u64 }`. The compiler immediately barked:

```
error[E0004]: non-exhaustive patterns: `IrqPreempt { .. }` not covered
   --> src/spmc_handler.rs:1163
```

Two call sites flagged, both real bugs:

- `handle_sp_exit` would treat IrqPreempt as unknown and drop the event
- `handle_sp_exit_as_caller` would treat the preempted SP as "direct_resp complete", corrupting caller state

Both bugs would have been very hard to reproduce at runtime (exact chain-preemption timing). Instead, they surfaced at compile time.

## Takeaways

1. **Exhaustive match shines on fielded enums.** Places like `match exit_reason`, where each variant has its own handling logic, make adding a variant safe.

2. **For Cartesian-product state transitions, match doesn't help.** You need tests and documentation.

3. **Atomic ops and type expressiveness are in tension.** You can't have both `Running { entry_pc: u64 }` and `AtomicU8::compare_exchange`.

4. **`_ =>` fallback is not a sin.** But every time you write one, ask: "if I added a new variant, should this change?" If yes, don't use `_ =>`.

## Honest disclaimer on Google + Rust + Android

While I'm here, a correction on a common claim: Google's Android uses Rust in the **Android Virtualization Framework** (crosvm, virtmgr, RMI bridging) and in **Pixel modem firmware**. The pKVM hypervisor itself — inside the Linux kernel — is still C. Don't conflate "Rust in Android" with "pKVM is Rust."

## Postscript

My hypervisor has 6 `unwrap()` calls and 45 `_ =>` fallbacks (mostly in MMIO register decode — unknown offsets return 0). It's not a "zero unwrap" project. But **every `unwrap()` and `_ =>` is a deliberate choice**, not laziness.

That's closer to the reality of systems programming than the "eliminate all unwraps" slogan: **tools, not dogma**.

---

Code: https://github.com/willamhou/hypervisor
Blog: https://willamhou.github.io/hypervisor/
