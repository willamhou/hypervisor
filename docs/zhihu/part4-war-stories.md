# 裸机四大坑 · 收尾篇 —— ARM 规范没写、TF-A 源码里挖出来的 SPMD per-CPU 握手

## 写在前面

这是"裸机四大坑"系列的最后一篇。前三个坑后来都各自展开过独立的深度版,这篇专门补完第四个,顺便收个尾。

为什么要拆?因为越往后查,越发现每个坑单独都够撑一篇长文,合在一起反而看不清各自的根因层。所以现在的分布是:

| 坑 | 根因所在层 | 在哪篇 |
|---|---|---|
| 一、`debug_assert!` 里藏 NEON 指令(SIMD trap) | 编译器代码生成 | [Part 7: 裸机 Rust 的三个"Rust 没问题,硬件有话说"的坑](./part7-bare-metal-rust-pitfalls.md) |
| 二、写成功了但读回全是零(NS bit) | 物理地址空间属性 | [Part 6: TrustZone 的 NS 位不只是权限,更是物理地址空间的选择](./part6-trustzone-ns-bit.md) |
| 三、70% 正常 30% Data Abort(跨核共享 buffer) | 跨 CPU 缓存一致性 + 同上 Part 7 的 SMC 屏障章节 | [Part 7](./part7-bare-metal-rust-pitfalls.md) |
| **四、secondary CPU 永远挂起(SPMD per-CPU 状态机)** | **固件状态机** | **本篇** |

如果你刚跟下来 Part 6 和 Part 7,这篇是最后一块拼图。如果你是直接进来的,后面的引用都点得开。

四个坑有一个共同点——**它们在你的层面看起来都是对的**。代码逻辑对、地址对、调用顺序对。但在更低的层面(编译器、MMU、cache、固件),有一些你不知道的约束正在被违反。前三个我已经把那"更低的层面"讲清楚了,第四个的"更低层"是 TF-A 的 SPMD 状态机——而它**根本没出现在任何 ARM 规范里**。

---

## 现象:CPU 0 起来了,1/2/3 永远挂起

进入 Phase 4.5 集成 pKVM 之后,启动序列是这样的:

```
EL3   TF-A BL31 + SPMD       ← ARM 安全监控器
S-EL2 我们的 SPMC            ← 管理 Secure Partition
S-EL1 SP1/SP2/SP3            ← 三个秘密分区
NS-EL2 pKVM                  ← Google 的 protected KVM
NS-EL1 Linux/Android         ← guest 内核
```

QEMU `secure=on,virtualization=on` 起来,日志看着不错:

- BL31 v2.12.0 启动 ✓
- 我们的 SPMC 在 S-EL2 起来 ✓
- SP1/SP2/SP3 各自 boot 到 Idle ✓
- pKVM 在 NS-EL2 起来,`Protected hVHE mode initialized successfully` ✓

然后 pKVM 启动 secondary CPU——`smp: Bringing up secondary CPUs ...`——**永远停在这里。** CPU 1 / 2 / 3 一个都不上来,pKVM 直接卡死等待 PSCI CPU_ON 的回复。

奇怪的是 EL3 / S-EL2 / SP 这条链上一切看起来正常。我们的 SPMC 没崩,SP 也没崩,日志没任何错误。CPU 0 上的所有功能都好用——FF-A discovery、PARTITION_INFO_GET、DIRECT_REQ 都过得去。问题只在 secondary CPU 起不来。

---

## 排查:FF-A 规范只字未提 secondary CPU

第一反应是 PSCI CPU_ON 的转发出了问题——pKVM 在 NS-EL2 发 `smc #0` 调 PSCI_CPU_ON,TF-A BL31 的 PSCI 服务收到后应该执行物理 CPU 上电。这条路径不经过我们的 SPMC,理论上跟 SPMC 无关。

但日志显示 PSCI 调用确实**进了 EL3** 又**没出来**。那就是 BL31 内部出了问题。

