# 从零开始写一个 Rust Hypervisor

> 用 AI 结对编程，从零构建一个 ARM64 Type-1 裸机 hypervisor。

这是一个重建生产级 hypervisor 的故事——一段曾经需要 10 个月的旅程，用 Claude Code 和 vibe coding 在大约 1 个月内重走了一遍。

从第一行 `boot.S` 到 pKVM 集成 FF-A v1.1，每一个 commit 都有记录。每一章都交织着技术实现和 AI 协作过程。

## 你会学到

- ARM64 虚拟化（EL2、Stage-2、GICv3、PSCI）
- Rust no_std 裸机编程
- Linux 内核在 hypervisor 下的启动过程
- ARM FF-A 固件框架与安全世界
- AI 辅助系统编程的工作流

## 代码仓库

所有代码位于 [github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor) — 191 个 commit，33 个测试套件，约 282 个断言。

## 如何阅读

**自顶向下**：从 Part 0 开始了解背景，然后按顺序阅读或跳到任意 Part。

**每个 Part**：概览 → 架构 → 实现 → 测试 → 踩坑记录 → AI 协作笔记。
