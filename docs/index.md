---
layout: default
title: "Two Hypervisors, One SoC: Replacing Hafnium with 30K Lines of Rust"
description: "A bare-metal SPMC at S-EL2 that boots Linux, manages Secure Partitions, and runs alongside Android pKVM"
---

*A bare-metal SPMC at S-EL2 that boots Linux, manages Secure Partitions, and runs alongside Android pKVM — from first ERET to 35/35 E2E tests in 10 weeks.*

[View on GitHub](https://github.com/willamhou/hypervisor)

---

I built an ARM64 hypervisor that runs *next to* Google's pKVM on the same chip. pKVM takes the Normal world at NS-EL2. My hypervisor takes the Secure world at S-EL2. They coordinate through ARM's FF-A protocol, relayed by EL3 firmware. 35 end-to-end tests pass through the full four-level stack: Linux kernel module → pKVM → TF-A → my SPMC → Secure Partitions → and back.

The Secure side already had an implementation: [Hafnium](https://hafnium.googlesource.com/hafnium/), Google's reference SPMC. It's 200K+ lines of C. I replaced it with 30,000 lines of `no_std` Rust — no runtime, no allocator crate, one dependency (a DTB parser). It boots Linux to a BusyBox shell, manages three Secure Partitions, and handles FF-A v1.1 messaging and memory sharing.

This post covers the architecture, the parts that were hard, and four bugs that cost me the most sleep.

## ARM's Split Personality

ARM's latest chips divide the CPU into two security worlds. Each world gets its own hypervisor at EL2:

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

Making this work required solving problems that don't appear in a single-hypervisor setup:

**SPMD is per-CPU.** TF-A's Secure Partition Manager Dispatcher maintains separate state for each physical CPU. When pKVM boots secondary CPUs via PSCI, each one enters S-EL2 on whichever physical core it lands on. My SPMC must register a secondary entry point (`FFA_SECONDARY_EP_REGISTER`), allocate per-CPU stacks (3 × 32KB), and run a full event loop on every core. If any CPU skips its `FFA_MSG_WAIT` handshake, SPMD blocks the entire PSCI boot sequence. This is documented nowhere except TF-A's source code.

**S-EL2 Stage-1 MMU and the NS bit.** The Secure world has its own physical address space. When S-EL2 writes to address `0x42a16000` with the MMU off, it hits the *Secure* alias. pKVM's RX buffer is at the same address in the *Non-Secure* alias. Different memory. I had to enable an S-EL2 Stage-1 identity map where all Normal world DRAM is marked `NS=1` to force writes to the correct alias. (More on this in War Stories.)

**Cross-CPU cache coherency.** pKVM writes a descriptor to its TX buffer on CPU 0, then issues an SMC. SPMD routes the call to S-EL2 on whichever CPU happens to be running — potentially CPU 2 with a stale L1 cache line. Even after adding `DSB SY` barriers, I had to copy the descriptor to a local stack buffer before parsing it. Reading directly from the cross-world buffer produced data aborts from corrupt pointer arithmetic.

On `make run-pkvm-ffa-test`:

```
Protected hVHE mode initialized successfully
...
ffa_test: [PASS] DIRECT_REQ to SP1 returns success
ffa_test: [PASS] SP1→SP3 relay chain
ffa_test: [PASS] MEM_SHARE to SP1, SP writes 0xCAFEFACE
ffa_test: [PASS] SP1→SP2 Secure DRAM share
ffa_test:   Results: 35/35 PASS
```

## Rust at Exception Level 2

Secure Partition lifecycle is a state machine: Reset → Idle → Running → Blocked → Preempted. In C, this is an int and a prayer. In Rust:

```rust
enum SpState { Reset, Idle, Running, Blocked, Preempted }
```

When I added the Blocked → Preempted edge for chain preemption during SP-to-SP messaging, `match` forced me to handle every case. That caught two bugs at compile time.

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

The Stage-2 walker reconstructs itself from `VTTBR_EL2` at SMC handling time — it walks and modifies PTEs without owning the page table memory. The SPMC can manipulate any VM's page tables by just knowing the L0 table physical address.

### SP-to-SP Messaging and Cycle Detection

Secure Partitions can message each other. SP1 sends a DIRECT_REQ to SP3, which forwards to SP2, which responds. The SPMC routes each hop:

```
NWd → SP1 runs → DIRECT_REQ(SP3) → SP3 runs
    → DIRECT_RESP(SP1) → SP1 resumes → DIRECT_RESP(NWd)
```

Each SP making an outgoing call transitions from Running to Blocked. The SPMC maintains a CallStack and checks for cycles: SP1 → SP3 → SP1 returns `FFA_BUSY`. Without this, deadlock.

The tricky part is preemption. A Normal world interrupt arrives while SP3 is running mid-chain. The SPMC transitions SP3 from Running to Preempted, SP1 from Blocked to Preempted (chain preemption), and returns `FFA_INTERRUPT`. When the Normal world later calls `FFA_RUN`, the entire chain resumes.

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

The SP doesn't know its RETRIEVE_REQ is handled locally rather than going to another entity. It does an SMC, gets a result, and continues. This is what makes E2E memory sharing work: the Normal world shares a page, SP1 retrieves it (in-loop), writes `0xCAFEFACE`, relinquishes (in-loop), and responds — all within a single dispatch.

## War Stories

### The Silent SIMD Trap

Week 4. The SPMC boots fine in release mode but hangs on the first `read_volatile` in debug. No output, no fault, nothing.

After hours with GDB: the CPU was stuck in an EL3 exception handler. ESR showed an FP/SIMD trap. But my code doesn't use floating point.

Rust's debug-mode codegen emits NEON instructions for things you wouldn't expect. The alignment check inside `read_volatile` compiles to `cnt v0.8b, v0.8b` — a SIMD population count. TF-A's default `CPTR_EL3.TFP=1` traps ALL floating-point and SIMD from every exception level. EL3's handler wasn't prepared for this trap, so it looped forever.

The fix: one build flag (`CTX_INCLUDE_FPREGS=1`). The lesson: when you run below the OS, your compiler's codegen becomes a hardware constraint.

### The NS Bit and the Invisible Write

Week 8. `PARTITION_INFO_GET` works perfectly from our BL33 test harness. The SPMC writes SP descriptors to the caller's RX buffer, caller reads them back. 24 bytes per partition, everything checks out.

Then pKVM calls the same function. Same code path, same descriptor format. pKVM reads... all zeros.

The write succeeded (no fault). The address was correct (verified in GDB). But the data wasn't there.

ARM has two physical address spaces. When S-EL2 runs with the MMU off, all memory accesses go through the Secure physical address space. pKVM's buffer is at `0x42a16000` in Non-Secure DRAM. The write hits `0x42a16000` Secure. pKVM reads from `0x42a16000` Non-Secure. Different memory.

The fix: S-EL2 Stage-1 MMU with an identity map where all Normal world DRAM has the `NS=1` attribute bit. I've worked with ARM for years and never internalized that Secure/Non-Secure is a *physical address space split*, not just a permission model. In QEMU, there's literally twice the memory at the same addresses, selected by one bit.

### The Stale Cache and the Phantom Data Abort

Week 11. pKVM's MEM_SHARE works 70% of the time. The other 30%, the SPMC crashes with a Data Abort at a pointer address like `0x240f` — clearly not a valid physical address.

`addr2line` traced it to `parse_mem_region` in my descriptor parser. The descriptor's `composite_offset` field, which should be 80, was reading as garbage. The SPMC was dereferencing `base + garbage` and faulting.

The descriptor lived in pKVM's TX buffer — Normal world DRAM. pKVM writes it on CPU 0, issues an SMC, SPMD context-switches to S-EL2 on CPU 2. Even though ARM's memory model guarantees the SMC acts as a barrier for the issuing CPU, the *receiving* CPU might still have a stale L1 cache line.

I added `DSB SY` (Data Synchronization Barrier, full system scope) before every cross-world buffer read. Still crashed. The barrier ensures visibility, but the buffer itself is in Non-Secure DRAM that the SPMC accesses through the NS=1 Stage-1 mapping. Between the barrier and the read, the data could still be inconsistent from the SPMC's perspective.

The fix: copy the entire descriptor to a local stack buffer before parsing.

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

Now if the copy captures stale data, the bounds checks in `parse_mem_region` reject it cleanly instead of chasing wild pointers into Secure memory. Crash rate went from 30% to 0%.

### SPMD Is Per-CPU (or: Read the Firmware Source)

Week 7. pKVM boots fine on CPU 0. Secondary CPUs hang.

The FF-A spec describes SPMC init but says almost nothing about secondary CPUs. After reading TF-A's `spmd_cpu_on_finish_handler()`, I found it: SPMD maintains *entirely separate state* per physical CPU. Each secondary entering S-EL2 must call `FFA_MSG_WAIT` — a handshake that signals "this CPU's Secure world is ready." Without it, SPMD never completes the PSCI CPU_ON call, so the Normal world secondary never boots either.

My initial code had secondary CPUs do `WFE` (wait for event) after basic init. That's the Normal world pattern. But SPMD needs its per-CPU handshake, per-CPU stacks (3 × 32KB in `.bss`), and a full event loop on each secondary. The fix was `FFA_SECONDARY_EP_REGISTER` during init, and this lesson: the FF-A spec tells you the *what*; TF-A's source code tells you the *how*.

## Testing Without an OS

All tests run on bare metal. No test harness, no OS, no `#[test]`. The binary calls each test suite sequentially, printing `[PASS]` or `[FAIL]` to UART.

For integration tests, the BL33 binary is a 500-line assembly program that sends 20 FF-A calls and validates each response. For pKVM E2E tests, `ffa_test.ko` is a Linux kernel module that does the same through pKVM's FF-A proxy.

No mocking. The BL33 tests go through real TF-A firmware at EL3. The pKVM tests traverse pKVM at NS-EL2, SPMD at EL3, our SPMC at S-EL2, and SPs at S-EL1. If any layer is broken, the test fails.

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

## What's Next

The big remaining piece is ARM's **Realm Management Extension** (RME) — the "R" in ARM CCA. RME adds a fourth world (Realm) with hardware-enforced memory isolation. A Realm VM's memory is inaccessible to both the Normal world hypervisor *and* the Secure world firmware.

The SPMC infrastructure (Stage-2 management, FF-A messaging, multi-CPU dispatch) provides a solid foundation, but RME requires Granule Protection Tables at EL3, a Realm Management Interface at EL2, and guest attestation. Significant step up.

## Try It

```bash
git clone https://github.com/willamhou/hypervisor
cd hypervisor
make run          # 34 test suites, ~5 seconds on QEMU
make run-linux    # boots Linux 6.12 to shell
```

For `make run-spmc` and `make run-pkvm-ffa-test`, you'll need TF-A and (for pKVM) the AOSP kernel — both build via Docker. The full build takes ~30 minutes the first time. See the [README](https://github.com/willamhou/hypervisor) for details.

---

*Built with Rust nightly, QEMU 9.2, and the ARM Architecture Reference Manual open on a second monitor for 10 weeks straight.*