翻 FF-A v1.1 规范(DEN0077A)——找"secondary CPU"。**只字未提。** 找"CPU_ON"——只在 PSCI 章节出现,跟 SPMC 完全切割。找"warm boot"——零结果。规范完整描述了:

- 怎么发 FFA_VERSION 握手
- 怎么 RXTX_MAP 注册邮箱
- 怎么 PARTITION_INFO_GET 列 SP
- DIRECT_REQ / MEM_SHARE 的协议格式

但 secondary CPU 怎么进入 Secure World、SPMC 这边要不要做什么准备——**完全没写**。规范在"Lifecycle"章节只讲了 primary CPU 的初始化(`FFA_MSG_WAIT` 握手通知 SPMD 自己就绪),对 secondary 一句话也没有。

合理的猜测:secondary 是不是也要做同样的 `FFA_MSG_WAIT`?但谁调用?在哪里调用?入口点怎么注册?

规范这里就断了。只能去读源码。

---

## 翻 TF-A 源码:`spmd_cpu_on_finish_handler`

TF-A 的代码在 `services/std_svc/spm/spmd/` 目录下。grep `cpu_on`,找到这个函数:

```c
// services/std_svc/spm/spmd/spmd_main.c
static void *spmd_cpu_on_finish_handler(const void *arg)
{
    /* On every CPU, after PSCI CPU_ON completes, SPMD needs to
     * activate the SPMC on this CPU. The SPMC must respond with
     * FFA_MSG_WAIT to indicate it's ready before SPMD returns
     * control to the Non-Secure caller. */
    ...
    spmd_spm_core_sync_entry(ctx);
    ...
}
```

读注释 + 配合状态机看明白了:**SPMD 为每个物理 CPU 维护完全独立的状态机**。每次 PSCI CPU_ON 在某个 secondary 上完成,SPMD 在那个 CPU 上做一次 "S-EL2 进入"——它会 ERET 到我们预先**注册过的 secondary entry point**,然后**阻塞**等我们调 `FFA_MSG_WAIT` 回去。只有等到这次握手,SPMD 才会继续 secondary 的 PSCI CPU_ON,通知 pKVM"这个 CPU 起来了"。

也就是说,每个 CPU 的启动流程都是:

```
CPU N PSCI CPU_ON 发起 (pKVM 在 EL2)
  → SMC 进 EL3,TF-A BL31 接到
  → BL31 PSCI 服务上电 CPU N
  → BL31 ERET 到 EL3 入口,把 CPU N 转交给 SPMD
  → SPMD 在 CPU N 上 ERET 到 SPMC 的 secondary entry (我们注册的)
  → SPMC 在 CPU N 上初始化必要的 EL2 状态
  → SPMC 调 FFA_MSG_WAIT  ← 这一步告诉 SPMD:"CPU N 的 Secure 侧就绪"
  → SPMD 收到 WAIT,继续 PSCI CPU_ON 的后半段
  → ERET 回 EL2,pKVM 看到 CPU N 起来了
```

而我最初的代码:**只有 primary CPU 做了 `FFA_MSG_WAIT`,根本没注册 secondary entry**。secondary CPU 进入 S-EL2 之后,SPMD 不知道往哪 ERET,直接挂在那里 → pKVM 看到的就是 secondary 永远不上来。

```
CPU 0: SPMC init → boot SPs → FFA_MSG_WAIT ✓ → SPMD 完成 → pKVM 启动
CPU 1: ???              → 没有 secondary entry → SPMD 阻塞 → pKVM CPU_ON 挂起
CPU 2: ???              → 同上
CPU 3: ???              → 同上
```

---

## 修法:三步

### 第一步:注册 secondary entry point

primary CPU 初始化完成后,调用 `FFA_SECONDARY_EP_REGISTER`(0x84000087)把 secondary entry 的物理地址告诉 SPMD:

