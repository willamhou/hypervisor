# Rust 裸机四大坑 — 在 ARM EL2 写 Hypervisor 踩过的雷

## 写在前面

前三篇讲了怎么从零开始写一个 ARM64 hypervisor：四个文件启动 EL2、陷入-模拟-恢复循环、让 Linux 跑起来。那些是"怎么做对"的故事。

这篇讲"怎么做错"的故事。

项目进入 Phase 4 之后，hypervisor 从 NS-EL2（Normal World）扩展到了 S-EL2（Secure World），变成了一个真正的 SPMC（Secure Partition Manager Core），跟 Google 的 pKVM 跑在同一颗芯片上。这个阶段的 bug 有一个共同特点：**它们都不报错**。没有 panic，没有 fault，没有任何输出。CPU 只是停在那里，或者你的数据默默地变成了零。

以下四个 bug，每个都花了至少半天到一天才定位。它们的根因分布在四个完全不同的层面：编译器代码生成、物理地址空间模型、跨 CPU 缓存一致性、固件状态机。

如果你在做 Rust no_std / 裸机 / ARM 底层开发，这些坑迟早会遇到。

---

## 坑一：编译器往你代码里塞了 SIMD 指令

**现象**：SPMC 在 release 模式下正常启动，换成 debug 模式就死在第一次 `read_volatile` 调用。没有任何输出，串口完全静默。

**排查过程**：

GDB attach 上去，发现 CPU 卡在 EL3 的异常处理函数里死循环。查 `ESR_EL3`，异常类型是 FP/SIMD trap（EC=0x07）。

但我的代码没有用浮点数。

反汇编 `read_volatile` 的调用点，找到了罪魁祸首：

```asm
cnt v0.8b, v0.8b    ; NEON SIMD population count
```

这是 Rust debug 模式下 `read_volatile` 内部的对齐检查代码。编译器为了计算 popcount，选择了一条 NEON 指令。在有操作系统的环境里这完全没问题——操作系统会在启动时使能 FP/SIMD。但在 bare-metal 环境下，我们跑在 TF-A 固件之上。TF-A 的默认配置是 `CPTR_EL3.TFP=1`，意思是：**从任何异常级别执行 FP/SIMD 指令都会陷入到 EL3**。

EL3 的默认异常处理并不知道怎么处理这个陷入，于是进入死循环。不报错，不 print，CPU 就静静地转圈。

**根因**：在操作系统之下写代码，**编译器的代码生成是硬件契约的一部分**。你不用浮点不代表你的二进制里没有浮点指令。Rust 的 debug 模式会插入大量安全检查（对齐、溢出），这些检查的实现可能用到 SIMD。

**修复**：在 TF-A 编译时加一个 flag：

```makefile
CTX_INCLUDE_FPREGS=1
```

这会让 TF-A 在上下文切换时保存/恢复 FP 寄存器，同时清除 `CPTR_EL3.TFP`，不再 trap FP/SIMD 指令。需要同时设置 `ENABLE_SVE_FOR_NS=0` 和 `ENABLE_SME_FOR_NS=0`，否则 TF-A 构建会报冲突。

**教训**：如果你的 bare-metal Rust 程序在 debug 模式下莫名其妙挂掉，但 release 模式正常——**先查 SIMD trap**。反汇编你的二进制，搜索所有 `v0`-`v31` 寄存器引用。这不是 Rust 独有的问题，Clang/GCC 的 debug 模式同样可能插入 NEON 指令，但 Rust 因为 safety check 更多，触发概率更高。

---

## 坑二：写成功了，但数据不在那里

**现象**：`PARTITION_INFO_GET` 这个 FF-A 调用，从 BL33 测试程序（跑在 NS-EL2）调用完全正常，SPMC 往调用者的 RX buffer 写 SP 描述符，调用者读回来，24 字节一个 partition，数据完全正确。

换成 pKVM 来调同一个函数。同样的代码路径，同样的描述符格式。pKVM 读回来——**全是零**。

**排查过程**：

GDB 确认写入成功了（没有 fault），地址也对（0x42a16000，就是 pKVM 注册的 RX buffer 地址）。数据写进去了，但读的时候不见了。

这不是缓存问题，不是对齐问题，不是时序问题。是**物理地址空间属性**的问题。

