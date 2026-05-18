---
title: "When Rust's Exhaustive Match Helps (And When It Doesn't): Notes from a Bare-Metal Hypervisor"
published: true
description: "Exhaustive match helps for enum dispatch, not for (from,to) state-transition tables — and the reason involves AtomicU8 size constraints, not typestate."
tags: rust, embedded, arm, systems
series: "ARM64 Bare-Metal Hypervisor in Rust"
---

> **Disclaimer**: This is about an experimental hypervisor project that only runs on QEMU virt — no real-hardware validation yet. The lessons apply to "Rust's tooling edges in systems programming," not production guidance.

10 weeks into writing an ARM64 bare-metal hypervisor, I assumed Rust's exhaustive `match` would be the safety net when I extended my state machine. Two observations, from one week of commits: **exhaustive match didn't help my state machine at all, but caught 6 errors the one time I extended my `Device` enum.** This post is about why — and why the distinction is about cardinality, not typestate vs tag enums.

---

I'm writing an ARM64 bare-metal hypervisor. Part of it is a thing called a **Secure Partition (SP)** — a lightweight VM managed by the SPMC. Each SP has a lifecycle: Reset → Idle → Running → Blocked → Preempted. 5 states, 8 legal transitions.

Recently I added a new transition: `Blocked → Preempted`, for chain preemption between SPs. By the textbook, this is exactly the scenario where Rust's `enum + match` should shine: add a state/transition, the compiler finds every site that needs updating.

**The compiler said nothing.**

This post is about why I didn't use the "enum-with-fields" pattern you see in tutorials, why `match` exhaustiveness didn't help on this state machine, and where it actually *did* help.

---

## The Real Code

No toy examples. Here's the actual `SpState` from the repo:

```rust
// src/sp_context.rs
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

Classic **tag-only enum** — `#[repr(u8)]`, every variant is one byte, no payload. Why not the textbook `Running { entry_pc: u64 }` / `Preempted { saved_ctx: VcpuContext }`?

Because the state lives in an **`AtomicU8`**.

The SPMC runs on multiple physical CPUs. Different CPUs inside TF-A's SPMD (Secure Partition Manager Dispatcher) can route requests to the same SP at once. Two CPUs racing to do `Idle → Running` — one *must* lose, or both will `ERET` into the same SP and clobber register context.

CAS drives the race:

```rust
pub fn try_transition(&self, expected: SpState, new_state: SpState) -> Result<(), SpState> {
    match self.state.compare_exchange(
        expected as u8,
        new_state as u8,
        Ordering::AcqRel,   // success: publishes the winner's context-save
        Ordering::Acquire,  // failure: syncs whatever value we observed
    ) {
        Ok(_) => Ok(()),
        Err(actual) => Err(SpState::try_from(actual).expect("corrupt SP state value")),
    }
}
```

The constraint isn't memory layout — `#[repr(u8, C)]` on a fields-carrying enum does give stable layout. The real constraint is **size**: `AtomicU8` wraps one byte, and any enum with a `u64` payload is at least 8 bytes wide. Atomic `u64` CAS is fine on aarch64, but that means every state change either serializes through a fat struct CAS or falls back to a lock. I wanted single-byte CAS in the fast path, so the payload lives elsewhere (in a separate `VcpuContext` guarded by the state transition itself).

Side note on `expect("corrupt SP state value")`: it really does panic. In this project the panic handler halts the offending CPU and dumps state via UART — because if the `AtomicU8` ever holds a value outside `0..=4`, memory corruption has already happened and limping along is worse than stopping. That's a conscious choice for this binary, not a general bare-metal guideline.

---

## Why Exhaustive Match Didn't Help

The legal-transition check lives in one function:

