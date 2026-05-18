---
title: "Three Days of Debugging the TrustZone NS Bit: It Isn't a Permission, It's an Address-Space Selector"
published: true
description: "Same DRAM, same physical address — SPMC writes data, pKVM reads zeros. The bug wasn't permissions, wasn't cache; it was an attribute bit on the AXI bus I'd never thought about."
tags: arm, trustzone, embedded, systems
series: "ARM64 Bare-Metal Hypervisor in Rust"
---

> **Disclaimer**: This is about an experimental hypervisor that only runs on QEMU virt — no real-hardware validation yet. The architectural facts about TrustZone are spec-grounded; the platform-specific details (which NS controller, which DRAM topology) come from QEMU and TF-A and may not generalize to every SoC.

I'm writing an ARM64 bare-metal hypervisor. It runs at S-EL2 — the Secure-world hypervisor level. On the same chip, Google's pKVM runs at NS-EL2 (the Normal-world hypervisor level). The two communicate via SMC calls, relayed by TF-A at EL3, using FF-A as the protocol.

One of those calls is `FFA_PARTITION_INFO_GET`. pKVM uses it to ask my SPMC, "what Secure Partitions (SPs) are you managing?" The SPMC writes a list of SP descriptors into pKVM's RX buffer; pKVM reads them back.

This post is about a 3-day bug on that one call — the kind that doesn't fit any failure mode I'd seen before. The root cause was that my mental model of TrustZone had been missing one critical piece from day one.

---

## Symptom: Writes Succeed, Reads Return Zero

I first validated `PARTITION_INFO_GET` with my own BL33 test program — a small C harness running at NS-EL1 that issues the SMC, reads the RX buffer, checks the descriptor format. It worked. Whatever SPMC wrote, BL33 read; the bytes were identical.

Swap in pKVM as the caller. pKVM issues the same SMC, gets `x2` (the descriptor count) correctly — but reads from its own RX buffer come back **all zeros**.

First instinct: endianness or layout bug. I attached GDB to QEMU, looked at the SPMC side at the buffer pKVM had registered (`0x42a16000`):

```text
(gdb) x/6gx 0x42a16000
0x42a16000: 0x8001000000010001 0x0000000100000000
0x42a16010: 0x0008000000000000 0x0000000000000000
```

The data is right there. Switch to pKVM's view (NS-EL2), read the same address: zeros.

**Writes succeeded** (volatile stores from SPMC, GDB sees them). **The address is right** (it's the one pKVM registered with `FFA_RXTX_MAP`). **Not a cache issue** (added `dsb sy` + `dc civac`, no change). **Not a pKVM parsing bug** (pKVM got the right partition count).

This wasn't any failure mode I recognized.

---

## Three Wrong Hypotheses

Before I understood what was actually happening, I tried — and ruled out — these:

**Hypothesis 1: QEMU bug**
"Maybe QEMU's Secure/Non-Secure isolation is broken." Nope. QEMU was right; **my mental model was wrong**.

**Hypothesis 2: FF-A spec requires explicit cache flushes**
Re-read the FF-A spec for an explicit `cache flush` requirement. The spec says shared memory regions should be Normal WB + Inner Shareable, but says nothing about Secure/NS attributes — it assumes the implementation knows.

**Hypothesis 3: I'm interpreting the RX buffer address wrong on my side**
Was `0x42a16000` actually a pKVM IPA (its own Stage-1 virtual address)? In this implementation we treat it as a physical address — pKVM's RXTX path passes physical addresses directly (see how `NWD_RXTX` is handled in `src/spmc_handler.rs`). So the SPMC has a PA in hand and doesn't need another layer of translation.

All three wrong. The truth was: **"physical address" in a TrustZone system isn't just a number.**

---

## TrustZone Isn't Just a Permission Model

I went back to the Arm Architecture Reference Manual section on `Secure and Non-secure memory`, then TF-A's Secure Partition Client Interface docs and the TZC-400 (TrustZone Address Space Controller) manual. The piece I'd been missing clicked into place.

My old mental model:

> TrustZone = a permission bit. Secure code can access any memory; Normal code can only access "non-secure" memory. The CPU's mode determines which side it belongs to.

This explains 90% of tutorial examples. But it **misses one critical detail**: it assumes that "the access target" (some chunk of physical memory) intrinsically belongs to one side, and permissions just decide who can see it.

The fuller model:

> TrustZone isn't just a permission model — architecturally it defines **two separate physical address spaces**: Secure and Non-secure. Every load/store the CPU emits carries an `NS` (Non-Secure) attribute, and that attribute decides which address space the access lands in. The enforcement varies by SoC (Arm's own TZASC / TZC-400 series, vendor-specific NS controllers), but they all do the same thing: arbitrate access by the transaction's `NS` attribute.

