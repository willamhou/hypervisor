# 知乎发布跟踪

ARM64 Hypervisor 开发系列的知乎发布状态与排程。每发一篇,把它从「待发」移到「已发」。

## 状态总览

| 状态 | 篇数 |
|---|---|
| ✅ 已发 | 9 |
| 📌 今天 | part8 |
| ⏳ 待发 | 4 |

## 已发 ✅

| 文章 | 文件 |
|---|---|
| Part 0a — 为什么写一个 Hypervisor | `part0a-why.md` |
| Part 1 — 从零到 "Hello from EL2!" | `part1-first-boot.md` |
| Part 2 — 陷入-模拟-恢复 | `part2-trap-emulate-resume.md` |
| Part 3 — 四个 CPU、一块磁盘:让 Linux 启动 | `part3-linux-boot.md` |
| Part 5 — Rust enum 状态机的真相 | `part5-enum-state-machine.md` |
| Part 6 — TrustZone 的 NS 位 | `part6-trustzone-ns-bit.md` |
| Part 7 — 裸机 Rust 三个硬件坑 | `part7-bare-metal-rust-pitfalls.md` |
| 综述 — 一颗芯片上跑两个 Hypervisor(替换 Hafnium) | `summary-two-hypervisors.md` |

## 今天 📌

| 文章 | 文件 |
|---|---|
| Part 8 — 两台 VM 互 ping(200 行 vSwitch) | `part8-multi-vm-vswitch.md` |

## 待发 ⏳(推荐顺序)

| 顺序 | 文章 | 文件 | 选位理由 |
|---|---|---|---|
| 1 | Part 4 — 四大坑·收尾(SPMD per-CPU) | `part4-war-stories.md` | 已重写,不再与 part6/part7 重复;把"四大坑"压轴收掉 |
| 2 | pKVM 完全解析 | `pkvm-explainer.md` | 科普换节奏,给"两个 hypervisor"主题补背景 |
| 3 | Part 0b — AI 工作流篇 | `part0b-ai-workflow.md` | meta 反思,与 part0a 同系列收尾 |
| 4 | 实战记 — 真 ARM 跑通完整 NS→Secure 链 | `e2e-on-arm-fieldnotes.md` | 最新一手战报,收官 |

## 发完之后

现有长文清空。继续日更需写新文,候选题材:
- `multi_pcpu` 分支(part8 结尾预告过,1:1 vCPU↔pCPU,真 PSCI CPU_ON 唤醒物理核)
- OP-TEE 作为 SP 跑在我们 SPMC 上(兼容性证明,需先做)
- 性能 benchmark vs KVM(REQUIREMENTS 列的目标,需先做)
- 完整 distro(Ubuntu/Debian arm64)启动(比 BusyBox 更有说服力)