```rust
// src/main.rs — SPMC init,primary CPU 路径
extern "C" { fn secondary_entry_sel2(); }
let ep = secondary_entry_sel2 as *const () as usize as u64;
let result = forward_smc8(FFA_SECONDARY_EP_REGISTER, ep, 0, 0, 0, 0, 0, 0);
```

这个调用必须在 primary 完成所有初始化、做第一次 `FFA_MSG_WAIT` 之前发出去,否则后续 secondary 上电时 SPMD 根本不知道往哪跳。

### 第二步:给每个 secondary 准备独立的栈

S-EL2 跑在裸机,栈得自己分配。primary 用的是 `boot_sel2.S` 里的 `_stack`。secondary 必须用独立栈,否则它们 boot 起来会互相踩 primary 的栈帧。

```asm
// arch/aarch64/boot_sel2.S
.section .bss.sel2_pcpu_stacks
.align 16
sel2_pcpu_stacks:
    .space 3 * 32 * 1024   // CPU 1, 2, 3 各 32KB
```

`secondary_entry_sel2` 入口先做的事就是按 `MPIDR_EL1` 的 CPU index 算出本核的栈顶,装进 SP:

```asm
secondary_entry_sel2:
    mrs   x0, mpidr_el1
    and   x0, x0, #0xff           // CPU index (Aff0)
    sub   x0, x0, #1              // CPU N → 栈 index N-1
    adrp  x1, sel2_pcpu_stacks
    add   x1, x1, :lo12:sel2_pcpu_stacks
    mov   x2, #(32 * 1024)
    madd  x0, x0, x2, x1          // 栈底 = base + N*32KB
    add   sp, x0, x2              // SP = 栈底 + 32KB(向下增长)
    bl    rust_main_sel2_secondary
```

(这里硬假设了 Aff0 ∈ {1,2,3},生产代码要做边界检查;QEMU virt 4 核场景下够用。)

### 第三步:secondary 上的初始化顺序(顺序不能乱)

`rust_main_sel2_secondary()` 是 secondary CPU 进入 Rust 后做的事:

```text
1. 安装 VBAR_EL2(exception::init)               ← 异常向量到位,否则后面任何 trap 都不可恢复
2. 清 CPTR_EL2 / MDCR_EL2 的 trap 位             ← 把 FP/SVE/SME/debug 的 trap 关掉,防止后面 isb 自陷
3. 复用 primary 的 S-EL2 Stage-1 页表            ← install_sel2_stage1_secondary()
   (页表是 primary 装好的,secondary 只需要把 TTBR0_EL2/TCR_EL2/MAIR_EL2 + SCTLR_EL2.M 打开)
4. 开 HCR_EL2.VM                                ← Secure Stage-2 开始生效
5. 开本 CPU 的 GICR PPI 26/29                    ← poll 定时器 + 安全物理 timer 中断使能
6. FFA_MSG_WAIT  ← 关键握手!告诉 SPMD 本 CPU 的 Secure 侧已就绪
7. run_event_loop()                              ← 持续处理后续到达本 CPU 的 SMC 请求
```

**顺序不能乱**——必须先放开 trap(CPTR/MDCR)**再**开 MMU。某些 TF-A 配置下,如果 CPTR 还没放开就走到 `SCTLR_EL2.M=1` 后面的 `isb`,会在 `isb` 处直接 trap 进 EL3,而我们的 EL3 默认 handler 不知道怎么处理这个 trap → 永久挂死。我第一次写错的就是这个顺序,排查又花了几小时。

`FFA_MSG_WAIT` 只是"入场券"。真正让 secondary 持续可用的是后面那个 `run_event_loop()`——每个 CPU 都得留在自己的循环里处理后续 SMC,否则下一次 pKVM 在这个 CPU 上发 FF-A 调用,SPMC 不会响应,直接超时。

---

## 教训:规范 vs 源码

回到那个让我卡了几小时的问题:**为什么 ARM 规范不写这件事?**