```rust
// src/sp_context.rs
pub fn transition_to(&mut self, new_state: SpState) -> Result<(), &'static str> {
    let current = self.state();
    let valid = match (current, new_state) {
        (SpState::Reset, SpState::Idle) => true,
        (SpState::Idle, SpState::Running) => true,
        (SpState::Running, SpState::Idle) => true,
        (SpState::Running, SpState::Blocked) => true,
        (SpState::Blocked, SpState::Running) => true,
        (SpState::Blocked, SpState::Preempted) => true,  // ← the newly added line
        (SpState::Running, SpState::Preempted) => true,
        (SpState::Preempted, SpState::Running) => true,
        _ => false,
    };
    // ...
}
```

Note the final `_ => false`. This is **not** an exhaustive match — the wildcard swallows every unlisted combination as "illegal."

The state-table edit for `Blocked → Preempted` was a 1-line diff in `transition_to()` (the commit also updated `test_sp_context.rs` to cover the new transition). The compiler reported nothing on the production code, because to the compiler, all 25 `(from, to)` combinations were already covered (8 explicit + `_` fallback).

I could have replaced `_ => false` with all 17 illegal combinations enumerated. I started to — "exhaustive is more Rust-y". Then I gave up halfway:

```rust
// This way...
(SpState::Reset, SpState::Reset) => false,
(SpState::Reset, SpState::Running) => false,
(SpState::Reset, SpState::Blocked) => false,
// ... 15 more lines of this
```

No new information, and every future state addition means maintaining an N² table. `_ => false` *is* the documentation here: **what's listed is legal; everything else isn't.**

**Verdict**: For simple C-style enum + state-transition pairs, `match` exhaustiveness doesn't save you. Bugs at this layer can only be caught by unit tests (my `test_sp_context.rs` has 58 assertions covering most legal transitions, key illegal ones, and CAS success/failure semantics).

---

## Where It Actually Saved Me

The place where `match` exhaustiveness actually saved me was device dispatch.

My hypervisor uses a `Device` enum to enumerate all virtual devices. Every time the guest touches MMIO, a `match` dispatches to the right implementation:

```rust
// src/devices/mod.rs
pub enum Device {
    Uart(pl011::VirtualUart),
    Gicd(gic::VirtualGicd),
    Gicr(gic::VirtualGicr),
    VirtioBlk(virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>),
    VirtioNet(virtio::mmio::VirtioMmioTransport<virtio::net::VirtioNet>),
    Pl031(pl031::VirtualPl031),
}
```

This **is** a fields-carrying enum — each variant holds the state struct for its device. No `_` fallback on matches against it, because every variant has its own handler:

```rust
impl MmioDevice for Device {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        match self {
            Device::Uart(d) => d.read(offset, size),
            Device::Gicd(d) => d.read(offset, size),
            Device::Gicr(d) => d.read(offset, size),
            Device::VirtioBlk(d) => d.read(offset, size),
            Device::VirtioNet(d) => d.read(offset, size),
            Device::Pl031(d) => d.read(offset, size),
        }
    }
    // write, base_address, size, pending_irq, ack_irq ...
}
```

When I added `Pl031` (PL031 RTC) for Android boot, I only touched the enum definition. The compiler immediately fired **6 errors** — every site that `match`es against `Device` was missing the `Pl031` arm:

```text
error[E0004]: non-exhaustive patterns: `&Device::Pl031(_)` not covered
  --> src/devices/mod.rs:51:15
error[E0004]: non-exhaustive patterns: `&mut Device::Pl031(_)` not covered
  --> src/devices/mod.rs:62:15
error[E0004]: non-exhaustive patterns: `&Device::Pl031(_)` not covered
  --> src/devices/mod.rs:73:15
// ... 6 total
```

Two of those were dispatch sites I'd written months earlier and **completely forgotten about**. Had I used C `switch` without explicitly opting into `-Wswitch-enum` + `-Werror` (and refusing a `default` arm), those two sites would silently fall into `default` and return "unknown device." The guest would do any MMIO to the RTC, fail to find a device, and hang mid-boot with an error pointing somewhere completely unrelated.

