# 知乎发布跟踪

ARM64 Hypervisor 开发系列的知乎发布状态与排程。每发一篇,把它从「待发」移到「已发」。

## 状态总览

| 状态 | 篇数 |
|---|---|
| ✅ 已发 | 12 |
| 📌 下一篇 | part0b |
| ⏳ 待发 | 1 |

## 已发 ✅

| 文章 | 文件 |
|---|---|
| Part 0a — 为什么写一个 Hypervisor | `part0a-why.md` |
| Part 1 — 从零到 "Hello from EL2!" | `part1-first-boot.md` |
| Part 2 — 陷入-模拟-恢复 | `part2-trap-emulate-resume.md` |
| Part 3 — 四个 CPU、一块磁盘:让 Linux 启动 | `part3-linux-boot.md` |
| Part 4 — 四大坑·收尾(SPMD per-CPU) | `part4-war-stories.md` |
| Part 5 — Rust enum 状态机的真相 | `part5-enum-state-machine.md` |
| Part 6 — TrustZone 的 NS 位 | `part6-trustzone-ns-bit.md` |
| Part 7 — 裸机 Rust 三个硬件坑 | `part7-bare-metal-rust-pitfalls.md` |
| Part 8 — 两台 VM 互 ping(200 行 vSwitch) | `part8-multi-vm-vswitch.md` |
| Part 9 — 4 个 vCPU 跑在 4 颗 pCPU 上(multi-pCPU 深度) | `part9-multi-pcpu.md` |
| 综述 — 一颗芯片上跑两个 Hypervisor(替换 Hafnium) | `summary-two-hypervisors.md` |
| pKVM 完全解析 | `pkvm-explainer.md` |

## 下一篇 📌

| 文章 | 文件 |
|---|---|
| Part 0b — AI 工作流篇 | `part0b-ai-workflow.md` |

## 待发 ⏳

| 顺序 | 文章 | 文件 | 选位理由 |
|---|---|---|---|
| 1 | 实战记 — 真 ARM 跑通完整 NS→Secure 链 | `e2e-on-arm-fieldnotes.md` | 最新一手战报,收官 |

## 发完之后

现有长文清空。继续日更需写新文,候选题材(按"无需新代码 → 需先做工作"分层):

**Tier 1 — 项目里已有,直接能写:**
- 跨世界内存共享 deep dive(part9 末尾预告过:MEM_SHARE/LEND/DONATE/RETRIEVE/RELINQUISH/RECLAIM 全套生命周期)
- GICv3 虚拟化从零(LR 注入 / ICC_SGI1R / EOImode 分离)
- FF-A v1.1 协议机制(composite descriptor / RXTX mailbox / fragmentation)
- TF-A 启动链 & SPKG 打包陷阱
- Secondary CPU warm-boot 完整版(part4 是发现,这篇是补完)
- Stage-2 页表与 IdentityMapper 演进(2MB → 4KB GICR 拆分)
- virtio-blk/net 从零

**Tier 2 — 方法论 / meta:**
- 裸机调试方法论(no JTAG 时怎么活)
- 30K Rust vs 200K C(Hafnium):为什么差 7 倍
- Bare-metal TDD 的边界
- 一个 AI 调出真 bug 的全过程(对照 part0b 的 meta)

**Tier 3 — 需新工作:**
- OP-TEE 作为 SP 跑在我们 SPMC 上
- 性能 benchmark vs KVM
- 完整 distro(Ubuntu/Debian arm64)启动
- RME/CCA Realm Manager
