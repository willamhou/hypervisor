# Writing an ARM64 Type-1 Hypervisor in Rust from Scratch

*How I built a bare-metal SPMC that boots Linux, manages Secure Partitions, and coexists with Android pKVM — in 30K lines of `no_std` Rust.*

---

ARM's latest chips split the CPU into two worlds. The Normal world runs Android, Linux, your apps. The Secure world runs firmware, crypto, DRM. Each world gets its own hypervisor at EL2.

Google's [pKVM](https://source.android.com/docs/core/virtualization) handles the Normal side. [Hafnium](https://hafnium.googlesource.com/hafnium/), Google's reference Secure Partition Manager Core (SPMC), handles the Secure side. Hafnium is 200K+ lines of C.

I replaced it with 30,000 lines of Rust. No runtime, no allocator crate, no dependencies beyond a DTB parser. It boots Linux to a BusyBox shell, manages three Secure Partitions, and passes 35/35 end-to-end tests running alongside a real pKVM kernel — all on QEMU, no hardware required.

This post covers how I built it, what I learned, and the three bugs that cost me the most sleep.

## The ARM Privilege Onion

If you've only worked on x86, ARM's exception levels take some getting used to. There are four levels, and the Secure world doubles most of them:

```
            Normal World          Secure World
           ┌────────────┐       ┌────────────┐
    EL0    │  Userspace  │       │            │
           ├────────────┤       ├────────────┤
    EL1    │ Linux/Android│       │  Secure    │
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

EL3 is the root of trust — ARM Trusted Firmware (TF-A) lives here. It acts as a relay: when the Normal world wants to talk to the Secure world, it does an SMC (Secure Monitor Call) to EL3, which switches contexts and delivers the message.

The protocol for this communication is [FF-A](https://developer.arm.com/documentation/den0077/latest) (Firmware Framework for Arm) v1.1. It defines how to send messages, share memory, transfer page ownership, and manage partitions. It's the system call interface between worlds.

My hypervisor fills the S-EL2 box. It manages Secure Partitions at S-EL1 (small trusted apps for things like key management), handles FF-A calls, and maintains per-SP Stage-2 page tables for isolation.

## Why Rust, Why from Scratch

Three reasons:

**Understanding.** I wanted to understand every layer — from the first instruction after reset to Linux printing `/ #`. Reading Hafnium's C and trying to modify it taught me less than building the equivalent from scratch.

**Rust's type system pays off at EL2.** Secure Partition lifecycle is a state machine: Reset → Idle → Running → Blocked → Preempted. In C, this is an int and a prayer. In Rust, it's an enum and the compiler rejects invalid transitions. When I added the Blocked → Preempted edge (for chain preemption during SP-to-SP messaging), `match` forced me to handle every case. That caught two bugs at compile time.

**`no_std` Rust is surprisingly viable for bare-metal.** My `Cargo.toml` has one dependency: `fdt = "0.1.5"` for device tree parsing. Everything else — page tables, GIC emulation, virtio drivers, the SPMC event loop — is hand-written. The `alloc` crate gives me `Box` and `Vec` backed by a bump allocator. Enum dispatch replaces trait objects for zero-cost MMIO routing.

## What It Looks Like

The full boot chain:

```
TF-A BL1 (ROM) → BL2 (loader) → BL31 (SPMD at EL3)
    → BL32 (our SPMC at S-EL2, boots SP1/SP2/SP3)
    → BL33 (pKVM at NS-EL2 → Linux at NS-EL1)
```

On `make run-pkvm-ffa-test`, you see both hypervisors come up on 4 CPUs, then a kernel module exercises the full FF-A stack:

