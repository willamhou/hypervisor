# 目录

[简介](./README.md)

---

# Part 0: 序章 — 为什么以及怎么做

- [概览](./part0-prologue/README.md)
- [个人背景](./part0-prologue/background.md)
- [项目初衷](./part0-prologue/motivation.md)
- [AI 工作流 — 用 Claude Code 做 Vibe Coding](./part0-prologue/ai-workflow.md)
- [工具链和开发环境](./part0-prologue/toolchain.md)

# Part 1: 第一次启动 — 从零到 EL2

- [概览](./part1-first-boot/README.md)
- [架构：ARM64 异常级别](./part1-first-boot/arch.md)
- [实现：boot.S 和第一行 Rust 代码](./part1-first-boot/impl.md)
- [测试](./part1-first-boot/test.md)
- [踩坑记录](./part1-first-boot/debug.md)

# Part 2: vCPU 与 Guest 执行

- [概览](./part2-vcpu/README.md)
- [架构：VM Entry/Exit 与 Stage-2 翻译](./part2-vcpu/arch.md)
- [实现：vCPU 框架与内存管理](./part2-vcpu/impl.md)
- [测试](./part2-vcpu/test.md)
- [踩坑记录](./part2-vcpu/debug.md)

# Part 3: 异常处理与 GICv3

- [概览](./part3-exceptions-gic/README.md)
- [架构：异常向量、MMIO 陷入、GICv3](./part3-exceptions-gic/arch.md)
- [实现：中断、设备仿真、GICv3](./part3-exceptions-gic/impl.md)
- [测试](./part3-exceptions-gic/test.md)
- [踩坑记录](./part3-exceptions-gic/debug.md)

# Part 4: 启动 Linux

- [概览](./part4-boot-linux/README.md)
- [架构：动态页表、HPFAR、Timer](./part4-boot-linux/arch.md)
- [实现：GICR/GICD 仿真、Virtio-blk](./part4-boot-linux/impl.md)
- [测试](./part4-boot-linux/test.md)
- [踩坑记录](./part4-boot-linux/debug.md)

# Part 5: SMP — 多核虚拟化

- [概览](./part5-smp/README.md)
- [单物理核：Round-Robin 调度](./part5-smp/single-pcpu.md)
- [多物理核：1:1 亲和](./part5-smp/multi-pcpu.md)
- [测试](./part5-smp/test.md)
- [踩坑记录](./part5-smp/debug.md)

# Part 6: 多虚拟机与网络

- [概览](./part6-multi-vm/README.md)
- [多 VM：VMID 与两级调度](./part6-multi-vm/multi-vm.md)
- [Virtio-net 与虚拟交换机](./part6-multi-vm/virtio-net.md)
- [测试](./part6-multi-vm/test.md)
- [踩坑记录](./part6-multi-vm/debug.md)

# Part 7: FF-A — ARM 固件框架

- [概览](./part7-ffa/README.md)
- [架构：FF-A v1.1、SMC 陷入、页面所有权](./part7-ffa/arch.md)
- [实现：代理、Stub SPMC、内存共享](./part7-ffa/impl.md)
- [测试](./part7-ffa/test.md)
- [踩坑记录](./part7-ffa/debug.md)

# Part 8: TF-A 引导链 — 进入安全世界

- [概览](./part8-tfa-boot-chain/README.md)
- [架构：BL1→BL2→BL31→BL32→BL33](./part8-tfa-boot-chain/arch.md)
- [实现：Docker 构建、BL33、BL32 SPMC](./part8-tfa-boot-chain/impl.md)
- [测试](./part8-tfa-boot-chain/test.md)
- [踩坑记录](./part8-tfa-boot-chain/debug.md)

# Part 9: S-EL2 SPMC — 替代 Hafnium

- [概览](./part9-spmc/README.md)
- [事件循环与 SP 启动](./part9-spmc/event-loop.md)
- [DIRECT_REQ 端到端](./part9-spmc/direct-req.md)
- [中断抢占与 vIRQ/vFIQ 注入](./part9-spmc/interrupts.md)
- [测试](./part9-spmc/test.md)
- [踩坑记录](./part9-spmc/debug.md)

# Part 10: pKVM 集成 — 最终架构

- [概览](./part10-pkvm/README.md)
- [架构：pKVM + SPMC 共存](./part10-pkvm/arch.md)
- [实现：AOSP 内核、S-EL2 MMU、SMP](./part10-pkvm/impl.md)
- [测试](./part10-pkvm/test.md)
- [踩坑记录](./part10-pkvm/debug.md)
