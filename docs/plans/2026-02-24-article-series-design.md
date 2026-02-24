# Design: "Scratch a Rust Hypervisor" Article Series

**Date**: 2026-02-24
**Status**: Draft

## Overview

A top-down, bilingual (EN/ZH) article series documenting how a bare-metal ARM64 hypervisor was built from scratch in Rust using AI pair programming (Claude Code). Published as an mdBook in the current repo, deployable to GitHub Pages.

## Core Narrative

**Dual-track**: Each article interleaves technical implementation with AI collaboration insights.

**Story arc**: An engineer who previously built a production hypervisor at a big tech company (10-month dev cycle + certification) rebuilds one from scratch in ~1 month using Claude Code and vibe coding.

## Target Audience

- Embedded/systems engineers wanting to learn Rust for bare-metal
- Rust developers curious about ARM virtualization
- General technical readers interested in hypervisor internals
- Security/virtualization researchers following ARM CCA/pKVM/FF-A
- Developers interested in vibe coding and AI-assisted systems programming

## Publishing Strategy

| Platform | Language | Format |
|----------|----------|--------|
| GitHub Pages (mdBook) | EN + ZH | Long-form canonical source |
| Twitter/X | EN | Short post + link to chapter |
| 知乎 | ZH | Long-form (copy from zh/ src) |
| 微信公众号 | ZH | Long-form (copy from zh/ src) |

## Repository Structure

```
docs/book/
├── book.toml                    # mdBook config (EN default, ZH as language)
├── en/
│   └── src/
│       ├── SUMMARY.md           # Table of contents
│       ├── part0-prologue/
│       │   ├── README.md        # Part overview
│       │   ├── background.md    # 0.1 Personal background
│       │   ├── motivation.md    # 0.2 Project motivation
│       │   ├── ai-workflow.md   # 0.3 AI pair programming workflow
│       │   └── toolchain.md     # 0.4 Toolchain and environment
│       ├── part1-first-boot/
│       │   ├── README.md
│       │   ├── arch.md
│       │   ├── impl.md
│       │   ├── test.md
│       │   └── debug.md
│       ├── part2-vcpu/
│       │   └── ...
│       └── ...
└── zh/
    └── src/
        ├── SUMMARY.md
        ├── part0-prologue/
        └── ...
```

## Article Tree

### Part 0: Prologue — Why and How

| Section | Content | Source |
|---------|---------|-------|
| 0.1 Personal Background | AI + systems software dual background. Co-founder of AI startup. Built production hypervisor at big tech (10-month dev + certification + commercial deployment). | Author writes first draft |
| 0.2 Project Motivation | Mid-2025: idea sparked but "Rust alone won't work". Late 2025: Claude Code major update reignites the idea. 10 months → 1 month narrative hook. | Author writes first draft |
| 0.3 AI Workflow | Claude Code + vibe coding concrete process: planning → TDD → code review → commit loop. CLAUDE.md as project knowledge base. Agent orchestration. | Derived from .claude/ config |
| 0.4 Toolchain | QEMU virt, aarch64-linux-gnu-gcc, Rust nightly, no_std, custom target spec. Docker builds. | Derived from Makefile, build.rs |

**Commits**: #1-4 (`9b72719..b2ff49f`)

### Part 1: First Boot — From Zero to EL2

| Section | Content |
|---------|---------|
| Arch | ARM64 exception levels (EL0-EL3), EL2 hypervisor mode, HCR_EL2, VTTBR_EL2 |
| Impl | `boot.S` (EL2 entry, stack setup, BSS zero), `linker.ld` (0x40200000 base), first `rust_main()`, UART `uart_puts` |
| Test | QEMU `-nographic` serial output verification |
| Debug | Page table alignment, EL2 entry conditions, QEMU `-machine virtualization=on` |

**Commits**: #1-4 (`9b72719..b2ff49f`)

### Part 2: vCPU and Guest Execution

| Section | Content |
|---------|---------|
| Arch | VM entry/exit loop, VcpuContext (x0-x30, SP, PC, SPSR), ERET semantics, Stage-2 translation |
| Impl | Sprint 1.1 (Vcpu struct, state machine), Sprint 1.2 (IdentityMapper, Stage-2 pages), `enter_guest()`/`exit_guest()` in assembly |
| Test | HVC hypercall test, simple_guest inline test |
| Debug | Stage-2 translation errors, guest code memory bugs |

