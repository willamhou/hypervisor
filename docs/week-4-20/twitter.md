# Twitter 推文（一天一条，4/20 - 4/26）

---

## 周一 4/20 — awesome-embedded-rust 收录

```
My bare-metal ARM64 hypervisor landed in awesome-embedded-rust 🎉

It's the only Type-1 hypervisor on that list — the rest are RTOSes and MCU firmware.

~21K lines of no_std Rust (non-test), one external dep: fdt for device tree parsing.

https://github.com/willamhou/hypervisor
```

---

## 周二 4/21 — Rust enum 状态机

```
Added a new SP state transition (Blocked → Preempted) for chain preemption.

The compiler found two places I forgot to handle it.

I still store state as u8 + AtomicU8 for cross-CPU CAS. Exhaustive match on the transition tuple makes new edges hard to miss.
```

---

## 周三 4/22 — ARM 双地址空间

```
TrustZone war story: it isn't just a permission bit. It's a second address space on the AXI bus.

Same DRAM, same physical address, different NS bit, different region.

I spent 3 days writing from S-EL2 to memory pKVM could never see.
```

---

## 周四 4/23 — 单依赖

```
My hypervisor has one external dependency: `fdt` for DTB parsing.

Everything else — page tables, GICv3, virtio-blk/net, FF-A, the SPMC event loop — is handwritten.

Reason: one stray `default-features = ["std"]` in bare metal can ruin your day.
```

---

## 周五 4/24 — NEON SIMD 陷阱

```
Release mode booted. Debug mode hung on the first MMIO read.

Cause: `debug_assert!(align.is_power_of_two())` inside `read_volatile` compiled to NEON popcount (`cnt`). TF-A traps FP/SIMD from EL2.

Below the OS, codegen is part of the hardware contract.
```

---

## 周六 4/25 — Codespaces

```
Want to try the ARM64 hypervisor without building a toolchain?

Open it in Codespaces, wait for the container, run `make run`.

Rust nightly, cross gcc, and QEMU are already there. 457 assertions finish in ~5s.

https://github.com/willamhou/hypervisor
```

---

## 周日 4/26 — 本周总结

```
Week recap from the ARM64 Rust hypervisor project:

- merged into awesome-embedded-rust
- merged into awesome-confidential-computing
- sent to Rust OSDev Monthly

Next step is real hardware instead of QEMU. If you have N1SDP or FVP, DM me.

https://github.com/willamhou/hypervisor
```
