# Building a Rust ARM64 SPMC that Replaces Hafnium and Runs Beside pKVM

*A bare-metal Secure Partition Manager Core at S-EL2, written in `no_std` Rust, that boots Linux, manages Secure Partitions, and passes 35/35 end-to-end tests through real TF-A firmware.*

I built an ARM64 hypervisor that runs next to Android pKVM on the same chip.

pKVM owns the Normal world at `NS-EL2`. This project owns the Secure world at `S-EL2`. The two sides communicate through ARM's FF-A protocol, relayed by EL3 firmware. On the full stack, a Linux kernel module can send an FF-A request that crosses pKVM, TF-A, the SPMC, and a Secure Partition, then makes the whole trip back. Right now that path passes `35/35` end-to-end tests.

The Secure side already had a reference implementation: [Hafnium](https://hafnium.googlesource.com/hafnium/), maintained by Google and Arm. It is large, production-oriented, and written in C. I wanted something else: a smaller codebase I could understand line by line, with the type system helping in the parts where Secure-world control flow gets subtle.

So I rebuilt the Secure Partition Manager Core in about `30,000` lines of `no_std` Rust. It has one dependency, boots Linux to a BusyBox shell, manages three Secure Partitions, supports FF-A v1.1 messaging and memory sharing, and runs beside pKVM on four physical CPUs.

This is the version of the project I would want to read before clicking into the repo: what the system is, what was actually hard, and where Rust helped in ways that were more concrete than "memory safety is nice."

## What Makes This System Unusual

Modern ARM systems do not just have one privileged world. They have at least two:

```text
            Normal World          Secure World
           ┌────────────┐       ┌────────────┐
    EL0    │  Userspace  │       │            │
           ├────────────┤       ├────────────┤
    EL1    │ Linux/Android│      │  Secure    │
           │  kernel     │       │  Partitions│
           ├────────────┤       ├────────────┤
    EL2    │  pKVM       │       │  SPMC      │
           │  (NS-EL2)   │       │  (S-EL2)   │
           └──────┬──────┘       └──────┬──────┘
                  │      ┌──────┐       │
    EL3           └──────│ TF-A │───────┘
                         │ SPMD │
                         └──────┘
```

The Normal world runs Linux or Android under a hypervisor such as pKVM. The Secure world runs small trusted components called Secure Partitions. EL3 firmware, typically TF-A, sits above both worlds and brokers the handoff.

My project fills the Secure-world hypervisor slot at `S-EL2`. That role is called the Secure Partition Manager Core, or SPMC.

The interesting part is not just that it runs at `S-EL2`. It is that it has to coexist with another hypervisor on the same physical CPUs, while speaking a protocol that spans multiple privilege levels and world switches. Most hypervisor projects do not have to care about this shape at all.

The boot chain looks like this:

```text
TF-A BL1 -> BL2 -> BL31 (SPMD at EL3)
          -> BL32 (our SPMC at S-EL2)
          -> BL33 (pKVM at NS-EL2 -> Linux at NS-EL1)
```

And when Linux wants to talk to a Secure Partition, the path looks like this:

```text
Linux (NS-EL1) -> pKVM (NS-EL2) -> TF-A/SPMD (EL3)
               -> SPMC (S-EL2) -> Secure Partition (S-EL1)
               -> back through the same stack
```

That is the system shape that drove most of the complexity.

## Why Rebuild Hafnium Instead of Using It

The practical reason is that rebuilding the Secure-world control plane teaches you things that reading the FF-A spec does not.

The FF-A spec tells you what messages exist. It does not tell you where the real footguns are:

- which CPU actually resumes after an SMC
- what happens when pKVM boots secondaries and SPMD state is per-CPU
- how Secure and Non-Secure memory aliases behave when the MMU is off
- what compiler codegen assumptions become hardware traps below the OS

The engineering reason is that this domain is dense with state machines, ownership transitions, and error paths. That is exactly where Rust is more than branding. A lot of the important logic in an SPMC is not algorithmically hard; it is "easy to get 95% right and still ship a subtle bug." I wanted the compiler to force more of those cases into the open.

## What It Does Today

The current project has four main capabilities:

- boots Linux 6.12 to a BusyBox shell
- manages three Secure Partitions at `S-EL1`
- implements FF-A v1.1 flows including direct messaging, indirect messaging, memory sharing, and notifications
- coexists with pKVM on four physical CPUs and passes full-stack tests through TF-A

The metrics are good enough that this is not just a design exercise:

- `make run`: `34` test suites, `457` assertions
- BL33 integration tests: `20/20`
- pKVM end-to-end tests: `35/35`
- release SPMC binary: about `230KB`
- dependencies: `1`

## Three Places Where It Got Real

### 1. SPMD Is Per-CPU

This was one of the first places where the actual system behavior diverged from the mental model you might get from the spec.

TF-A's Secure Partition Manager Dispatcher keeps separate state for each physical CPU. That matters because pKVM boots secondary CPUs via PSCI, and each one enters the Secure world on whatever physical core it lands on. It is not enough to bring up CPU 0 and assume the Secure side is globally initialized.

Each secondary has to do its own `FFA_MSG_WAIT` handshake so SPMD knows that CPU's Secure world is ready. If one CPU skips that handshake, SPMD can block the Normal-world `CPU_ON` sequence. The result looks like a secondary boot failure in pKVM, but the real cause is that the Secure side never finished its per-CPU setup.

The fix was to register `FFA_SECONDARY_EP_REGISTER`, allocate per-CPU stacks, and run a full event loop on every core.

That may sound obvious in hindsight. It was not obvious from the public spec. I found the missing piece in TF-A source.

### 2. The NS Bit and the Invisible Write

This was the most memorable architecture bug in the whole project.

`PARTITION_INFO_GET` worked perfectly from a BL33 harness. The SPMC wrote descriptors to the caller's RX buffer, and the caller read them back correctly. Then pKVM called the same path and got zeros.

There was no fault. The address was correct. GDB showed the store executed. But the data was not there when the Normal world read it.

The reason is that Secure and Non-Secure are not just permission labels. They are separate physical address spaces. With the S-EL2 MMU off, a write from the Secure side goes to the Secure physical alias. pKVM was reading the Non-Secure alias at the same numeric address. Same address, different memory.

What fixed it was enabling an S-EL2 Stage-1 identity map where Normal-world DRAM is marked `NS=1`, forcing those accesses into the Non-Secure physical space.

It is the kind of bug that makes perfect sense once explained, but is still easy to miss if your intuition comes from systems where "secure" and "non-secure" mostly mean access control rather than address-space selection.

### 3. Rust State Machines Actually Mattered

The Secure Partition lifecycle is a state machine:

```rust
enum SpState {
    Reset,
    Idle,
    Running,
    Blocked,
    Preempted,
}
```

That alone is not special. What mattered was what happened when the design changed.

During SP-to-SP messaging, one partition can block on another. Then a Normal-world interrupt can arrive mid-chain, which means a `Running` partition becomes `Preempted`, while an upstream `Blocked` partition may also need to move into a preempted state so the chain can resume consistently later.

When I added that behavior, Rust forced me to revisit every transition point and every `match` that assumed a smaller state graph. That flushed out two bugs before runtime.

In application code, this would be a nice correctness benefit. At `S-EL2`, it is more than that. Many failure modes do not give you a friendly crash. They give you a hang, a world-switch dead end, or a corrupted control path that only becomes visible several exception levels later.

This was probably the clearest example in the entire project of Rust paying for itself in a way that was specific to the job.

## Two Bugs That Best Capture The Work

### The Silent SIMD Trap

At one point the SPMC booted in release mode but hung in debug mode on the first `read_volatile`. No output. No obvious fault. Just a dead system.

After a few hours in GDB, the CPU turned out to be stuck in an EL3 exception handler because an FP/SIMD trap had fired. The confusing part was that my code was not doing floating-point work.

The real cause was compiler codegen. In debug mode, the alignment path around `read_volatile` compiled into a NEON instruction. TF-A's default configuration traps floating-point and SIMD use from lower exception levels. EL3 was catching that trap, but the flow was not wired for what I had just generated.

The fix was one build flag: `CTX_INCLUDE_FPREGS=1`.

The lesson was not "remember this one TF-A flag." The lesson was that once you run below an OS, your compiler's code generation is part of the hardware contract. You do not get to pretend codegen details are an implementation detail anymore.

### The Stale Cache And The Phantom Data Abort

Another bug only reproduced some of the time: pKVM memory sharing worked for a while, then the SPMC crashed with a Data Abort from a nonsense pointer.

The descriptor had been written by pKVM into a Normal-world TX buffer on CPU 0. The SMC eventually routed into the Secure side on another CPU. Even with explicit barriers, parsing the shared buffer in place was not reliable enough. Occasionally one field came back stale, the parser computed a garbage offset, and the Secure side chased it into unmapped memory.

What fixed it was copying the entire descriptor into a local stack buffer before parsing it. Parsing from a local, stable copy turned a sporadic crash into a clean validation path.

That is a very specific bug, but it captures the broader theme of this project: once two worlds and multiple CPUs are involved, "the data is there" is not a strong enough statement. You need to reason about where the data was written, which CPU sees it, under which translation regime, and in which world.

## Why I Think This Is a Good Rust Project

This project is not interesting because it is "Rust instead of C." It is interesting because the problem shape matches the language well.

The SPMC is full of:

- explicit state transitions
- typed ownership transfer
- protocol decoding with lots of edge cases
- invariants that should be impossible rather than merely unlikely

At the same time, it does not need a huge runtime. The code is `no_std`. The dependency list is tiny. The memory model is explicit. Most of the complexity comes from hardware and protocol semantics, not framework concerns.

That combination makes Rust feel less like a fashionable choice and more like a good fit.

## Try It

If you want the short path, start here:

```bash
git clone https://github.com/willamhou/hypervisor
cd hypervisor
make run
```

That runs the bare-metal test suites on QEMU in a few seconds.

If you want the code and architecture next:

- repo: <https://github.com/willamhou/hypervisor>
- architecture overview: [ARCHITECTURE.md](../ARCHITECTURE.md)

And if this is the kind of work you care about, the repo is set up to be read in layers: quick start first, architecture second, source code third.
