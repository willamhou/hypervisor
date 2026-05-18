---
title: "Three Bare-Metal Rust Pitfalls Where the Compiler Is Right and the Hardware Has Opinions"
published: true
description: "Below the OS, your compiler's codegen is part of the hardware contract. Three real cases from an ARM64 bare-metal hypervisor: NEON in debug builds, SMC isn't a barrier, and BL33's x0 isn't always your DTB."
tags: rust, embedded, arm, systems
series: "ARM64 Bare-Metal Hypervisor in Rust"
---

> **Disclaimer**: This is about an experimental hypervisor that only runs on QEMU virt — no real-hardware validation yet. The lessons apply to anyone writing `no_std` Rust on ARM64 below an OS, but the platform-specific details (TF-A configs, QEMU boot modes) may vary by setup.

I'm writing an ARM64 bare-metal hypervisor in `no_std` Rust. No OS, no libc, no runtime. The Rust language itself behaves the same as it does in your CLI tools — ownership, borrowing, traits, async all work as advertised. But the hardware changes, and what the OS used to handle invisibly for you now surfaces.

This post is about three pitfalls that bit me repeatedly over 10 weeks. The common theme: **your Rust code is right, the compiler is right, the hardware has its own rules**.

---

## Pitfall 1: A NEON Instruction Hidden Inside `debug_assert!`

**Week 4.** SPMC boots fine in release mode. Switch to debug mode — the first `read_volatile(mmio_addr)` after powerup just dies. No panic, no fault output, the UART is silent, the CPU is stuck.

GDB attach: the CPU is sitting in **EL3**'s exception handler (not my code). Check `ESR_EL3` — exception class is `0x07`: **FP/SIMD access trapped**.

Problem is, my hypervisor doesn't use floating-point anywhere. `no_std`, no `f32`/`f64`, no `libm`, no FP-related dependency in the entire `Cargo.toml`.

I disassembled around `ELR_EL3`:

```text
  200140: cnt   v0.8b, v0.8b
  200144: addv  b0, v0.8b
  200148: umov  w0, v0.b[0]
```

`cnt v0.8b, v0.8b` is NEON SIMD — **byte-wise popcount across a 64-bit register**. Where on earth was the SIMD coming from?

Tracing back from `ELR_EL3`, this turned out to be inlined inside a `read_volatile` call site. Rust's `core::ptr::read_volatile` runs through `ub_checks::assert_unsafe_precondition!`; whether the alignment check actually gets emitted into the binary depends on the UB-checks compiler setting (on by default in debug profile, off by default in release). The exact macro / function name has shifted across stdlib versions; on the nightly I was using, the alignment-check branch ended up walking this chain:

```text
read_volatile(src)
  → ub_checks::maybe_is_aligned(addr, align)
    → addr.is_aligned_to(align)
      → align.is_power_of_two()
        → align.count_ones() == 1
```

