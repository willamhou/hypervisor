---
title: "3 pitfalls writing bare-metal Rust on ARM64"
published: true
description: "The Rust compiler is the Rust compiler. But the hardware has its own ideas."
tags: rust, embedded, arm, systemsprogramming
canonical_url: https://willamhou.github.io/hypervisor/
---

> Publish date: 4/26

I've been writing an ARM64 bare-metal hypervisor in `no_std` Rust for 10 weeks. No OS, no libc, no runtime. The language behaves exactly like it does in your CLI projects — but the hardware doesn't. Here are three pitfalls from this codebase, each one a case of "Rust is fine, but the hardware has opinions."

## Pitfall 1: Debug-mode NEON popcount

Week 4. SPMC works in release. Debug mode hangs on boot. No output, no fault.

GDB: CPU stuck in an EL3 exception handler. `ESR_EL3` shows `EC=0x07` — FP/SIMD exception.

But my hypervisor doesn't use floating point. `no_std`, no `f32/f64`.

`ELR_EL3` points to a `read_volatile(mmio_addr)` call. Disassembly:

```
  200140:	cnt	v0.8b, v0.8b
  200144:	addv	b0, v0.8b
  200148:	umov	w0, v0.b[0]
```

`cnt v0.8b, v0.8b` is NEON SIMD — **popcount**.

Why would `read_volatile` have a popcount? Tracing through:

`read_volatile` in debug mode runs an alignment assert. The assert includes `debug_assert!(align.is_power_of_two())`. `is_power_of_two` is implemented as `popcount(x) == 1`. LLVM lowers popcount on AArch64 to `cnt`.

In release mode, `debug_assert!` is removed, the NEON instruction disappears.

TF-A sets `CPTR_EL3.TFP=1` by default, trapping FP/SIMD from EL2 and below. S-EL2 executes NEON → trap to EL3 → EL3's handler isn't prepared for this trap → infinite loop.

Fix: build TF-A with `CTX_INCLUDE_FPREGS=1`.

**Lesson**: Rust's `debug_assert!` can contain instructions you didn't expect (popcount → NEON). On bare metal, any debug assert can trigger unexpected hardware behavior. Either build release-only, or confirm FP/SIMD is available at your exception level.

## Pitfall 2: Cross-pCPU buffer visibility

Week 11. pKVM and my SPMC share memory via FF-A. pKVM writes a descriptor on CPU 0, issues `smc #0` to EL3, which ERETs into my SPMC at S-EL2.

The catch: **the SP can later be resumed via `FFA_RUN` on CPU 2** (SP scheduling allows migration across pCPUs). When the SP on CPU 2 tries to read pKVM's buffer, CPU 2's L1 cache may be stale.

30% of the time, my `composite_offset` read as garbage (`0x240f` when it should be 80).

**A common misconception**: SMC is a memory barrier. **It's not**. ARMv8-A specifies SMC as a synchronous exception with only a Context Synchronization Event — no memory ordering guarantees. Cross-CPU visibility needs explicit `dsb ish` or `dsb sy`.

Adding `DSB SY` helped but didn't eliminate the issue. Final fix: **copy to local stack first, then parse**:

```rust
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)); }
let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8, local_buf.as_mut_ptr(), total_length,
    );
}
let desc = parse_mem_region(local_buf.as_ptr(), total_length);
```

The copy is multiple reads. ARM's memory model guarantees read-after-read consistency on one CPU. Even if `local_buf` captures a stale snapshot, it's at least **self-consistent** — bounds checks reject it cleanly instead of dereferencing wild pointers.

30% crash rate → 0.

**Lesson**: Cross-world or cross-pCPU shared buffers — even with `DSB SY`, the safest pattern is "copy locally, then parse." Self-consistency beats freshness when you can't get both.

## Pitfall 3: QEMU's `-bios` mode doesn't pass DTB to BL33

Week 2. Hypervisor booting, DTB parsing:

```rust
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_addr as *const u8) };
    // ...
}
```

QEMU passes DTB address in x0 in `-kernel` mode. Switch to `-bios` mode (running TF-A), DTB address is **0**.

QEMU source confirms: in `-bios` mode, QEMU passes DTB as BL2's `HW_CONFIG` to BL31 (EL3), not to BL33. My hypervisor is BL33.

Fix: hardcode QEMU virt defaults, fall back when DTB is absent:

```rust
pub const UART_BASE: u64 = 0x0900_0000;
pub const GICD_BASE: u64 = 0x0800_0000;
pub const GICR_BASE: u64 = 0x080A_0000;
```

**Lesson**: Bootloader conventions aren't standardized. `-kernel` and `-bios` pass args differently; real hardware is another convention; each TF-A configuration is another. Your hypervisor needs a fallback for "DTB not received."

## Summary

Rust on bare metal has no surprises at the language level. But you'll hit:

1. **Compiler codegen assumptions** (debug asserts, SIMD, allocator availability) expect a normal OS
2. **Hardware memory model details** (cache coherency, NS bit, SMC is not a barrier)
3. **Bootloader environment variance** (DTB passing, boot conventions, initial register state)

Each pitfall alone is simple. Together they cost weeks. The key skill isn't Rust — it's **when you see an "impossible" symptom, reach for the ARM ARM and TF-A source, not the compiler bug tracker**.

---

Code: https://github.com/willamhou/hypervisor
Blog: https://willamhou.github.io/hypervisor/
