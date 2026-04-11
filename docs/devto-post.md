---
title: "Two Hypervisors, One SoC: Replacing Google's Hafnium with 30K Lines of Rust"
published: true
description: "Building a bare-metal ARM64 SPMC at S-EL2 in no_std Rust — running alongside Android pKVM, booting Linux, managing Secure Partitions, with 35/35 E2E tests through real TF-A firmware."
tags: rust, arm, hypervisor, embedded
cover_image: 
canonical_url: https://willamhou.github.io/hypervisor/
---

I built an ARM64 hypervisor that runs *next to* Google's pKVM on the same chip. pKVM takes the Normal world at NS-EL2. My hypervisor takes the Secure world at S-EL2. They coordinate through ARM's FF-A protocol, relayed by EL3 firmware. 35 end-to-end tests pass through the full four-level stack: Linux kernel module → pKVM → TF-A → my SPMC → Secure Partitions → and back.

The Secure side already had an implementation: [Hafnium](https://hafnium.googlesource.com/hafnium/), Google's reference SPMC. It's 200K+ lines of C. I replaced it with 30,000 lines of `no_std` Rust — no runtime, no allocator crate, one dependency (a DTB parser). It boots Linux to a BusyBox shell, manages three Secure Partitions, and handles FF-A v1.1 messaging and memory sharing.

I'll walk through the architecture, the parts that were genuinely hard, and the four bugs I spent the most time chasing.

{% github willamhou/hypervisor %}

## ARM's Split Personality

ARM's latest chips divide the CPU into two security worlds. Each world gets its own hypervisor at EL2:

```
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

EL3 is the root of trust — ARM Trusted Firmware (TF-A) lives here and relays messages between worlds via SMC (Secure Monitor Call). The protocol is [FF-A](https://developer.arm.com/documentation/den0077/latest) v1.1: it defines messaging, memory sharing, page ownership transfer, and partition management. My hypervisor fills the S-EL2 box.

## Two Hypervisors, One Chip

This is the part most hypervisor projects don't deal with: coexistence. pKVM and my SPMC boot on the same 4 physical CPUs, each managing their own world. The boot chain:

```
TF-A BL1 (ROM) → BL2 (loader) → BL31 (SPMD at EL3)
    → BL32 (our SPMC at S-EL2, boots SP1/SP2/SP3)
    → BL33 (pKVM at NS-EL2 → Linux at NS-EL1)
```

When pKVM's Linux guest wants to talk to a Secure Partition, the message crosses four exception levels and two world switches:

```
Linux (NS-EL1) → SMC → pKVM (NS-EL2) → SMC → SPMD (EL3)
    → ERET → SPMC (S-EL2) → ERET → SP1 (S-EL1)
    → SMC → SPMC → SMC → SPMD → ERET → pKVM → ERET → Linux
```

The proof: Linux sends `x4=0xBBBB` via FF-A DIRECT_REQ, SP1 adds `0x1000`, Linux reads back `0xCBBB`. One round trip, four privilege levels, two world switches.

Making this work meant dealing with problems that mostly don't show up in a single-hypervisor setup:

**SPMD is per-CPU.** TF-A's Secure Partition Manager Dispatcher maintains separate state for each physical CPU. When pKVM boots secondary CPUs via PSCI, each one enters S-EL2 on whichever physical core it lands on. My SPMC must register a secondary entry point (`FFA_SECONDARY_EP_REGISTER`), allocate per-CPU stacks (3 × 32KB), and run a full event loop on every core. If any CPU skips its `FFA_MSG_WAIT` handshake, SPMD blocks the entire PSCI boot sequence. This is documented nowhere except TF-A's source code.

**S-EL2 Stage-1 MMU and the NS bit.** The Secure world has its own physical address space. When S-EL2 writes to address `0x42a16000` with the MMU off, it hits the *Secure* alias. pKVM's buffer is at the same address in the *Non-Secure* alias. Different memory. I had to enable an S-EL2 Stage-1 identity map where all Normal world DRAM is marked `NS=1` to force writes to the correct alias.

**Cross-CPU cache coherency.** pKVM writes a descriptor to its TX buffer on CPU 0, then issues an SMC. SPMD routes the call to S-EL2 on whichever CPU happens to be running — potentially CPU 2 with a stale L1 cache line. Even after adding `DSB SY` barriers, I had to copy the descriptor to a local stack buffer before parsing it.

On `make run-pkvm-ffa-test`, the full TF-A boot chain comes up, then pKVM initializes, and our kernel module exercises every FF-A path:

```
ffa_test: Sending DIRECT_REQ to SP 0x8001...
ffa_test:   x3=0xaaaa x4=0xcbbb x5=0xcccc x6=0xdddd
ffa_test: [PASS] SP 0x8001 x4 = 0xBBBB + 0x1000
...
ffa_test: [PASS] Shared page == 0xCAFEFACE (SP wrote it)
ffa_test: [PASS] SP1→SP3 relay chain returns success
ffa_test: [PASS] SP1→SP2 Secure DRAM share verified
ffa_test:   Results: 35/35 PASS
```

## Rust at Exception Level 2

Secure Partition lifecycle is a state machine: Reset → Idle → Running → Blocked → Preempted. In C, this would probably be an integer plus a set of invariants everyone has to remember. In Rust:

```rust
enum SpState { Reset, Idle, Running, Blocked, Preempted }
```

When I added the Blocked → Preempted edge for chain preemption during SP-to-SP messaging, the compiler forced me to revisit every transition. That flushed out two bugs before I ever ran the code.

My `Cargo.toml` has one dependency: `fdt = "0.1.5"`. Everything else — page tables, GIC emulation, virtio drivers, the SPMC event loop — is hand-written. The `alloc` crate gives me `Box` and `Vec` backed by a bump allocator. Enum dispatch replaces trait objects for zero-cost MMIO routing.

## Technical Highlights

### Stage-2 Page Table Tricks

ARM's Stage-2 translation maps guest physical addresses to real physical addresses. I use identity mapping but repurpose the software-defined PTE bits for ownership tracking:

```
PTE bits [56:55]:
  00 = Owned          (page belongs to this VM)
  01 = SharedOwned    (shared out, sender retains ownership)
  10 = SharedBorrowed (mapped from another VM/SP)
  11 = Donated        (irrevocably transferred)
```

This mirrors pKVM's model. When VM 0 shares a page with SP1: validate ownership (SW bits = `00`), set to SharedOwned (`01`) + read-only, map into SP1's Secure Stage-2 as SharedBorrowed (`10`). On reclaim: validate SP1 has relinquished, restore to Owned + read-write.

### SP-to-SP Messaging and Cycle Detection

Secure Partitions can message each other. SP1 sends a DIRECT_REQ to SP3, which forwards to SP2, which responds. The SPMC routes each hop:

```
NWd → SP1 runs → DIRECT_REQ(SP3) → SP3 runs
    → DIRECT_RESP(SP1) → SP1 resumes → DIRECT_RESP(NWd)
```

Each SP making an outgoing call transitions from Running to Blocked. The SPMC maintains a CallStack and checks for cycles: SP1 → SP3 → SP1 returns `FFA_BUSY`. Without this, deadlock.

### The `handle_sp_exit()` Loop

This is the heart of the SPMC. When the SPMC dispatches to an SP, the SP runs until it traps — but the trap might not be a response. It could be a memory operation, a log message, or a call to another SP.

```rust
loop {
    enter_guest();  // ERET to S-EL1
    let exit = decode_exit();
    match exit {
        FFA_MSG_SEND_DIRECT_RESP => return response,
        FFA_MEM_RETRIEVE_REQ    => { handle locally; re-enter SP },
        FFA_MEM_RELINQUISH      => { handle locally; re-enter SP },
        FFA_MEM_SHARE           => { record share; re-enter SP },
        FFA_CONSOLE_LOG         => { print to UART; re-enter SP },
        FFA_MSG_SEND_DIRECT_REQ => { dispatch to target SP; re-enter },
        _ => return error,
    }
}
```

The SP doesn't know its RETRIEVE_REQ is handled locally rather than going to another entity. This is what makes E2E memory sharing work: the Normal world shares a page, SP1 retrieves it (in-loop), writes `0xCAFEFACE`, relinquishes (in-loop), and responds — all within a single dispatch.

## War Stories

### 1. The Silent SIMD Trap

Week 4. The SPMC boots fine in release mode but hangs on the first `read_volatile` in debug. No output, no fault, nothing.

After a few hours with GDB, I found the CPU stuck in an EL3 exception handler. ESR showed an FP/SIMD trap. But my code doesn't use floating point.

Rust's debug-mode codegen will happily emit NEON instructions for things that look unrelated. In my case, the alignment check inside `read_volatile` compiled to `cnt v0.8b, v0.8b` — a SIMD population count. TF-A's default `CPTR_EL3.TFP=1` traps all floating-point and SIMD from every exception level. EL3's handler wasn't prepared for that trap, so it looped forever.

**The fix:** one build flag: `CTX_INCLUDE_FPREGS=1`. 

**The lesson:** once you're running below an OS, your compiler's codegen is part of the hardware contract.

### 2. The NS Bit and the Invisible Write

Week 8. `PARTITION_INFO_GET` works perfectly from our BL33 test harness. Then pKVM calls the same function. Same code path, same descriptor format. pKVM reads... all zeros.

The write succeeded (no fault). The address was correct (verified in GDB). But the data wasn't there.

ARM has two physical address spaces. When S-EL2 runs with the MMU off, all memory accesses go through the Secure physical address space. pKVM's buffer is at `0x42a16000` in Non-Secure DRAM. The write hits `0x42a16000` Secure. pKVM reads from `0x42a16000` Non-Secure. Different memory.

**The fix:** enabling an S-EL2 Stage-1 MMU with an identity map where all Normal world DRAM has the `NS=1` attribute bit.

**The lesson:** Secure/Non-Secure is a *physical address space split*, not just a permission model. In QEMU, there's literally twice the memory at the same addresses, selected by one bit.

### 3. The Stale Cache and the Phantom Data Abort

Week 11. pKVM's MEM_SHARE works 70% of the time. The other 30%, the SPMC crashes with a Data Abort at a pointer address like `0x240f` — clearly not a valid physical address.

The descriptor lived in pKVM's TX buffer — Normal world DRAM. pKVM writes it on CPU 0, issues an SMC, SPMD context-switches to S-EL2 on CPU 2. Even though ARM's memory model guarantees the SMC acts as a barrier for the issuing CPU, the *receiving* CPU might still have a stale L1 cache line.

**The fix:** copying the entire descriptor to a local stack buffer before parsing it.

```rust
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)); }
let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8, local_buf.as_mut_ptr(), total_length,
    );
}
// Parse from local_buf, never from the shared buffer
let parsed = parse_mem_region(local_buf.as_ptr(), total_length);
```

### 4. SPMD Is Per-CPU (or: Read the Firmware Source)

Week 7. pKVM boots fine on CPU 0. Secondary CPUs hang.

The FF-A spec describes SPMC init but says almost nothing about secondary CPUs. After reading TF-A's `spmd_cpu_on_finish_handler()`, I found it: SPMD maintains *entirely separate state* per physical CPU. Each secondary entering S-EL2 must call `FFA_MSG_WAIT`. Without it, SPMD never completes the PSCI CPU_ON call, so the Normal world secondary never boots either.

**The fix:** registering `FFA_SECONDARY_EP_REGISTER` during init and giving each secondary its own stack and event loop. 

**The lesson:** the FF-A spec tells you *what* has to happen; TF-A's source code is where you find *how*.

## Numbers

| Metric | Value |
|--------|-------|
| Rust source | 26,000 lines (96 files) |
| ARM64 assembly | 3,400 lines (9 files) |
| Unit test assertions | 457 |
| BL33 integration tests | 20/20 |
| pKVM E2E tests | 35/35 |
| Dependencies | 1 (`fdt` crate) |
| Dev time | ~10 weeks (solo) |
| Binary size | 230KB (release, SPMC) |

## Try It

```bash
git clone https://github.com/willamhou/hypervisor
cd hypervisor
make run          # 34 test suites, ~5 seconds on QEMU
make run-linux    # boots Linux 6.12 to shell
```

For `make run-spmc` and `make run-pkvm-ffa-test`, you'll need TF-A and (for pKVM) the AOSP kernel — both build via Docker. The full build takes ~30 minutes the first time. See the [README](https://github.com/willamhou/hypervisor) for details.

---

*Built with Rust nightly, QEMU 9.2, and a lot of time spent cross-checking the ARM ARM.*