ARM 的内存事务带一个 `NS` 属性位。架构上，TrustZone 定义了**两个独立的物理地址空间**：Secure 和 Non-secure。同一个数值地址 `0x42a16000` 在两个空间下是两个不同的架构地址，事务的 `NS` 属性决定这次访问归哪个空间——具体怎么裁决由内存系统（TZASC、TZC 系列、或 SoC 自己的 NS 控制器）实现，可能落到同一颗 DRAM 的不同区段，也可能是真正分离的存储。架构层面要记住的是：**这是两个不同的物理地址空间**。

我们的 SPMC 跑在 S-EL2，当时 MMU 关着。MMU 关闭时，S-EL2 发出的所有内存访问默认都是 `NS=0`——走 Secure 物理地址空间。pKVM 注册的 RX buffer 在 Non-secure 物理地址空间，只接受 `NS=1` 的事务。

所以 SPMC 往 `0x42a16000` 写——事务带 `NS=0`，走 Secure 物理地址空间，成功落地。pKVM 从 `0x42a16000` 读——事务带 `NS=1`，走的是 Non-secure 物理地址空间，里面还是原来的值。两边都"访问成功"了，但从架构看根本走的是两个不同的地址空间。

```text
SPMC 写入:  0x42a16000  NS=0  → Secure 物理地址空间      ← 数据落在这里
pKVM 读取:  0x42a16000  NS=1  → Non-secure 物理地址空间  ← 这里是空的
```

**修复**：给 S-EL2 启用 Stage-1 MMU，建立一个恒等映射（identity map），把所有 Normal World DRAM 区域标记为 `NS=1`：

```rust
// src/sel2_mmu.rs
const NS_BIT: u64 = 1 << 5;

// L1[1]: 0x40000000-0x7FFFFFFF — NWd DRAM, NS=1
const NORMAL_NS_XN: u64 = ATTR_NORMAL_WB | NS_BIT | AP_RW | SH_ISH | AF | XN;
```

当 S-EL2 的 Stage-1 页表里 NS bit 为 1 时，硬件会把这个地址的访问路由到 Non-Secure 物理地址空间。写入才能真正到达 pKVM 的内存。

**教训**：Secure/Non-Secure 不只是权限模型，它在架构上对应**两个独立的物理地址空间**，事务的 `NS` 属性决定这次访问归哪边。跨世界通信的时候，正确的地址数值对上错误的 `NS`，写和读就会各走各的地址空间——两边都没报错，但谁也读不到对方的数据。

如果你在做跨世界（Secure ↔ Non-Secure）共享内存，**第一件事是确认 S-EL2 这边的 Stage-1 MMU 已经启用，并且 NWd DRAM 区域的 PTE 打了 `NS=1`**。

---

## 坑三：70% 的时候正常，30% 的时候 Data Abort

**现象**：pKVM 的 `MEM_SHARE`（FF-A 内存共享）大约 70% 的时间能正常工作。剩下 30%，SPMC 崩溃，Data Abort 异常，fault 地址是类似 `0x240f` 这样的值——明显不是一个合法的物理地址。

**排查过程**（这部分讲的是修 parser 边界检查**之前**的旧症状）：

`addr2line` 定位到 `parse_mem_region` 函数——FF-A 内存描述符解析器。描述符里的 `composite_offset` 字段应该是 80，但读出来是一个看起来随机的值。当时的 parser 还没把 offset/长度的边界检查做严，离谱的 offset 直接被拿去做 `base + offset` 的指针运算 → Data Abort。每次测试拿到的"垃圾值"都不一样。后来这套 parser 单独加了 bounded check（看 `src/ffa/descriptors.rs`），所以现在哪怕读到坏数据也只会返回 `FFA_INVALID_PARAMETERS`，不再崩——但根本问题（**为什么会读到坏数据**）还是要解决。

描述符放在 pKVM 的 TX buffer 里——这是 Normal World DRAM。问题出在这条链上：

```text
pKVM (CPU 0): 写描述符到 TX buffer → SMC → 进入 EL3
SPMD (EL3): 切到 S-EL2
SPMC (S-EL2): 读 TX buffer → 解析描述符
```

