---
title: "Three days chasing the NS bit: how ARM's TrustZone address space caught me"
published: true
description: "A war story from an ARM64 hypervisor — TrustZone isn't a permission model, it's an AXI bus signal"
tags: arm, rust, embedded, security
canonical_url: https://willamhou.github.io/hypervisor/
---

> Publish date: 4/24

Week 8 of my ARM64 hypervisor project. I spent three days on a bug where data writes succeeded, the addresses were correct, and the reader saw zeros. This post walks through what I learned about **how TrustZone actually works at the bus level** — because my mental model was wrong.

## Setup

My hypervisor runs at S-EL2 managing Secure Partitions. Android's pKVM runs at NS-EL2 on the same chip. They communicate through EL3 firmware using the FF-A protocol. One call, `PARTITION_INFO_GET`, has pKVM read a descriptor my SPMC writes into its RX buffer.

## The bug

My internal BL33 test harness (Normal World EL1) calls `PARTITION_INFO_GET`, works fine. pKVM calls the same function — **pKVM reads zeros**.

GDB on the SPMC side, inspecting `0x42a16000`:

```
(gdb) x/6gx 0x42a16000
0x42a16000: 0x8001000000010001 0x0000000100000000
0x42a16010: 0x0008000000000000 0x0000000000000000
```

Data is there. pKVM reads the same address: all zeros.

## What TrustZone actually is

I had the wrong mental model. I thought TrustZone was a permission model — Secure code can access Normal memory, Normal code cannot access Secure memory.

**It's not.** TrustZone introduces an extra signal on the AXI bus: the **NS bit**.

- Accesses from S-EL2 default to NS=0 (Secure)
- Accesses from NS-EL2/NS-EL1 are always NS=1 (Non-Secure)

A **single DRAM chip** can serve both worlds. A TrustZone Address Space Controller (TZASC, or TZC-400) sits between the CPU and the DRAM controller. It sees the NS bit on each AXI transaction and enforces region-based access control: certain physical address ranges respond only to NS=1 requests, others only to NS=0.

So `0x42a16000` in the Secure world and `0x42a16000` in the Non-Secure world are **two different logical regions of the same DRAM**, enforced by bus-level access control. They hold independent data.

## The fix

Two steps:

1. Enable S-EL2 Stage-1 MMU
2. Mark Normal World DRAM regions with NS=1 in the page table entries

Stage-1 PTEs have an NS bit. Default 0 means "access is Secure." Setting it to 1 means "emit NS=1 on the AXI transaction."

Post-fix mapping:

```
0x00000000 - 0x08000000: Device Secure    (GIC, UART)
0x40000000 - 0x80000000: Normal NS=1      (pKVM's DRAM — cross-world)
0x0E000000 - 0x10000000: Normal Secure    (SPMC code and data)
```

When the SPMC writes to `0x42a16000`, Stage-1 MMU marks the AXI transaction NS=1. TZASC routes it to the Non-Secure region. pKVM sees the write.

## Why BL33 tests "worked"

My earlier BL33 tests ran without QEMU's strict `secure=on` mode. That configuration doesn't fully separate Secure and Non-Secure memory. When I moved to the pKVM scenario with full TF-A + `secure=on`, the two worlds became actually isolated, and the bug emerged.

Lesson: **test harness and production environment may have different Secure/NS enforcement**. If your test harness doesn't run at the same privilege level and world as the actual target, verify the test environment's isolation matches production.

## Common misconceptions I had

**Misconception 1:** "Secure and Non-Secure are different DRAM chips."
**Reality:** Same DRAM. TZASC enforces at the AXI bus, by address range.

**Misconception 2:** "SMC is a memory barrier."
**Reality:** ARMv8-A specifies SMC as a synchronous exception with a Context Synchronization Event. **No memory ordering guarantees.** Cross-CPU data sharing needs explicit `dsb ish/sy`.

**Misconception 3:** "SMC routes subsequent processing to a different CPU."
**Reality:** SMC is a synchronous exception, always handled on the originating CPU. But an SP resumed via `FFA_RUN` can execute on a different pCPU — that's SP migration, not SMC migration.

## Postscript

Three days on this bug. Every "it should just work" moment was me defaulting to the wrong mental model.

If you're building an ARM64 hypervisor or doing cross-world memory sharing: **every cross-world buffer needs explicit NS attribute on its page table entries**. It's not permission enforcement, it's bus-level tagging. This is fundamental to TrustZone, and the most commonly missed detail.

---

Code: https://github.com/willamhou/hypervisor
Blog: https://willamhou.github.io/hypervisor/
