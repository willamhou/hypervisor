# 知乎发布跟踪

ARM64 Hypervisor 开发系列的知乎发布状态与排程。每发一篇,把它从「待发」移到「已发」。

## 状态总览

| 状态 | 篇数 |
|---|---|
| ✅ 已发 | 12 |
| 📌 下一篇 | part10 |
| ⏳ 待发 | 11 |

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
| Part 10 — 一块 4KB 内存从 pKVM 到 SP 再回来(FF-A 内存共享生命周期) | `part10-ffa-mem-share.md` |

## 待发 ⏳(推荐顺序)

按"先填欠债 → A 档 deep dive → B 档短篇换节奏 → meta breath → 收官"的节奏:

| 顺序 | 文章 | 文件 | 档 | 选位理由 |
|---|---|---|---|---|
| 1 | Part 12 — GICv3 虚拟化从零(LR/HW=1/EOImode) | `part12-gicv3-virt.md` | A ~270 | 紧接 part10 末尾"下一篇 GICv3"预告;硬件主线 |
| 2 | Part 11 — Stage-2 演进与堆 gap | `part11-stage2-heap-gap.md` | A ~200 | 接 part12 的 GIC 进入 MMU 主题,反直觉钩子"堆在 guest PA 但 Stage-2 不映射" |
| 3 | Part 13 — HPFAR_EL2 vs FAR_EL2 | `part13-hpfar-el2.md` | B ~150 | 短篇调试现场,本身是 Stage-2 fault 话题,跟 part11 紧紧扣上 |
| 4 | Part 14 — TF-A 启动链 & SPKG 打包陷阱 | `part14-tfa-boot-chain.md` | A ~230 | 工程 war story,SPKG header 现场记忆点强 |
| 5 | Part 15 — FF-A v1.1 协议机制(描述符 + RXTX + 分片) | `part15-ffa-protocol-mechanics.md` | A ~260 | 接 part10 lifecycle 把协议层补完 |
| 6 | Part 16 — virtio-blk + virtio-net 从零 | `part16-virtio-from-scratch.md` | A ~260 | virtio 是独立主题,换条主线 |
| 7 | Part 17 — Secondary CPU warm-boot 六步 | `part17-secondary-warmboot.md` | B ~200 | 接 part4 "发现握手"的"完整装配"补完 |
| 8 | Part 18 — HCR_EL2.TSC 非对称语义 | `part18-hcr-tsc.md` | B ~160 | 短篇,trap 设计哲学的小推论 |
| 9 | Part 19 — ICC_SGI1R_EL1 位域那笔糊涂账 | `part19-icc-sgi1r-bitfield.md` | B ~130 | 短篇收束,bit 位踩坑 |
| 10 | Part 0b — AI 工作流篇 | `part0b-ai-workflow.md` | meta | meta 反思换调,与 part0a 同系列收尾;放在 deep dive 之后 |
| 11 | 实战记 — 真 ARM 跑通完整 NS→Secure 链 | `e2e-on-arm-fieldnotes.md` | 实战 | 最新一手战报,整个系列收官 |

## 发完之后

12 篇队列 + 今天的 part10,共 12 天日更素材(约两周)。继续日更需写新文,候选题材(按"无需新代码 → 需先做工作"分层):

**Tier 1 — 项目里已有,直接能写:**
- (已写完)multi_pcpu、FF-A 共享、Stage-2、GICv3、HPFAR、TF-A 链路、FF-A 协议、virtio、warm-boot、HCR_TSC、ICC_SGI1R

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