You can absolutely get the same protection in C, but it takes deliberate setup. Linux defaults to `-Wswitch-default` (and only enables `-Wswitch-enum` at `W=3`), and `-Werror` is gated behind `CONFIG_WERROR`. So in practice, most C projects don't get this safety net by default; the ones that configure it correctly do. Rust makes the same check a precondition for compiling instead of a build-system setting you can drop. Worth more in a solo project, less in a shop with a strict style guide.

Either way, the compiler caught this bug instead of the guest doing so at boot time.

---

## When Exhaustive Match Actually Pays Off

Reviewing this state-machine extension + `Device` extension, here's my distilled rule:

**Exhaustive match saves you**: **fields-carrying enum + every variant has independent handler logic**.

- `Device::{Uart, Gicd, ..., Pl031}` — each device's `read/write` is totally different
- `MmioAccess::{Read { reg, size }, Write { reg, size, val }}` — read vs write semantics differ
- `ExitReason::{HvcCall, SmcCall, DataAbort, WfiWfe, ...}` — each exception class has its own handler

Common trait: **adding a variant potentially leaves gaps across the entire codebase**, and each gap's correct implementation is non-trivial (not just "error vs OK" binary output).

**Exhaustive match doesn't help**: **simple tag enum + cartesian-product check**.

- State machine `(from, to)` transition table — N² explosion, `_ => false` is more readable
- Permission matrix `(user_role, action)` — same
- Input sanity check `match(input) { valid_range => ..., _ => reject }` — tautological

These scenarios are "enumerate a small set of legal cases, reject everything else." `_ => fallback` loses no information — it's *more* readable.

---

## A Few Takeaways

**1. `#[repr(u8)]` is everyday life in hypervisor/kernel/driver code. Don't apologize for the atomic trade-off.**

Every time a "Rust state machine" tweet appears, someone in the replies recommends typestate. Typestate is genuinely powerful when transitions happen through owning APIs (`File::open → Handle<Open>`), but it doesn't compose with shared mutable state across CPUs — the entire point of `AtomicU8` is that multiple cores hold a reference to one byte. Typestate requires owning `self` by value to consume the old state; a multi-CPU SPMC can't do that on the fast path. Not a rejection of typestate, just the wrong tool for this edge.

**2. `_ => fallback` isn't a sin, but ask yourself every time.**

"If I add a new variant in the future, should this site force me to update it?"

- Yes → drop the `_`, enumerate every variant
- No (illegal state-machine pair, MMIO unknown-offset) → `_ => default` is documentation

**3. State-machine correctness is never a gift from Rust. It's a gift from tests + documentation + code review.**

My `test_sp_context.rs` has dedicated tests covering most legal transitions, key illegal ones, and CAS success/failure semantics. Rust didn't generate those; I wrote them. Rust saved me from some defensive code (no "sixth value" of `SpState` — `try_from_u8` rejects it), but whether the legal-transition table is *correct*, Rust has no opinion.

**4. What really saves you is "fields-carrying enum + each variant has its own handler."**

That's Rust's signature strength. Find the places in your codebase that fit this pattern and get them right — it pays more than agonizing over whether the state machine should be typestate-ified.

---

## Closing

My hypervisor isn't a "zero-unwrap" project. The `src/` tree has about 6 `unwrap()` calls (in boot-time paths that can't reasonably panic — `vm.rs`, `guest_loader.rs`); the `tests/` tree has more. There are dozens of `_ => default` fallback arms across the codebase, mostly in MMIO register decode for unknown offsets.

Every `unwrap()` and `_ =>` was a decision at the time, not laziness. Engineering beats slogans.

Rust gives you a good weapon. It doesn't think for you. Whether the state-transition table is legal is in *your* head, not the compiler's.

---

**Code**: [github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor)

**Blog**: [willamhou.github.io/hypervisor](https://willamhou.github.io/hypervisor/)

*This is part 5 of the ARM64 Hypervisor development series. The Chinese version is the canonical source — see [part5-enum-state-machine.md](https://github.com/willamhou/hypervisor/blob/main/docs/zhihu/part5-enum-state-machine.md).*
