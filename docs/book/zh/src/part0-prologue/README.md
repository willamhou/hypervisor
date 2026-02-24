# Part 0: 序章 — 为什么以及怎么做

> 从一个 10 个月的生产级 hypervisor，到 1 个月的 AI 辅助 Rust 重建。

2026 年 1 月，我开始从零用 Rust 写一个 ARM64 裸机 hypervisor。这不是学术练习——我以前在工作中做过同样的事。但这一次，我的结对编程搭档不是人类，而是 Claude Code。

30 天。193 个 commit。从第一行 `boot.S` 到 Linux 4 核启动、virtio 存储、VM 间网络、FF-A 固件框架、TF-A 安全世界引导链，以及在 S-EL2 层级的 pKVM 集成。

这个系列记录了整个过程，技术深潜与 AI 协作反思交织——什么有效，什么无效，什么出乎意料。

## 系列内容

| Part | 主题 | 关键里程碑 |
|------|------|-----------|
| 0 | 序章 | 背景、动机、工作流 |
| 1 | 第一次启动 | "Hello from EL2!" |
| 2 | vCPU | 通过 ERET 执行 Guest |
| 3 | 异常与 GICv3 | 中断虚拟化 |
| 4 | 启动 Linux | BusyBox shell 提示符 |
| 5 | SMP | 4 个 vCPU 跑在 4 个物理核上 |
| 6 | 多虚拟机 | 2 个 VM 加 virtio-net 网络 |
| 7 | FF-A | ARM 固件框架 v1.1 |
| 8 | TF-A 引导链 | BL1→BL2→BL31→BL32→BL33 |
| 9 | S-EL2 SPMC | 替代 Hafnium |
| 10 | pKVM | 最终架构 |

## 如何阅读

每个 Part 遵循相同的结构：

- **架构**：ARM 概念和设计决策
- **实现**：代码走读，附 commit 链接
- **测试**：测试策略和关键断言
- **踩坑记录**：真实的 bug，真实的修复——实战故事
- **AI 协作笔记**：Claude Code 帮了什么忙（或帮倒忙）

从这里开始，然后进入 [Part 1](../part1-first-boot/README.md)，或者跳到你感兴趣的任何 Part。