`count_ones()` is popcount. LLVM lowers popcount on AArch64 to this NEON sequence (see LLVM's AArch64 popcount lowering):

```text
cnt   v0.8b, v0.8b      ; per-byte popcount
addv  b0,    v0.8b       ; horizontal sum
umov  w0,    v0.b[0]     ; move scalar back to GPR
```

The compiler picking NEON isn't a bug. `cnt` is the fastest popcount on ARMv8-A, and it's LLVM's default codegen.

In an OS environment, none of this matters — the OS enables FP/SIMD at boot and any user program freely uses NEON. But I'm running on top of TF-A, whose default config is **`CPTR_EL3.TFP=1`**: meaning "any FP/SIMD instruction at EL2 or below traps to EL3." EL3's default trap handler doesn't know how to deal with this, so it loops forever on the trap.

In release mode, `debug_assert!` is optimized out, the `cnt` doesn't appear, everything works.

### Fix

Build TF-A with:

```makefile
CTX_INCLUDE_FPREGS=1
```

This tells TF-A to save/restore FP/SIMD registers on world switch and clears `CPTR_EL3.TFP`. You also need `ENABLE_SVE_FOR_NS=0` and `ENABLE_SME_FOR_NS=0`, otherwise the TF-A build fails on SVE/SME feature-gate conflicts (took me half an hour to figure out they're mutually exclusive).

### Lesson

**Below the OS, your compiler's codegen is part of the hardware contract.** "I didn't write any FP code" doesn't mean your binary contains no FP instructions. Rust has denser sanity checks than C, and any of those checks may be lowered to instructions your exception level isn't allowed to execute.

Practical checklist:

1. Default to release builds for bare-metal work. To debug, use `opt-level = "z"` + manual `println!`s.
2. If you must use a debug build, grep the disassembly for `v0`-`v31` register references.
3. Confirm your exception level can actually execute those instructions (`CPTR_ELx` doesn't trap them).

This isn't Rust-specific — Clang/GCC debug modes can also pick NEON. Rust just trips it more often because it has more sanity checks.

---

## Pitfall 2: `SMC` Isn't a Memory Barrier

**Week 11.** pKVM uses FF-A `MEM_SHARE` to share a page with my SPMC. pKVM writes the FF-A descriptor into a TX buffer on some pCPU, then `smc #0` to EL3, EL3 ERETs into S-EL2, I read the buffer to parse it.

Most of the time (~70%) it works. The other ~30%, my parser sees garbage in `composite_memory_region_offset` (something like `0x240f`). The SPMC does `base + offset` pointer arithmetic on it — Data Abort.

First instinct: parser bug. `addr2line` lands me on `parse_mem_region`. The function logic is correct; the raw bytes it's reading are wrong.

Here's a misconception I keep seeing taken seriously: **"the SMC instruction is a memory barrier."** It isn't. ARMv8-A is explicit:

> An SMC instruction is a Synchronous exception. It causes a Context Synchronization Event, but no Data Synchronization Barrier or Instruction Synchronization Barrier.

A `Context Synchronization Event` only guarantees the CPU's pipeline/predicted state is consistent for the executing instruction stream. **It guarantees nothing about cross-CPU memory visibility.**

Back to the bug. The relevant ordering:

```text
pCPU_A  pKVM: write descriptor to TX buffer → smc #0 → enter EL3
pCPU_A  SPMD: switch to S-EL2 on pCPU_A
pCPU_A  SPMC: record FFA_MEM_SHARE request, return handle to pKVM

time passes...

pCPU_B  pKVM: FFA_RUN(SP2, handle) — schedules SP2 onto pCPU_B
pCPU_B  SPMC: ERET into SP2; SP2 reads that TX buffer → ???
```

The TX buffer is in Normal World DRAM. pKVM wrote on pCPU_A; pCPU_B's SPMC reads. One subtle point worth nailing down up front: **a `dsb` barrier only orders accesses on the CPU executing it. It does NOT reach back and flush someone else's L1**. For pCPU_A's writes to become visible to pCPU_B, you rely on ARM's Inner Shareable cache coherency protocol — the writer does a `dsb ish/sy` at the right time, the reader does a `dsb` plus any necessary cache maintenance, both halves together. A bare `smc` is neither a barrier nor a coherency trigger.

I first added `dsb sy` on the reader side. More stable than before, still flaky. The reason isn't hard to guess: the writer (pKVM) is in code I don't control, and I can't assume it does the right barrier after writing. Add cross-world, cross-pCPU, shared NWd DRAM on top — one reader-side barrier obviously isn't enough.

### Fix: barrier + local copy + bounded parse

Real code (`src/spmc_handler.rs:2370-2432`):

```rust
// src/spmc_handler.rs
// DSB SY: ensure NWd's TX buffer writes are visible to S-EL2.
// pKVM's per-CPU SPMD may enter S-EL2 on a different physical CPU
// than the one that wrote the descriptor — L1 D-cache can be stale.
// SAFETY: DSB SY is a barrier instruction with no side effects.
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }

let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8,
        local_buf.as_mut_ptr(),
        total_length as usize,
    );
}

// NEVER parse from the shared buffer directly — all parsing reads local_buf.
let parsed = parse_mem_region(local_buf.as_ptr(), total_length);
```

After this, the intermittent Data Abort stopped reproducing in tests.

Why does the local copy fix what the barrier doesn't?

Be careful not to overclaim: `copy_nonoverlapping` doesn't guarantee that the bytes it copies represent a snapshot at any specific producer moment — the copy can still straddle a mix of fresh and stale cache lines. What changes is that the parser only ever reads a fixed local copy. `parse_mem_region` runs bounded checks against that fixed byte stream. Either it parses, or it returns `FFA_INVALID_PARAMETERS` for an out-of-range offset/length — it doesn't chase pointers into the shared buffer.

That demotes the failure mode from "non-recoverable Data Abort" to "occasional `FFA_INVALID_PARAMETERS`". The first is brutal to debug; the second is obvious.

### Lesson

For cross-world + cross-pCPU shared buffers, don't parse in place. **`dsb sy`, copy to a local stack/static buffer, parse only from the local copy.**

What this pattern actually buys you isn't "the snapshot you see is from the producer's moment" — the copy can still straddle old and new cache lines. What it buys you is that **the parsing stage isn't chasing pointers anymore**: bounded parse runs over a frozen byte stream, succeeds, or returns a structured error for offset/length violations. The cross-core visibility problem isn't gone, but the consequence of "reading bad data" drops from "unrecoverable Data Abort" to "return error code" — orders of magnitude easier to debug.

While we're here, a related misconception worth clearing up: **SMC does not migrate the request to another CPU.** It's a synchronous exception, always handled on the originating CPU. But an SP resumed via `FFA_RUN` can run on any pCPU — that's SP scheduling, not SMC migration. Conflating the two leads to wrong barrier logic.

---

## Pitfall 3: QEMU `-bios` Doesn't Pass DTB to BL33

**Week 2.** The hypervisor was about to start parsing DTB. Initial code, looked fine:

```rust
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_addr as *const u8) };
    let uart = find_uart(&fdt);
    let gic = find_gic(&fdt);
    // ...
}
```

QEMU in `-kernel` mode follows the Linux kernel boot protocol: x0 = DTB physical address. Grab it, parse it.

Worked fine for weeks. Switched to `-bios` mode in week 4 (to run TF-A) — boot hangs inside `fdt::Fdt::from_ptr`. Print x0 — **0**.

QEMU source (`hw/arm/virt.c`) plus the TF-A QEMU platform docs explain it:

- `-kernel` mode: QEMU constructs the DTB itself, places it in RAM, passes the address in x0 to the kernel entry.
- `-bios` mode: QEMU leaves the DTB in memory, BL2 picks it up as `HW_CONFIG`. **What BL33 receives in x0 depends on TF-A's build options:**
  - `ARM_LINUX_KERNEL_AS_BL33=1`: the FDT address is placed in BL33's x0 (the Linux-as-BL33 path).
  - Default BL33 arg in TF-A's QEMU port (no `ARM_LINUX_KERNEL_AS_BL33`): BL33 entry registers carry the MPIDR low bits — the boot CPU's `x0` ends up at 0, secondary CPUs see their MPIDR. There's no DTB pointer there.

My setup is "SPMC + custom hypervisor as BL33", without `ARM_LINUX_KERNEL_AS_BL33`. The first version of my code assumed `x0 = DTB`, which is wrong — what I actually got was the boot CPU's MPIDR low bits (i.e., 0). The general lesson: **BL33's x0 doesn't have a fixed semantic you can assume — it depends entirely on how TF-A is configured.**

This isn't a QEMU bug — "bootloader boot conventions" don't have a unified standard. `-kernel` follows the Linux boot protocol, `-bios` follows TF-A's `HW_CONFIG` convention (further modulated by BL33 build options), real hardware follows whatever each vendor's UEFI/coreboot does. If your hypervisor hard-codes "x0 is the DTB," you've coupled it to one specific environment.

### Fix: Every Boot Parameter Has a Fallback

Real code in `src/dtb.rs`:

```rust
// src/dtb.rs
/// Global platform info with QEMU virt defaults.
static PLATFORM_INFO: PlatformInfoCell = PlatformInfoCell {
    inner: UnsafeCell::new(PlatformInfo {
        uart_base: 0x0900_0000,
        gicd_base: 0x0800_0000,
        gicr_base: 0x080A_0000,
        gicr_size: 0,
        num_cpus: 4,
        ram_base: 0x4000_0000,
        ram_size: 0x4000_0000,
    }),
    initialized: AtomicBool::new(false),
};

pub fn init(dtb_addr: usize) {
    if let Some(info) = parse_host_dtb(dtb_addr) {
        unsafe { *PLATFORM_INFO.inner.get() = info; }
        PLATFORM_INFO.initialized.store(true, Ordering::Release);
    }
    // If DTB parsing fails, defaults stay; the hypervisor keeps running.
}

fn validate_dtb_address(addr: usize) -> bool {
    if addr == 0 { return false; }
    if !(0x4000_0000..0x8000_0000).contains(&addr) { return false; }
    let magic = unsafe { core::ptr::read_volatile(addr as *const u32) };
    u32::from_be(magic) == 0xD00D_FEED
}
```

Two things to note:

1. **Defaults are `static`-initialized**, not "panic if DTB parsing fails." If I run with hardcoded QEMU virt defaults under `-bios`, everything still works.
2. **`validate_dtb_address` does triple validation** — non-zero, in-RAM range, correct magic. Because x0 might be 0, might be random junk, might point to a totally unrelated chunk of memory. Having a bad DTB address cause an `fdt` crate panic is pointless; just fall back to defaults.

### Lesson

Writing a hypervisor isn't like writing userspace — you can't assume the environment "set everything up for you." DTB might not arrive; GIC register initial values might be random; UART might not have been enabled by the upstream bootloader.

**Every "should-already-be-set" assumption needs either a fallback path or a panic message.** Defaults + try-to-update-from-environment + fall-back-to-defaults-on-failure — this pattern already lets my hypervisor share one codebase across QEMU `-kernel` and `-bios` paths.

Extending it to real hardware needs more. The current `validate_dtb_address()` hardcodes the DTB-address range to QEMU virt's RAM (`0x4000_0000..0x8000_0000`). On a different machine or board, that range needs adjusting; a "truly generic fallback" would need to read `/memory` nodes / UEFI memmap, or simply accept any address that passes magic validation outside known-bad regions. I haven't done that because my target platform is just QEMU virt — but for a production hypervisor, this layer can't be skipped.

---

## In Summary: Three Blind Spots in Bare-Metal Rust

Looking back at all three pitfalls, the common patterns:

1. **Compiler codegen assumes a "normal OS environment"**
   Debug asserts using NEON, allocators relying on page-fault handling, panic handlers assuming stdout — these assumptions become bugs on bare metal.

2. **Hardware memory model has more details than you remember**
   Cache coherency, the NS bit, SMC not being a barrier, `dsb ish` vs `dsb sy`, Inner vs Outer Shareable — each is simple in isolation, but the combination can eat a whole day.

3. **Bootloader conventions are non-standard**
   `-kernel` / `-bios` / UEFI / coreboot / TF-A each go their own way. What x0/x1 contain, the initial register state, the stack pointer — every environment is different.

Each pitfall has a short explanation. The reason they each take days to debug is that **your mental model assumes some premise that doesn't hold at this layer** — assuming SMC is a barrier, that `read_volatile` only does a memory read, that the DTB is always in x0 at boot.

The most important skill in writing bare-metal Rust isn't Rust. It's: **when you see an "impossible" symptom, reach for the Arm ARM, TF-A source, and LLVM lowering rules — not your compiler's bug tracker.** Below the abstraction layer there's no OS to bail you out. Your real adversary isn't your code; it's the gap between your mental model of the hardware and how the hardware actually behaves.

There's no shortcut to closing that gap — **disassemble, read the manual, read the firmware source.**

---

**Code**: [github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor)

**Blog**: [willamhou.github.io/hypervisor](https://willamhou.github.io/hypervisor/)

*This is part 7 of the ARM64 Hypervisor development series — the last long-form of this week. The Chinese version is the canonical source — see [part7-bare-metal-rust-pitfalls.md](https://github.com/willamhou/hypervisor/blob/main/docs/zhihu/part7-bare-metal-rust-pitfalls.md).*