这里的陷阱是：SPMC 的**后续处理可能被调度到另一个 pCPU 上跑**。比如 pKVM 从 CPU 2 发起的一个 FFA_RUN 让 SPMC 恢复了 SP，SP 在 CPU 2 的 S-EL2 上读那段由 pKVM 在 CPU 0 写过的 TX buffer——CPU 2 这边的 cache line 状态不一定反映 CPU 0 的最新写。需要先讲清楚一点：**`dsb` 屏障只 order 执行它的那个 CPU 的访问**，它不会"反向"去 flush 别人 L1。让 pCPU 0 的写最终对 pCPU 2 可见，靠的是 ARM 的 Inner Shareable cache coherency 协议；写入方在合适时机做 `dsb ish/sy`、读取方按需做 `dsb` + 必要的 cache 维护，两边凑齐才完整。单凭一条 `smc` 既不是屏障，也不触发任何一致性流程。

第一次尝试：读 TX buffer 之前加 `DSB SY`（全系统范围的数据同步屏障）。

```rust
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)); }
```

仍然偶发失败——这其实可以预料。读侧的 `dsb sy` 只能 order SPMC 自己 CPU 上的访问；写入侧（pKVM）写完有没有做正确的屏障，是我控制不了的代码。靠一条读侧 barrier 不可靠，得换个思路。

**最终修复**：DSB + 整块拷贝到本地缓冲区，然后只从本地缓冲区解析：

```rust
// src/spmc_handler.rs
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }
let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8,
        local_buf.as_mut_ptr(),
        total_length,
    );
}
// 绝不直接解析共享 buffer——所有解析都在本地副本上
let parsed = parse_mem_region(local_buf.as_ptr(), total_length);
```

原理（小心别多承诺）：`copy_nonoverlapping` 把共享 buffer 的字节先一次性挪到本地副本——拷贝过程仍可能横跨多个新旧 cache line，**它不保证你拿到的是 producer 那个时刻的快照**。但有一点变了：解析器从此只读这份不会再变的本地字节流。bounded parse 跑在固定字节流上，要么解析成功，要么 offset/长度越界返回 `FFA_INVALID_PARAMETERS`——不会去追共享 buffer 里的指针、不会因为再次读时字节又变了而崩。加上这段修复后，测试里不再复现原来那种间歇性 Data Abort。

**教训**：跨世界、跨 CPU 的共享内存原地解析是个坏习惯。跨安全世界的共享属性 / 缓存一致性的实际效果跟你在 Normal World 里习惯的那套不一样。防御性做法是：**`dsb sy` + 先拷到本地、解析只读本地**。这个 pattern 不解决跨核可见性问题，但它把"读到坏数据"的后果从"不可恢复的 Data Abort"降级成"返回错误码"——后者要好 debug 几个数量级。

---

## 坑四：SPMD 是 per-CPU 的（或者说：去读固件源码）

**现象**：pKVM 在 CPU 0 上正常启动。Secondary CPU（CPU 1/2/3）全部挂起，永远无法完成 PSCI CPU_ON。

**排查过程**：

FF-A 规范描述了 SPMC 的初始化流程，但对 secondary CPU 几乎只字不提。规范告诉你**做什么**，不告诉你**怎么串起来**。

花了几个小时读 TF-A 的源码（不是文档，是 C 代码），找到了 `spmd_cpu_on_finish_handler()` 函数。真相是：**SPMD 为每个物理 CPU 维护完全独立的状态**。每个 secondary CPU 进入 S-EL2 之后，必须调用 `FFA_MSG_WAIT` 完成一个握手——这个握手告诉 SPMD："这个 CPU 的 Secure World 已经就绪了。"如果任何一个 CPU 跳过了这个握手，SPMD 就不会完成对应的 PSCI CPU_ON 调用，于是 Normal World 的 secondary CPU 也永远启动不了。

```
CPU 0: SPMC init → boot SPs → FFA_MSG_WAIT ✓ → SPMD 完成 → pKVM 启动
CPU 1: ???  → 没有 FFA_MSG_WAIT → SPMD 阻塞 → pKVM CPU_ON 挂起
CPU 2: ???  → 同上
CPU 3: ???  → 同上
```

我最初的代码让 secondary CPU 做了 `WFE`（Wait For Event）然后等待——这是 Normal World 的标准模式。但在 Secure World 里，SPMD 需要 per-CPU 的握手。

**修复**：关键有三步。

第一步，在 SPMC 初始化阶段注册 secondary CPU 的入口地址：

