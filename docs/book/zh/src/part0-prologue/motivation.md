# 项目初衷

## 一个挥之不去的想法

2025 年年中。我脑子里反复转着同一个念头：如果从零重建那个 hypervisor 呢？

[之前那个](background.md)也是 Rust 写的——3-4 人团队，10 个月，世界上第一个通过 CCRC 认证的 Rust SPMC。我们已经证明了 Rust 做得了生产级 hypervisor。但我们也切身体会过那种工程量。ARM 的架构面太大了。每一个边界情况——某个只在特定 CPU 处于特定电源状态时才有意义的 GICR 寄存器、某个只在 guest MMU 和 hypervisor MMU 同时开启时才触发的 Stage-2 fault——都必须正确处理。没有"差不多就行"这回事。

一个人能把这段旅程的核心复刻出来吗？大概能吧。但要好几个月。我还有创业公司要经营。

搁置了。

## Claude Code 改变了等式

2025 年底，我日常在用 Claude Code 做应用层的活——API、前端，不算什么硬核的东西。然后有天晚上心血来潮，让它写一个 ARM64 EL2 入口的 `boot.S`。栈设置、BSS 清零、跳转到 Rust。

架构层面写对了。`adr` 和 `ldr` 用对了，`.section .text.boot` 放对了，`wfe` halt 循环也对。我带过的工程师，有人第一次写这个也会搞错。（平台层面的细节——加载地址、UART 初始化——它会搞错。但 ARM 架构概念是扎实的。详见 [Part 1](../part1-first-boot/debug.md)。）

于是我继续加码。"写一个 2MB block 的 Stage-2 identity map。" 对了。"用 HCR_EL2.TWI 陷入 WFI，在异常向量里处理。" 也对了，包括 PC 前进。

它分得清 EL1 和 EL2。能推理 `VTTBR_EL2` 的位域。能解释 Stage-2 翻译为什么需要单独的寄存器来拿 IPA。不完美——有些盲区后来让我们付出了好几天的代价——但基础是在的。

想法又回来了。不是"AI 能不能写 hypervisor？"——这个问题就问错了。正确的问题是：**AI 能不能把一个 3-4 人团队 10 个月的活，压缩到一个人几周干完？**

## 实验

我定了一个简单的目标：从零重建核心的 hypervisor 旅程，用 Rust，Claude Code 当搭档。不是玩具——一个真正的 Type-1 hypervisor，能跑 Linux，搞得定 SMP，做 virtio I/O，实现 ARM FF-A 固件框架。

规则：
- **只用 Rust**（加上启动和异常向量必要的 ARM64 汇编）
- **不碰现有代码**——不 fork Hafnium，不从生产版本抄
- **AI 当结对搭档**——Claude Code 负责规划、实现、测试、调试
- **全程留痕**——每个 commit，每个设计决策，每个 bug

2026 年 1 月 26 日：第一个 commit。2 月 24 日：pKVM 带着我们的 SPMC 在 S-EL2 跑起来了，FF-A v1.1 完全可用。

30 天。193 个 commit。看看这事是怎么发生的。