**Commits**: #5-8 (`f159a89..6aa6cb8`)

### Part 3: Exception Handling and GICv3

| Section | Content |
|---------|---------|
| Arch | Exception vector table layout (VBAR_EL2), ESR_EL2 decoding, GICv3 virtual interface (ICH_LR, ICH_HCR, ICH_VMCR), MMIO trap-and-emulate |
| Impl | Sprint 1.3 (exception vectors, `handle_exception()`), Sprint 1.4 (DeviceManager enum dispatch, VirtualUart), Sprint 1.5 (GICv3 LR injection, EOImode) |
| Test | MMIO decode tests (ISS paths), GICv3 LR injection + ELRSR, complete interrupt flow |
| Debug | Infinite exception loop → halt, MMIO instruction encoding (external assembler for test), GICD/GICR shadow vs physical |

**Commits**: #9-26 (`35a4704..df429fe`)

### Part 4: Booting Linux

| Section | Content |
|---------|---------|
| Arch | DynamicIdentityMapper (2MB+4KB), HPFAR_EL2 for IPA (CRITICAL: FAR_EL2=VA not IPA), HW=1 timer virtualization, DTB runtime parsing |
| Impl | Dynamic page tables (heap-allocated), GICR full trap-and-emulate (Stage-2 unmapped 4KB pages), GICD write-through, virtio-blk (VirtioMmioTransport), Linux 6.12 kernel build (Docker), initramfs (BusyBox) |
| Test | 4 vCPU boot, `smp: Brought up 1 node, 4 CPUs`, BusyBox shell prompt, virtio-blk device visible |
| Debug | HPFAR_EL2 vs FAR_EL2 (shadow-only GICD hangs Linux), Stage-2 must cover full DTB-declared memory, `fdt` crate zero-copy parsing |

**Commits**: #27-66 (`e4b8e95..a1c1231`)

### Part 5: SMP — Multi-Core Virtualization

| Section | Content |
|---------|---------|
| 5a: Single-pCPU Multi-vCPU | Round-robin scheduler, PSCI CPU_ON signaling, SGI/IPI emulation (ICC_SGI1R_EL1 TALL1 trap), CNTHP preemption timer (10ms, INTID 26), VcpuArchState save/restore (GIC LRs, timer, EL1 sysregs, PAC keys) |
| 5b: Multi-pCPU 1:1 Affinity | TPIDR_EL2 per-CPU context, real PSCI CPU_ON SMC to QEMU firmware, physical GICR programming (`ensure_vtimer_enabled()`), SpinLock-protected DeviceManager, cross-pCPU IPI (`msr icc_sgi1r_el1`), WFI passthrough |
| Debug | `vcpu_online_mask` must include vCPU 0 at boot, `inject_spi()` deadlock (cannot acquire DEVICES lock), QEMU secondary CPUs powered off, ICC_SGI1R_EL1 bit field encoding |

**Commits**: #51-52, #67-84 (`90779fc..a73cb73`)

### Part 6: Multi-VM and Networking

| Section | Content |
|---------|---------|
| 6a: 2 VMs Time-Sliced | VMID in VTTBR_EL2 bits[63:48], per-VM global state (VmGlobalState), per-VM DeviceManager, two-level scheduler (`run_multi_vm()` → `run_one_iteration()`), memory partitioning (VM0@0x48000000, VM1@0x68000000) |
| 6b: Virtio-net + VSwitch | L2 virtual switch (16-entry MAC table), NetRxRing SPSC ring buffer, VirtioNet backend (device_id=1), TX→VSwitch→RX path, per-VM MAC (52:54:00:00:00:{id+1}), auto-IP (10.0.0.{id+1}/24), `virtio_slot()` abstraction |
| Debug | inject_rx descriptor leak (undersized descriptors must return to used ring), ifconfig shell arithmetic, linker script lost in merge |

**Commits**: #85-111 (`04477b4..35619ba`)

### Part 7: FF-A — Firmware Framework for Arm