```
[SPMC] SP1 booted, now Idle
[SPMC] SP2 booted, now Idle
[SPMC] SP3 booted, now Idle
[SPMC] Secondary EP registered with SPMD
...
Protected hVHE mode initialized successfully
...
ffa_test: [PASS] DIRECT_REQ to SP 0x8001 returns success
ffa_test: [PASS] SP 0x8001 x4 = 0xBBBB + 0x1000
ffa_test: [PASS] MEM_SHARE returns success
ffa_test: [PASS] Shared page == 0xCAFEFACE (SP wrote it)
...
ffa_test: [PASS] SP1 RECLAIM returns FFA_SUCCESS
ffa_test:   Results: 35/35 PASS
```

The `x4 = 0xBBBB + 0x1000` is the proof: the Normal world sends `x4=0xBBBB` via FF-A DIRECT_REQ, the message traverses EL3 (SPMD), arrives at S-EL2 (our SPMC), gets dispatched to SP1 at S-EL1, and SP1 adds `0x1000` before responding. Four exception levels, two world switches, one round trip.

## Technical Highlights

### Stage-2 Page Table Tricks

ARM's Stage-2 translation maps guest physical addresses (IPAs) to real physical addresses. I use identity mapping (GPA == HPA) but with a twist: the software-defined bits in each page table entry track ownership state.

```
PTE bits [56:55]:
  00 = Owned          (page belongs to this VM)
  01 = SharedOwned    (shared out, sender retains ownership)
  10 = SharedBorrowed (mapped from another VM/SP)
  11 = Donated        (irrevocably transferred)
```

This mirrors pKVM's ownership model. When VM 0 shares a page with SP1:

1. SPMC validates VM 0 owns the page (SW bits = `00`)
2. Sets VM 0's PTE to SharedOwned (`01`) + read-only (S2AP)
3. Maps the page into SP1's Secure Stage-2 as SharedBorrowed (`10`)
4. On reclaim: validates SP1 has relinquished, restores VM 0's PTE to Owned + read-write

The Stage-2 walker reconstructs itself from `VTTBR_EL2` at SMC handling time — it walks and modifies PTEs without owning the page table memory. This means the SPMC can manipulate any VM's page tables by just knowing the L0 table physical address.

### SP-to-SP Messaging and Cycle Detection

Secure Partitions can send messages to each other. SP1 sends a DIRECT_REQ to SP2, which forwards to SP3, which responds. The SPMC routes each hop:

```
NWd → DIRECT_REQ(SP1) → SP1 runs → DIRECT_REQ(SP3) → SP3 runs
    → DIRECT_RESP(SP1) → SP1 resumes → DIRECT_RESP(NWd)
```

Each SP that makes an outgoing call goes from Running to Blocked. The SPMC maintains a CallStack and checks for cycles on every dispatch: if SP1 → SP3 → SP1, that's `FFA_BUSY`. Without this check, the system would deadlock.

The tricky part is preemption. A Normal world interrupt can arrive while SP3 is running inside a chain. The SPMC must transition SP3 from Running to Preempted, SP1 from Blocked to Preempted (a "chain preemption"), and return `FFA_INTERRUPT` to the Normal world. When the Normal world later calls `FFA_RUN`, the entire chain resumes.

Getting the state machine right required 58 dedicated unit tests for `SpContext` alone, including every illegal transition.

### The `handle_sp_exit()` Loop

This is the heart of the SPMC, and the design I'm most pleased with. When the SPMC dispatches a request to an SP, the SP runs until it does an SMC — but that SMC might not be a response. It could be the SP requesting a memory operation, logging a message, or calling another SP.

```rust
loop {
    enter_guest();  // ERET to S-EL1
    let exit = decode_exit();
    match exit {
        FFA_MSG_SEND_DIRECT_RESP => return response,
        FFA_MEM_RETRIEVE_REQ => { handle locally; re-enter SP },
        FFA_MEM_RELINQUISH  => { handle locally; re-enter SP },
        FFA_CONSOLE_LOG     => { print to UART; re-enter SP },
        FFA_MSG_SEND_DIRECT_REQ => { dispatch to target SP; re-enter SP },
        FFA_MEM_SHARE       => { record share; re-enter SP },
        _ => return error,
    }
}
```

