# 3 天 debug：TrustZone 的 NS 位不只是权限，更是物理地址空间的选择

> 同一块 DRAM，同一个物理地址，SPMC 写进去有数据，pKVM 读出来全 0。问题不在权限，不在缓存，在 AXI 总线上一个我从没写过的属性位。

---

我在写一个 ARM64 裸机 hypervisor。它跑在 S-EL2（Secure World 的 hypervisor 层），同一颗芯片上，Google 的 pKVM 跑在 NS-EL2（Normal World 的 hypervisor 层）。两者通过 EL3 的 TF-A 中转 SMC 调用，协议是 FF-A。

其中有个调用叫 `FFA_PARTITION_INFO_GET`，pKVM 用它向我询问"你手下管着哪些安全分区（SP）"。SPMC 把 SP 列表的描述符写到 pKVM 的 RX 缓冲区里，pKVM 读回来。

这篇写一个我在这个调用上卡了 3 天的 bug——它的根因是我对 TrustZone 的心智模型从一开始就漏掉了一块关键的东西。

---

## 症状：写成功了，读出来全是 0

我最早是用自己写的 BL33 测试程序验证 `PARTITION_INFO_GET`：一个跑在 NS-EL1 的小 C 程序，发 SMC，读 RX buffer，检查返回的描述符格式。跑通了。SPMC 写什么，BL33 读什么，字节完全一致。

换成 pKVM 做同样的调用。pKVM 发 SMC，拿回的 `x2` 是正确的（描述符个数），但读自己的 RX buffer——**全 0**。

第一反应是字节序或者格式错了。GDB attach 到 QEMU，在 SPMC 端查 pKVM 注册的 RX buffer 地址 `0x42a16000`：

```text
(gdb) x/6gx 0x42a16000
0x42a16000: 0x8001000000010001 0x0000000100000000
0x42a16010: 0x0008000000000000 0x0000000000000000
```

数据明明在。切到 pKVM 的视角（在 NS-EL2），让它读同一个地址——全 0。

**写成功了**（SPMC 这边 volatile 写，GDB 能读到）。**地址是对的**（就是 pKVM 用 `FFA_RXTX_MAP` 注册的那个地址）。**不是缓存问题**（加了 `dsb sy` + `dc civac` 清 cache，没用）。**不是 pKVM 解析 bug**（pKVM 拿到了正确的 partition 计数）。

这不是我熟悉的任何一种 bug 类型。

---

## 半途的错误假设

在真正理解之前，我依次尝试过、都错了的假设：

**假设 1: QEMU 的 bug**  
"可能 QEMU 的 Secure/Non-Secure 隔离有问题。"  
实际上：QEMU 的模型是对的，**我的心智模型错了**。

**假设 2: FF-A 协议要求显式 flush**  
读 FF-A 规范，看有没有 explicit 的 `cache flush` 要求。规范写了内存共享时共享区域要用 Normal WB + Inner Shareable，但是没提 Secure/NS 属性——它假设实现方自己知道。

**假设 3: RX buffer 地址在我侧解释错了**  
`0x42a16000` 是 pKVM 传过来的 IPA（它自己的 Stage-1 虚地址）吗？这套实现里把它当 PA 处理（pKVM 这条调用路径上的 RXTX 是直接传物理地址，参见我的 `src/spmc_handler.rs` NWD_RXTX 处理）。所以 SPMC 拿到的就是 PA，不需要再做一层翻译。

三个假设都不对。真相在于：**"物理地址" 这个词在 TrustZone 系统里其实不只是一个数字**。

---

## TrustZone 不是权限模型，是总线信号

重读 Arm Architecture Reference Manual 关于 `Secure and Non-secure memory` 的章节，还读了 TF-A 的 Secure Partition Client Interface 文档和 TZC-400 (TrustZone Address Space Controller) 的手册。豁然开朗。

我以前的 mental model：

> TrustZone = 权限位。Secure 代码可以访问任何内存，Normal 代码只能访问"非安全"的内存。CPU 运行模式决定它属于 Secure 还是 Normal。

这个模型能解释 90% 的教程示例。但它**漏掉一个关键细节**：它假设"访问目标"（某块物理内存）天然归属于某一边，权限只是决定谁能看它。

更完整的模型是：

