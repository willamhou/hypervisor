# Blog Post Outline: ARM64 Hypervisor in Rust

## Title Options

1. **Writing an ARM64 Type-1 Hypervisor in Rust from Scratch (and Making It Coexist with Android pKVM)** — best for HN
2. **Replacing Hafnium: Building a Bare-Metal S-EL2 SPMC in Rust in 10 Weeks**
3. **30K Lines of no_std Rust: A Bare-Metal Hypervisor That Boots Linux and Manages Secure Partitions**
4. **Two Hypervisors, One SoC: How I Built an ARM64 SPMC in Rust That Runs Alongside pKVM** — best for r/rust

## Hook

- ARM's Confidential Compute splits the CPU into Secure and Normal worlds, each with its own hypervisor at EL2
- Hafnium (Google's reference SPMC) is 200K+ lines of C. This project replaces it with ~30K lines of Rust
- Boots Linux to a shell, manages 3 Secure Partitions, passes 35/35 pKVM E2E tests — all on QEMU
- 10 weeks, solo developer, from first ERET to full FF-A v1.1 memory sharing

## Sections

### 1. Why Build a Hypervisor? (Motivation)
- ARM EL0-EL3 + Secure/Normal world split
- The S-EL2 gap: pKVM owns NS-EL2, Hafnium owns S-EL2 — but Hafnium is hard to extend
- Rust for bare-metal: no_std, memory safety without a runtime
- Goal: understand every layer by building it yourself
- **[DIAGRAM: ARM exception levels with Secure/Normal split]**

### 2. Architecture Overview
- Boot chain: TF-A BL1→BL2→BL31(SPMD)→BL32(our SPMC)→BL33(pKVM/Linux)
- Dual hypervisor: our SPMC at S-EL2 + pKVM at NS-EL2, 4 CPUs
- Guest: Linux 6.12 with 4 vCPUs, virtio-blk, virtio-net, L2 vSwitch
- 3 SPs at S-EL1, FF-A v1.1 full implementation
- Key numbers: 30K LOC, 457 assertions, 35/35 E2E
- **[DIAGRAM: System architecture showing Secure/Normal columns with FF-A arrows]**

### 3. Technical Deep Dives
- **no_std Rust at EL2**: custom target, bump allocator, enum dispatch (no trait objects)
- **Stage-2 gymnastics**: PTE SW bits for ownership, cross-VM map_page
- **GIC emulation**: GICD write-through, GICR trap-and-emulate, SGI decode
- **FF-A memory sharing**: full lifecycle, handle_sp_exit() loop
- **SP-to-SP**: CallStack cycle detection, chain preemption state machine
- **[DIAGRAM: SP state machine]**

### 4. War Stories (3 debugging tales)

**CPTR_EL3.TFP — The Silent SIMD Trap**
- Debug Rust emits NEON for alignment checks → TF-A traps FP/SIMD from S-EL2 → silent hang
- Fix: CTX_INCLUDE_FPREGS=1
- Lesson: codegen matters below the OS

**SPMD Is Per-CPU**
- Secondary CPUs hang after PSCI CPU_ON → SPMD expects per-CPU FFA_MSG_WAIT
- Discovered by reading TF-A source, not any spec
- Fix: FFA_SECONDARY_EP_REGISTER + per-CPU stacks + event loops

**S-EL2 Stage-1 and the NS Bit**
- PARTITION_INFO_GET works from BL33 but pKVM reads zeros → writes go to Secure alias
- Fix: S-EL2 Stage-1 MMU with NS=1 for NWd DRAM
- Lesson: ARM's two physical address spaces are real, even in emulation

### 5. Testing Strategy
- Unit tests on bare-metal QEMU (no OS, no harness)
- BL33 integration: real TF-A boot chain
- pKVM E2E: ffa_test.ko through real SPMD
- No mocking — everything on actual (emulated) hardware

### 6. What I Learned
- Start with simplest guest (HVC #0 → exit), add complexity per trap
- Read the ARM ARM, not just blog posts
- TF-A source is the real spec for S-EL2 boot
- Debug vs release codegen causes architectural traps

### 7. What's Next
- Phase 5: RME & CCA (Realm Manager)
- Hardware validation (Graviton/Ampere)
- AVF pVM boot

### 8. Call to Action
- GitHub link, `make run` instructions
- Contributions welcome

## Production Notes

**Diagrams (3-4):**
1. ARM exception levels + Secure/Normal split
2. System architecture with FF-A message flow
3. SP state machine
4. Optional: FF-A memory sharing lifecycle with PTE states

**Screenshots:**
- `make run-pkvm-ffa-test` showing 35/35 PASS
- `make run` showing test suites

**Length**: 2500-3500 words
**Tone**: Technical, conversational, first person, honest about limitations
**Publish to**: Hacker News, Reddit r/rust, lobste.rs, dev.to