The SP doesn't know (or care) that its RETRIEVE_REQ is being handled locally by the SPMC rather than going to another entity. It does an SMC, gets a result, and continues. This is what makes the E2E memory sharing test work: the Normal world shares a page, SP1 retrieves it (SMC to SPMC, handled in-loop), writes `0xCAFEFACE`, relinquishes (another in-loop SMC), and responds — all within a single dispatch.

## War Stories

### The Silent SIMD Trap

Week 4. The SPMC boots fine in release mode but hangs on the first `read_volatile` in debug mode. No output, no fault, just... nothing.

After hours with GDB, I found the CPU stuck in an EL3 exception handler. ESR showed an FP/SIMD trap. But my code doesn't use floating point — it's `no_std`, I'm reading integers from memory.

Turns out, Rust's debug-mode codegen emits NEON instructions for things you wouldn't expect. The alignment check inside `read_volatile` compiles to `cnt v0.8b, v0.8b` — a SIMD population count. TF-A's default configuration sets `CPTR_EL3.TFP=1`, which traps ALL floating-point and SIMD instructions from every exception level to EL3. EL3's handler wasn't prepared for this trap, so it looped forever.

The fix was one build flag: `CTX_INCLUDE_FPREGS=1` in TF-A, which clears the trap bit. But the lesson is deeper: when you run below the OS, your compiler's code generation becomes a hardware constraint. "Normal" Rust operations can hit architectural traps that don't exist in userspace.

### SPMD Is Per-CPU

Week 7. pKVM boots fine on CPU 0. Secondary CPUs come up via PSCI CPU_ON, reach S-EL2, and... hang.

The FF-A spec describes the SPMC init sequence but says almost nothing about secondary CPUs. After reading TF-A's `spmd_cpu_on_finish_handler()` source, I discovered: SPMD maintains entirely separate state for each physical CPU. When a secondary arrives at S-EL2, SPMD expects it to call `FFA_MSG_WAIT` — a handshake that signals "this CPU's Secure world is ready."

My initial code had secondary CPUs do `WFE` (wait for event) after basic init. That's what you'd do in a Normal world hypervisor. But SPMD was waiting for its per-CPU `FFA_MSG_WAIT`, and without it, SPMD never completes the PSCI CPU_ON call, so the Normal world secondary never boots either.

The fix was `FFA_SECONDARY_EP_REGISTER` during init (tells SPMD where secondary CPUs should enter S-EL2), per-CPU stacks (3 x 32KB in `.bss`), and a full event loop on each secondary. This is documented nowhere except TF-A's source code.

### The NS Bit and the Invisible Write

Week 8. `PARTITION_INFO_GET` works perfectly when called from BL33 (our test harness at NS-EL2). The SPMC writes SP descriptors to the caller's RX buffer, and the caller reads them back. 24 bytes per partition, two partitions, everything checks out.

Then pKVM calls the same function. The SPMC writes to pKVM's RX buffer — same code path, same descriptor format. pKVM reads... all zeros.

This took a full day. The write was succeeding (no fault). The address was correct (verified in GDB). But the data wasn't there.

The answer is ARM's two physical address spaces. When S-EL2 runs with the MMU off (which it does by default), all memory accesses go through the **Secure** physical address space. pKVM's RX buffer is at, say, `0x42a16000` in **Non-Secure** DRAM. The write hits physical address `0x42a16000` in the Secure alias. pKVM reads from `0x42a16000` in the Non-Secure alias. Different memory.

The fix was enabling S-EL2's Stage-1 MMU with an identity map where all Normal world DRAM (0x40000000+) is marked with the `NS=1` attribute bit. This forces the CPU to access the Non-Secure alias, where pKVM can see it.

I've worked with ARM systems for years and never internalized that the Secure/Non-Secure distinction is a *physical address space split*, not just a permission model. In QEMU, there's literally twice the memory — two 2GB regions at the same addresses, selected by the NS bit.

## Testing Without an OS