```rust
// src/main.rs — SPMC init
extern "C" { fn secondary_entry_sel2(); }
let ep = secondary_entry_sel2 as *const () as usize as u64;
let result = forward_smc8(FFA_SECONDARY_EP_REGISTER, ep, 0, 0, 0, 0, 0, 0);
```

第二步，给每个 secondary CPU 分配独立的栈（3 × 32KB，在 `.bss.sel2_pcpu_stacks` 段）：

```asm
// arch/aarch64/boot_sel2.S
.section .bss.sel2_pcpu_stacks
.align 16
sel2_pcpu_stacks:
    .space 3 * 32 * 1024   // CPU 1, 2, 3 各 32KB
```

第三步，每个 secondary 上电后**按顺序**初始化 EL2 执行环境，`FFA_MSG_WAIT` 握手，然后进入 per-CPU 事件循环：

```text
CPU 1..3: secondary_entry_sel2
  → 设置每 CPU 栈
  → 安装 VBAR_EL2（exception::init）
  → 开 HCR_EL2.VM（Secure Stage-2）
  → 清 CPTR_EL2 / MDCR_EL2 的 trap 位（FP、SVE、SME、debug）
  → 复用 primary 的 S-EL2 Stage-1 页表（install_sel2_stage1_secondary）
  → 开该 CPU 的 GICR PPI 26/29（poll 定时器 + 安全物理 timer）
  → FFA_MSG_WAIT  ← 握手，SPMD 放行该 CPU 的 PSCI CPU_ON
  → run_event_loop()  ← 持续处理后续到达该 CPU 的 SMC 请求
```

**顺序不能乱**：必须先放开 trap（CPTR/MDCR）再开 MMU，否则某些 TF-A 配置下会在打开 Stage-1 的 `isb` 处直接陷入 EL3。

`FFA_MSG_WAIT` 只是入口；真正让 secondary 持续可用的是后面那个 `run_event_loop()`——每个 CPU 都要留在自己的事件循环里处理后续请求，否则下一次 pKVM 在这个 CPU 上发 SMC，SPMC 不会响应。

**教训**：ARM 的规范文档（DEN0077A 等）是"接口定义"，不是"实现指南"。它告诉你有哪些 SMC 调用、参数格式、返回值。但怎么把它们串起来——什么时候调用、在哪个 CPU 上调用、调用顺序——你得去读固件源码。

TF-A 的代码是事实上的参考实现。`services/std_svc/spm/` 目录下的 SPMD 代码比规范文档的"Lifecycle"章节信息量大得多。如果你要跟 TF-A 打交道，把它的源码当文档读。

---

## 总结

| 坑 | 层面 | 表现 | 根因 |
|----|------|------|------|
| SIMD trap | 编译器代码生成 | Debug 模式静默挂起 | Rust 安全检查编译为 NEON 指令，被 CPTR_EL3.TFP trap |
| NS bit 写入 | 事务属性 | 数据写成功但读回全零 | Secure/Non-secure 是两个独立物理地址空间，S-EL2 Stage-1 要打 NS=1 |
| 幽灵失败 | 共享内存解析 | 间歇性 Data Abort（旧 parser）→ 间歇性 INVALID_PARAMETERS（修过 bounds check 后） | 跨 CPU/跨世界共享 TX buffer 不能原地解析，需 `dsb sy` + 本地拷贝 |
| SPMD per-CPU | 固件状态机 | Secondary CPU 永远挂起 | 每个 CPU 都要完成 `FFA_MSG_WAIT` 握手 + 进自己的事件循环，规范没写 |

四个 bug 有一个共同点：**它们在你的层面看起来都是对的**。代码逻辑对，地址对，调用顺序对。但在更低的层面（编译器、MMU、cache、固件），有一些你不知道的约束正在被违反。

在裸机开发中，你的抽象层下面没有操作系统帮你兜底。你的对手不是你的代码——是你对硬件的心智模型跟硬件实际行为之间的差距。

缩小这个差距的唯一办法是：**反汇编你的二进制，读硬件手册，读固件源码**。没有捷径。

---

*这是 ARM64 Hypervisor 开发系列的第四篇。之前的文章：*
- *Part 0a: 为什么写一个 Hypervisor*
- *Part 0b: AI 辅助系统编程*
- *Part 1: 从零到 "Hello from EL2!"*
- *Part 2: 陷入-模拟-恢复*
- *Part 3: 四个 CPU、一块磁盘 — 让 Linux 启动*

*项目开源：[github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor)*