> TrustZone 不只是权限模型——它在架构上定义了 **两个独立的物理地址空间**：Secure 和 Non-Secure。CPU 发出的每一次 load/store 都带 `NS`（Non-Secure）属性，这次访问被路由到哪个地址空间，由这个属性决定。具体的强制实现因 SoC 而异（典型的有 Arm 自己的 TZASC / TZC-400 系列、各家 SoC 自定义的 NS 控制器），都做同一件事：按事务的 `NS` 属性裁决访问。

换句话说——**同一个数值地址在 Secure / Non-secure 两个物理地址空间下，是两个不同的架构地址**。

- `NS=0` 事务访问 `0x42a16000` → 走 Secure 物理地址空间
- `NS=1` 事务访问 `0x42a16000` → 走 Non-secure 物理地址空间

底层是同一颗 DRAM、同一颗 DRAM 上的不同区段，还是真正的两块独立存储，是 SoC 实现细节。架构层面要记住的是：**这是两个不同的物理地址空间**——内存控制器（TZASC、TZC 系列、或 SoC 自己的 NS 控制器）按 `NS` 属性裁决访问归哪一边。

这就是为什么我 SPMC 在 `0x42a16000` 写成功、pKVM 在 `0x42a16000` 读全 0：两边都"访问成功"了，但从架构看根本走的是两个不同的地址空间。

---

## 从 CPU 到总线：NS 位怎么被设成 0 还是 1

现在问题变成：**一次 CPU 发出的 load/store，它的 `NS` 位是谁决定的？**

答案在 Stage-1 MMU 里。

当 CPU 处于 Non-Secure 世界（NS-EL1/NS-EL2），硬件强制把所有 AXI 事务的 `NS` 位设为 1。pKVM 怎么折腾都是 `NS=1`。

当 CPU 处于 Secure 世界（S-EL1/S-EL2/EL3），**`NS` 位由 Stage-1 PTE 的 `NS` 位决定**：

- Stage-1 MMU 关闭时 → 默认 `NS=0`
- Stage-1 MMU 开启时 → 每条 PTE 的 bit 5 决定这次访问的 `NS`
  - PTE `NS=0` → 事务 `NS=0` (Secure access)
  - PTE `NS=1` → 事务 `NS=1` (Non-Secure access)

我的 bug 就是：**SPMC 当时没启用 S-EL2 Stage-1 MMU**。

为什么没启用？因为 SPMC 启动流程短，加上 SPMC 自己的代码和数据都在 Secure DRAM，MMU 关着也能跑。跨到 `0x42a16000`（pKVM 在 Normal DRAM 里的 buffer）就出事——硬件默认 `NS=0`，事务被 TZC 路由到 Secure 区，pKVM 永远看不到。

---

## 修法：给 S-EL2 建一张最小恒等映射

解法就一个：**在 SPMC 初始化里把 Stage-1 MMU 打开，给 NWd DRAM 区域的 PTE 打 `NS=1`**。

我不需要完整的虚拟内存——SPMC 自己不搞 userland，不换地址空间。建一张 identity map，覆盖四类区域就够：

```rust
// src/sel2_mmu.rs
// L1[1] = 1GB block at 0x4000_0000, NS=1, Normal WB, XN
S1_L1.0[1].store(
    0x4000_0000u64 | PTE_VALID | PTE_BLOCK | NORMAL_NS_XN,
    Ordering::Relaxed,
);

// L1[2] = 1GB block at 0x8000_0000, NS=1, Normal WB, XN
S1_L1.0[2].store(
    0x8000_0000u64 | PTE_VALID | PTE_BLOCK | NORMAL_NS_XN,
    Ordering::Relaxed,
);

// L2 blocks for SPMC + SPs + heap (0x0E00_0000..0x0FFF_FFFF): NS=0
for idx in 112..=127 {
    let addr = (idx as u64) << 21;
    S1_L2_LOW.0[idx].store(addr | PTE_VALID | PTE_BLOCK | NORMAL_S, Ordering::Relaxed);
}

// L2 blocks for GIC + UART (0x0800_0000..0x09FF_FFFF): NS=0, Device
for idx in 64..=79 {
    let addr = (idx as u64) << 21;
    S1_L2_LOW.0[idx].store(addr | PTE_VALID | PTE_BLOCK | DEVICE_S, Ordering::Relaxed);
}
```

布局一目了然：