All tests run on bare metal. There's no test harness, no OS, no `#[test]`. The binary's `main()` calls each test suite sequentially:

```rust
fn main() {
    test_dtb::run();
    test_allocator::run();
    // ... 32 more suites ...
    test_guest_interrupt::run(); // last, blocks forever
}
```

Each suite prints `[PASS]` or `[FAIL]` to UART. QEMU exits when the guest calls `SYSTEM_RESET` (or I kill it after the blocking test).

For integration tests, the BL33 binary is a 500-line assembly program that sends 20 FF-A calls to the SPMC and validates each response. For pKVM E2E tests, `ffa_test.ko` is a Linux kernel module that does the same through pKVM's FF-A proxy.

No mocking. The BL33 tests go through real TF-A firmware at EL3. The pKVM tests go through pKVM's real hypervisor at NS-EL2, through real SPMD at EL3, into our SPMC at S-EL2, down to real SPs at S-EL1. If any layer is broken, the test fails.

## Numbers

| Metric | Value |
|--------|-------|
| Rust source | 26,000 lines (96 files) |
| ARM64 assembly | 3,400 lines (9 files) |
| Unit test assertions | 457 |
| BL33 integration tests | 20/20 |
| pKVM E2E tests | 35/35 |
| Commits | 312 |
| Dependencies | 1 (`fdt` crate for DTB parsing) |
| Dev time | ~10 weeks (solo) |
| Binary size | 379KB (release, default) / 230KB (release, SPMC) |

## What I'd Do Differently

**Start with `opt-level = 1` from day one.** Debug-mode Rust generates much larger stack frames. I spent a week debugging a multi-CPU crash that turned out to be a stack overflow — secondary CPU stacks were 16KB, and debug-mode Rust's call chain was deeper than that. Release mode worked fine. I now use `opt-level = 1` even in dev profile.

**Read TF-A source before the specs.** The FF-A spec tells you the *what*. TF-A's source tells you the *how* — what SPMD actually does with your `FFA_MSG_WAIT`, how secondary CPUs get routed to S-EL2, which EL1 registers are (not) saved across world switches. I would have saved days by reading `spmd_main.c` first.

**Don't assume caches are coherent across worlds.** On multi-CPU pKVM, the SPMD enters S-EL2 on whichever physical CPU happens to be running. The Normal world writes a descriptor to its TX buffer on CPU 0, but S-EL2 might read it on CPU 1 with a stale L1 cache line. The fix is a `DSB SY` barrier before every cross-world buffer read + copying to a local buffer before parsing. This took two debugging sessions to figure out.

## What's Next

The big remaining piece is ARM's **Realm Management Extension** (RME) — the "R" in ARM CCA (Confidential Compute Architecture). RME adds a fourth world (Realm) with hardware-enforced memory isolation. A Realm VM's memory is inaccessible to both the Normal world hypervisor *and* the Secure world firmware.

This requires:
- Granule Protection Tables (GPT) at EL3
- A Realm Management Interface (RMI) at EL2
- Realm guest support with attestation

It's a significant step up from what exists today, but the SPMC infrastructure (Stage-2 management, FF-A messaging, multi-CPU dispatch) provides a solid foundation.

The project runs entirely on QEMU today. Hardware validation (AWS Graviton, Ampere Altra) is on the list but not blocking — QEMU's TCG accurately models the exception levels, GIC, and memory model.

## Try It

```bash
git clone https://github.com/willamhou/hypervisor
cd hypervisor
make run          # 34 test suites, ~5 seconds on QEMU
make run-linux    # boots Linux 6.12 to shell
```

For the SPMC and pKVM targets, you'll need to build TF-A and the AOSP kernel — the Makefile handles both via Docker. See the [README](https://github.com/willamhou/hypervisor) for details.

Questions, bugs, and contributions welcome on [GitHub Issues](https://github.com/willamhou/hypervisor/issues).

---

*Built with Rust nightly, QEMU 9.2, and an unreasonable amount of time staring at the ARM Architecture Reference Manual.*
