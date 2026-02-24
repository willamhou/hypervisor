# Summary

[Introduction](./README.md)

---

# Part 0: Prologue — Why and How

- [Overview](./part0-prologue/README.md)
- [Personal Background](./part0-prologue/background.md)
- [Project Motivation](./part0-prologue/motivation.md)
- [AI Workflow — Vibe Coding with Claude Code](./part0-prologue/ai-workflow.md)
- [Toolchain and Environment](./part0-prologue/toolchain.md)

# Part 1: First Boot — From Zero to EL2

- [Overview](./part1-first-boot/README.md)
- [Architecture: ARM64 Exception Levels](./part1-first-boot/arch.md)
- [Implementation: boot.S and First Rust Code](./part1-first-boot/impl.md)
- [Testing](./part1-first-boot/test.md)
- [Debugging Notes](./part1-first-boot/debug.md)

# Part 2: vCPU and Guest Execution

- [Overview](./part2-vcpu/README.md)
- [Architecture: VM Entry/Exit and Stage-2](./part2-vcpu/arch.md)
- [Implementation: vCPU Framework and Memory](./part2-vcpu/impl.md)
- [Testing](./part2-vcpu/test.md)
- [Debugging Notes](./part2-vcpu/debug.md)

# Part 3: Exception Handling and GICv3

- [Overview](./part3-exceptions-gic/README.md)
- [Architecture: Exception Vectors, MMIO Traps, GICv3](./part3-exceptions-gic/arch.md)
- [Implementation: Interrupts, Device Emulation, GICv3](./part3-exceptions-gic/impl.md)
- [Testing](./part3-exceptions-gic/test.md)
- [Debugging Notes](./part3-exceptions-gic/debug.md)

# Part 4: Booting Linux

- [Overview](./part4-boot-linux/README.md)
- [Architecture: Dynamic Page Tables, HPFAR, Timer](./part4-boot-linux/arch.md)
- [Implementation: GICR/GICD Emulation, Virtio-blk](./part4-boot-linux/impl.md)
- [Testing](./part4-boot-linux/test.md)
- [Debugging Notes](./part4-boot-linux/debug.md)

# Part 5: SMP — Multi-Core Virtualization

- [Overview](./part5-smp/README.md)
- [Single-pCPU: Round-Robin Scheduling](./part5-smp/single-pcpu.md)
- [Multi-pCPU: 1:1 Affinity](./part5-smp/multi-pcpu.md)
- [Testing](./part5-smp/test.md)
- [Debugging Notes](./part5-smp/debug.md)

# Part 6: Multi-VM and Networking

- [Overview](./part6-multi-vm/README.md)
- [Multi-VM: VMID, Two-Level Scheduling](./part6-multi-vm/multi-vm.md)
- [Virtio-net and VSwitch](./part6-multi-vm/virtio-net.md)
- [Testing](./part6-multi-vm/test.md)
- [Debugging Notes](./part6-multi-vm/debug.md)

# Part 7: FF-A — Firmware Framework for Arm

- [Overview](./part7-ffa/README.md)
- [Architecture: FF-A v1.1, SMC Trap, Page Ownership](./part7-ffa/arch.md)
- [Implementation: Proxy, Stub SPMC, Memory Sharing](./part7-ffa/impl.md)
- [Testing](./part7-ffa/test.md)
- [Debugging Notes](./part7-ffa/debug.md)

# Part 8: TF-A Boot Chain — Entering Secure World

- [Overview](./part8-tfa-boot-chain/README.md)
- [Architecture: BL1→BL2→BL31→BL32→BL33](./part8-tfa-boot-chain/arch.md)
- [Implementation: Docker Builds, BL33, BL32 SPMC](./part8-tfa-boot-chain/impl.md)
- [Testing](./part8-tfa-boot-chain/test.md)
- [Debugging Notes](./part8-tfa-boot-chain/debug.md)

# Part 9: S-EL2 SPMC — Replacing Hafnium

- [Overview](./part9-spmc/README.md)
- [Event Loop and SP Boot](./part9-spmc/event-loop.md)
- [DIRECT_REQ End-to-End](./part9-spmc/direct-req.md)
- [Interrupt Preemption and vIRQ/vFIQ](./part9-spmc/interrupts.md)
- [Testing](./part9-spmc/test.md)
- [Debugging Notes](./part9-spmc/debug.md)

# Part 10: pKVM Integration — The Final Architecture

- [Overview](./part10-pkvm/README.md)
- [Architecture: pKVM + SPMC Coexistence](./part10-pkvm/arch.md)
- [Implementation: AOSP Kernel, S-EL2 MMU, SMP](./part10-pkvm/impl.md)
- [Testing](./part10-pkvm/test.md)
- [Debugging Notes](./part10-pkvm/debug.md)