我现在的理解是:ARM 规范文档(DEN0077A FF-A、ARM ARM 等)定位是**"接口定义"** —— 它告诉你有哪些 SMC 调用、参数格式、返回值、状态转换。但**怎么把这些调用串成一个能跑的系统**——什么时候调、在哪个 CPU 上调、调用顺序——属于"实现指南"层,规范刻意不写。理由也合理:实现可以有不同选择,规范只锁接口语义,留实现自由。

代价是:**事实上的参考实现就是规范的延伸**。TF-A 的 `services/std_svc/spm/spmd/` 整个目录是 SPMD 的真实状态机,信息量比规范的"Lifecycle"章节大几个数量级。规范告诉你"SPMC 在每个 CPU 上要 ready",源码告诉你"通过 `FFA_MSG_WAIT` 握手通知 SPMD,SPMD 会阻塞等这一刻"。前者是 1 行抽象,后者是 200 行状态转换。

实战 checklist:

- **跟 ARM 固件打交道,把 TF-A 源码当文档读**。`services/std_svc/spm/` 是 SPM 的源头,`services/std_svc/spm/spmd/spmd_main.c` 是 SPMD 主入口
- **遇到"规范没说但显然得做"的事**,grep TF-A 源码里相关的 handler 名字(`*_on_finish_handler`、`*_off_handler`、`*_suspend_handler`)
- **secondary CPU 永远是最容易踩坑的地方**——大多数规范默认讲 primary 流程,secondary 的额外约束散落在源码里
- 如果你写 SPMC、TEE OS 或任何 S-EL2 软件,**先把 `spmd_cpu_on_finish_handler` 整段读一遍**

---

## 四大坑的共同模式

回头看四个坑:

| 坑 | 你以为对的层面 | 真正在被违反的层面 |
|---|---|---|
| 一、SIMD trap | "我没写浮点,二进制里就没有浮点指令" | LLVM 把 popcount 降成 NEON,而 `CPTR_EL3.TFP=1` 在 trap 这些 |
| 二、NS bit | "0x42a16000 是个物理地址,读写都到那" | Secure / Non-secure 是两个独立物理地址空间,数值相同地址不同 |
| 三、跨核 buffer | "写了,SMC 切了世界,然后读,正常" | SMC 不是屏障,跨核 cache 一致性靠 DSB + 一致性协议凑齐 |
| 四、SPMD 握手 | "PSCI CPU_ON 是 EL3 的事,我 SPMC 不管" | SPMD 为每个 CPU 维护独立状态,等 SPMC 的握手才放行 |

共同点很清楚:**抽象层下面没有 OS 给你兜底**。在 userspace 写代码,你可以默认"OS 已经把硬件处理好了";写 kernel 至少 OS 是你的;写 hypervisor、写 SPMC,你**就是**那个"应该处理好硬件"的层。再下面的层(硬件、固件、编译器)只按它们自己的契约工作,你违反契约,它们也不会报错——只会让你以为程序对了。

缩小"心智模型 vs 硬件实际行为"的差距,没有捷径:**反汇编、读 ARM ARM、读 TF-A / Hafnium / Linux KVM 源码**。这是裸机开发真正的核心技能。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第四篇(单独成篇的"四大坑·收尾")。之前的文章:*

- *Part 0a: [为什么写一个 Hypervisor](./part0a-why.md)*
- *Part 0b: [AI 辅助系统编程](./part0b-ai-workflow.md)*
- *Part 1: [从零到 "Hello from EL2!"](./part1-first-boot.md)*
- *Part 2: [陷入-模拟-恢复](./part2-trap-emulate-resume.md)*
- *Part 3: [让 Linux 启动](./part3-linux-boot.md)*
- *Part 5: [Rust enum 状态机的真相](./part5-enum-state-machine.md)*
- *Part 6: [TrustZone 的 NS 位](./part6-trustzone-ns-bit.md)* —— 坑二深度版
- *Part 7: [bare-metal Rust 三个坑](./part7-bare-metal-rust-pitfalls.md)* —— 坑一 + 坑三深度版