| Section | Content |
|---------|---------|
| Arch | FF-A v1.1 spec overview, SMC trap via HCR_EL2.TSC, page ownership model (PTE SW bits [56:55]), SMC routing (is_ffa_function, PSCI vs FF-A) |
| Impl | FfaProxy (VERSION/ID_GET/FEATURES/RXTX/messaging/memory), stub SPMC (2 fake SPs), Stage2Walker (PTE read/write from VTTBR_EL2), descriptor parsing (FfaMemRegion/FfaCompositeMemRegion), SMC forwarding to EL3, VM-to-VM MEM_SHARE/RETRIEVE/RELINQUISH, 2MB→4KB block split, notifications (64-bit bitmaps), indirect messaging (MSG_SEND2) |
| Test | 44 assertions across VERSION/FEATURES/RXTX/messaging/memory/notifications, Stage-2 ownership validation, VM-to-VM integration test with real page tables |
| Debug | Stale VTTBR_EL2 in unit tests, probe_spmc() crash (QEMU EL3 firmware crashes on FFA_VERSION), S2AP transitions (SHARE→RO, LEND→NONE, RECLAIM→RW) |

**Commits**: #112-139 (`744679c..da94ff3`)

### Part 8: TF-A Boot Chain — Entering Secure World

| Section | Content |
|---------|---------|
| Arch | ARM Trusted Firmware boot flow: BL1→BL2→BL31(SPMD)→BL32→BL33, Secure vs Non-Secure world, EL3/S-EL2/NS-EL2 privilege model |
| Impl | Sprint 4.1 (Docker build infra, QEMU 9.2.3 `secure=on`), Sprint 4.2 (hypervisor as BL33 via PRELOADED_BL33_BASE), Sprint 4.3 (hypervisor as BL32/SPMC, `boot_sel2.S`, linker_sel2.ld@0x0e100000, manifest FDT parsing) |
| Test | "Hello from S-EL2!" serial output, BL33 → Linux → BusyBox shell through TF-A |
| Debug | CPTR_EL3.TFP traps FP/SIMD (CTX_INCLUDE_FPREGS=1), ROM overlap (QEMU 9.2+ fatal), FFA_MSG_WAIT ID wrong (0x84000071 vs 0x8400006B), DTB at 0x40000000 in -bios mode |

**Commits**: #140-146 (`b61e357..57beb0c`)

### Part 9: S-EL2 SPMC — Replacing Hafnium

| Section | Content |
|---------|---------|
| 9a: Event Loop + SP Boot | SPMC event loop (`dispatch_ffa()`), SpContext state machine (Reset→Idle→Running→Blocked→Preempted), Secure Stage-2 (VSTTBR_EL2), SPKG header parsing (img_offset=0x4000), SP1 (sp_hello) + SP2 (sp_irq) boot via ERET, BL33 FF-A test client (12 tests) |
| 9b: DIRECT_REQ End-to-End | `tfa_boot` feature, NS proxy → SPMD → SPMC → SP1 (x4 += 0x1000 proof), RXTX forwarding (SPMD manages NWd RXTX), PARTITION_INFO_GET (24-byte descriptors), Linux FF-A driver discovery |
| 9c: Interrupt Preemption + vIRQ/vFIQ | NS interrupt preemption (FFA_INTERRUPT → FFA_RUN resume), per-SP INTID ownership, HCR_EL2.VI injection (VBAR_EL1+0x280), HF_INTERRUPT_GET paravirt (0xFF04), CNTHP poll timer, cross-SP preemption, `vfiq` feature flag (HCR_EL2.VF, HF_FIQ_GET) |
| Test | 12/12 BL33 tests pass (11 base + 1 vFIQ), 42 unit test assertions (45 with vfiq), 28 SpContext assertions (40 with vfiq) |
| Debug | SCTLR_EL1/VBAR_EL1 stale from TF-A, UUID byte-swap in sp_mk_generator.py, SPMD framework message x1 encoding, tb_fw_config single cell, FEATURES must NOT advertise RXTX_MAP |

**Commits**: #147-177 (`9f8ce8d..b65045e`)

### Part 10: pKVM Integration — The Final Architecture