```text
0x00000000 - 0x07FFFFFF:  (unmapped)
0x08000000 - 0x09FFFFFF:  Device, NS=0    GIC + UART
0x0E000000 - 0x0FFFFFFF:  Normal, NS=0    SPMC 代码 + SPs + 堆
0x40000000 - 0x7FFFFFFF:  Normal, NS=1    pKVM 的 DRAM (第 1GB)
0x80000000 - 0xBFFFFFFF:  Normal, NS=1    pKVM 的 DRAM (第 2GB)
```

然后是开 MMU 的标准动作——按代码里的顺序是：先 `dsb ishst` 保证页表 store 落盘，`tlbi alle2` 清 S-EL2 TLB，`dsb ish` 等 TLB 失效完成，`isb` 同步流水线；再写 `MAIR_EL2`/`TCR_EL2`/`TTBR0_EL2` + `isb`；最后置 `SCTLR_EL2.{M,C,I}` + `isb`。

打开以后，SPMC 访问 `0x42a16000`：Stage-1 查 L1[1]（`0x42a16000 >> 30 == 1`），PTE 的 `NS=1`，硬件在事务上打 `NS=1`。内存系统（TZASC / TZC 这类 NS 控制器）把这次访问路由到 Non-secure 物理地址空间。pKVM 立刻看到数据。

Secondary CPU 也一样——bootstrap 代码 `install_sel2_stage1_secondary()` 复用 primary 的页表，每个 CPU 自己 `msr ttbr0_el2, ...` + 开 MMU。页表是共享的，MMU 激活是 per-CPU 的。

---

## 几个容易被骗的误区

排查过程中我"差点相信了"的几个错误结论，都有文档或教程做背书，这里列出来：

**"Secure 和 Non-Secure 必须是不同的 DRAM 芯片"**  
不一定。架构层面只要求两个独立的**物理地址空间**，物理实现可以是不同芯片，也可以是同一颗 DRAM 内的不同区段，由 TZC-400 之类的 NS 控制器按 `NS` 属性裁决。很多 ARM 教程画架构图时把 Secure/Normal 画成两个内存块，给人物理隔离的错觉。

**"SMC 指令是 memory barrier"**  
不是。Arm Architecture Reference Manual（DDI 0487）里 `SMC` 被定义成同步异常，会触发 Context Synchronization Event，但**对内存顺序没有保证**。跨 CPU 共享数据之前需要显式 `dsb ish` 或 `dsb sy`。

**"SMC 会把请求路由到别的 CPU"**  
不会。SMC 是同步异常，永远在发起 CPU 上就地处理。但是 SP 被 `FFA_RUN` resume 时**可以在任一 pCPU 上执行**——这是 SP 调度，不是 SMC 迁核。两者混为一谈会让你写出错误的屏障逻辑。

**"开 `secure=on` 的 QEMU virt 和不开等价"**  
在我这套 setup 里不等价。早期 BL33 harness 没开 TF-A + `secure=on` 时跑得过，换到完整 TF-A + `secure=on` 才把 NS bit 这个 bug 露出来。这条结论严格说只覆盖 QEMU virt 这一种平台 / 配置组合，但教训是通用的：**测试和生产的隔离强度对得上，再去信测试结果**。

---

## 一句话总结

在 TrustZone 系统里，物理地址不是一个数字，它是 `(address, NS)` 两元组。

在 Secure 世界里跑代码、用 S-EL2 Stage-1 翻译时，决定 `NS` 的是 PTE 的 `NS` 位：MMU 关着 → 默认 `NS=0`，访问 Secure 物理地址空间；MMU 开着 → 看 PTE 的 `NS` 位。想让 Secure 侧 CPU 的 store 落到 Non-secure 物理地址空间，最直接的做法就是在 Stage-1 页表上把那段地址标 `NS=1`。

这不是权限的问题，是路由的问题。

---

代码：<https://github.com/willamhou/hypervisor>

博客：<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第六篇。之前的文章：*

- *Part 0a: [为什么写一个 Hypervisor](./part0a-why.md)*
- *Part 0b: [AI 辅助系统编程](./part0b-ai-workflow.md)*
- *Part 1: [从零到 "Hello from EL2!"](./part1-first-boot.md)*
- *Part 2: [陷入-模拟-恢复](./part2-trap-emulate-resume.md)*
- *Part 3: [让 Linux 启动](./part3-linux-boot.md)*
- *Part 4: [裸机四大坑](./part4-war-stories.md)*
- *Part 5: [Rust enum 状态机的真相](./part5-enum-state-machine.md)*