Put differently: **the same numeric address, in the Secure vs Non-secure physical address space, is two different architectural addresses**.

- An `NS=0` transaction to `0x42a16000` → goes to the Secure physical address space
- An `NS=1` transaction to `0x42a16000` → goes to the Non-secure physical address space

Whether the underlying storage is the same DRAM chip with different regions, or genuinely separate memories, is an SoC implementation detail. What matters at the architectural level is: **there are two physical address spaces**, and the memory system (TZASC, TZC-series, or a vendor's NS controller) decides which one this transaction targets based on `NS`.

That's why my SPMC succeeded writing to `0x42a16000` and pKVM read zeros from `0x42a16000` — both "succeeded," but architecturally they were addressing two different address spaces.

---

## From CPU to Bus: Who Decides the NS Bit?

The next question: **for a given CPU load/store, who decides what `NS` value it carries?**

The answer is in the Stage-1 MMU.

When the CPU is in Non-Secure world (NS-EL1/NS-EL2), the hardware forces every transaction's `NS` to 1. No matter what pKVM does, it's `NS=1`.

When the CPU is in Secure world (S-EL1/S-EL2/EL3), **the `NS` bit comes from the Stage-1 PTE's `NS` bit**:

- Stage-1 MMU disabled → `NS=0` by default
- Stage-1 MMU enabled → bit 5 of each PTE decides this access's `NS`
  - PTE `NS=0` → transaction `NS=0` (Secure access)
  - PTE `NS=1` → transaction `NS=1` (Non-Secure access)

My bug was simple: **the SPMC didn't have its S-EL2 Stage-1 MMU enabled**.

Why not? The SPMC's boot path is short, and its own code and data live in Secure DRAM, so it ran fine with the MMU off. But once it tried to reach into `0x42a16000` (pKVM's buffer in Normal DRAM), the hardware default kicked in: `NS=0`, the transaction got routed to the Secure region by the TZC, and pKVM never saw the write.

---

## Fix: A Minimal Identity Map for S-EL2

The fix is straightforward: **enable Stage-1 MMU during SPMC init and mark NWd DRAM PTEs with `NS=1`**.

I don't need full virtual memory — the SPMC has no userland and doesn't switch address spaces. An identity map covering four regions is enough:

```rust
// src/sel2_mmu.rs
// L1[1] = 1GB block at 0x4000_0000, NS=1, Normal WB, XN
S1_L1.0[1].store(
    0x4000_0000u64 | PTE_VALID | PTE_BLOCK | NORMAL_NS_XN,
    Ordering::Relaxed,
);

// L1[2] = 1GB block at 0x8000_0000, NS=1, Normal WB, XN
S1_L1.0[2].store(
    0x8000_0000u64 | PTE_VALID | PTE_BLOCK | NORMAL_NS_XN,
    Ordering::Relaxed,
);

// L2 blocks for SPMC + SPs + heap (0x0E00_0000..0x0FFF_FFFF): NS=0
for idx in 112..=127 {
    let addr = (idx as u64) << 21;
    S1_L2_LOW.0[idx].store(addr | PTE_VALID | PTE_BLOCK | NORMAL_S, Ordering::Relaxed);
}

// L2 blocks for GIC + UART (0x0800_0000..0x09FF_FFFF): NS=0, Device
for idx in 64..=79 {
    let addr = (idx as u64) << 21;
    S1_L2_LOW.0[idx].store(addr | PTE_VALID | PTE_BLOCK | DEVICE_S, Ordering::Relaxed);
}
```