| Section | Content |
|---------|---------|
| Arch | Target: pKVM(NS-EL2) + our SPMC(S-EL2) coexistence. pKVM owns Normal World, our SPMC owns Secure World. FF-A as the interface. |
| Impl | AOSP android16-6.12 kernel (`gki_defconfig`, Docker build), S-EL2 Stage-1 MMU (`sel2_mmu.rs`: NS=1 for NWd DRAM), secondary CPU warm-boot (FFA_SECONDARY_EP_REGISTER, per-CPU stacks, `rust_main_sel2_secondary()`), per-CPU SPMC event loop |
| Test | pKVM Protected hVHE mode, FF-A v1.1 driver registered, SP1+SP2 discovered via `/sys/bus/arm_ffa/devices/` |
| Debug | SVE trap (ENABLE_SVE_FOR_NS=0 + sve=off), DTB memory layout (memory@40000000 2GB), PARTITION_INFO_GET x3=24, S-EL2 NS alias (writes to NWd RXTX hit Secure PA without MMU), binary_size single cell, per-CPU event loop (SPMD is per-CPU) |

**Commits**: #178-191 (`ae6ff25..4b31bb6`)

## Per-Article Template

Each Part follows this structure (scaled to complexity):

```markdown
# Part N: Title

> One-sentence summary of what we build in this part.

## What We're Building
Brief overview, diagram if helpful.

## Architecture
ARM concepts, design decisions, trade-offs.

## Implementation
Key code walkthrough. Links to commits.
Code snippets with explanation (not full listings).

## Testing
Test strategy, key assertions, coverage.

## Debugging Notes
Real bugs encountered, how they were found and fixed.
This is the "war stories" section — most engaging for readers.

## AI Collaboration Notes
How Claude Code helped (or didn't) in this part.
Specific prompts, agent usage, vibe coding moments.
What worked, what required manual intervention.

## Key Takeaways
3-5 bullet points summarizing lessons learned.
```

## AI Collaboration Thread

Each Part's "AI Collaboration Notes" section covers:

- **Planning**: How the design was brainstormed with AI (planner agent, brainstorming skill)
- **Implementation**: Claude Code's role — code generation, debugging, review
- **Testing**: TDD loop with AI — writing tests first, iterating until green
- **What AI couldn't do**: Cases where manual ARM knowledge was essential
- **Vibe coding moments**: When the AI "just got it" and produced working code from high-level intent
- **Metrics**: Approximate time spent, commits generated, human vs AI contribution ratio

## Twitter/X Strategy

Each Part maps to 1-3 tweet threads:

```
🧵 Part 4: Booting Linux under a Rust hypervisor

We went from bare-metal UART to a full Linux 6.12 BusyBox shell.

Key insight: FAR_EL2 gives you the guest VA, not the IPA.
Use HPFAR_EL2 instead. This one bug cost us a day.

Full writeup: [link to GitHub Pages chapter]

#RustLang #ARM64 #Hypervisor #VibeCoding
```

## Commit Reference Convention

Articles reference commits using short SHA links:

```markdown
In [commit `6a6cd60`](../../commit/6a6cd60), we fixed the critical
HPFAR_EL2 bug that broke all MMIO after guest MMU was enabled.
```

## Build and Deploy

```bash
# Install mdBook
cargo install mdbook

# Local preview
cd docs/book && mdbook serve

# Build for GitHub Pages
cd docs/book && mdbook build
# Output in docs/book/book/ → configure GitHub Pages to serve from this path
```

GitHub Actions can automate deployment on push to `main`.

## Writing Order

Priority order (start with highest-impact articles):

1. **Part 0** (Prologue) — sets the hook, author writes first draft
2. **Part 1** (First Boot) — immediate payoff, "Hello from EL2!"
3. **Part 4** (Boot Linux) — the "wow" moment, most shareable
4. **Part 10** (pKVM) — cutting-edge, attracts security/virtualization audience
5. **Parts 2-3** — fill in the foundation
6. **Parts 5-6** — SMP and multi-VM deep dives
7. **Parts 7-9** — FF-A and Secure World (most specialized)

## Success Criteria

- [ ] mdBook builds and renders locally
- [ ] EN + ZH SUMMARY.md with full article tree
- [ ] Part 0 published (author first draft + structured)
- [ ] Part 1 published (first technical article)
- [ ] GitHub Pages deployed
- [ ] First Twitter thread posted with link
- [ ] First 知乎/微信公众号 article posted