The layout is straightforward:

```text
0x00000000 - 0x07FFFFFF:  (unmapped)
0x08000000 - 0x09FFFFFF:  Device, NS=0    GIC + UART
0x0E000000 - 0x0FFFFFFF:  Normal, NS=0    SPMC code + SPs + heap
0x40000000 - 0x7FFFFFFF:  Normal, NS=1    pKVM's DRAM (1st GB)
0x80000000 - 0xBFFFFFFF:  Normal, NS=1    pKVM's DRAM (2nd GB)
```

Then the standard MMU bring-up. The actual order in the code is: `dsb ishst` to ensure page-table stores are visible, `tlbi alle2` to invalidate S-EL2 TLBs, `dsb ish` to wait for the TLB invalidation, `isb` to synchronize the pipeline; then write `MAIR_EL2`/`TCR_EL2`/`TTBR0_EL2` followed by an `isb`; finally set `M`/`C`/`I` in `SCTLR_EL2` followed by an `isb`.

After that, when the SPMC accesses `0x42a16000`, Stage-1 walks to L1[1] (`0x42a16000 >> 30 == 1`), the PTE has `NS=1`, the hardware tags the transaction `NS=1`. The memory system (TZASC, TZC-series, the SoC's NS controller) routes it to the Non-secure physical address space. pKVM sees the data immediately.

Same on secondary CPUs — `install_sel2_stage1_secondary()` reuses the primary's page tables, each CPU runs its own `msr ttbr0_el2, ...` + MMU enable. Page tables are shared; MMU activation is per-CPU.

---

## A Few Plausible-Sounding Things That Are Wrong

A few wrong conclusions I almost talked myself into during this debug, all backed by some doc or tutorial somewhere:

**"Secure and Non-Secure must be different DRAM chips"**
Not necessarily. Architecturally you only need two distinct **physical address spaces**. The implementation can be different chips, or different regions in the same DRAM arbitrated by an NS controller like TZC-400. A lot of ARM tutorials draw two memory blocks side-by-side, which gives the impression of physical separation.

**"The SMC instruction is a memory barrier"**
It isn't. The Arm ARM (DDI 0487) defines `SMC` as a synchronous exception that causes a Context Synchronization Event but **gives no guarantees about memory ordering**. Cross-CPU shared data needs explicit `dsb ish` or `dsb sy`.

**"SMC routes the request to a different CPU"**
It doesn't. SMC is a synchronous exception — it's always handled on the originating CPU. But an SP resumed via `FFA_RUN` can run on any pCPU. That's SP scheduling, not SMC migration. Conflating the two leads to wrong barrier logic.

**"QEMU virt with `secure=on` is equivalent to without"**
In my setup, no. My early BL33 harness ran without TF-A + `secure=on` and passed; only when I switched to the full TF-A + `secure=on` configuration did the NS-bit bug surface. Strictly speaking this conclusion is QEMU-virt-specific, but the general lesson holds: **make the test environment's isolation match production before you trust the test results.**

---

## In One Sentence

In a TrustZone system, a physical address isn't a number — it's a `(address, NS)` tuple.

When code runs in the Secure world via S-EL2 Stage-1 translation, the PTE's `NS` bit decides: MMU off → `NS=0` by default, hitting the Secure physical address space; MMU on → the PTE's `NS` bit decides. To make a Secure-side CPU store land in the Non-secure physical address space, the most direct approach is to mark that address range with `NS=1` in the Stage-1 page tables.

It's not a permission problem. It's a routing problem.

---

**Code**: [github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor)

**Blog**: [willamhou.github.io/hypervisor](https://willamhou.github.io/hypervisor/)

*This is part 6 of the ARM64 Hypervisor development series. The Chinese version is the canonical source — see [part6-trustzone-ns-bit.md](https://github.com/willamhou/hypervisor/blob/main/docs/zhihu/part6-trustzone-ns-bit.md).*
