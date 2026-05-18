# 我花了 3 天才搞清楚 TrustZone 的 NS 位

> 发布日期：4/24
> 同一块 DRAM，从 Secure 和 Non-Secure 看到的是不同的地址区域——AXI 总线上多出来的一个 bit 决定了这一切。

---

我在写 ARM64 hypervisor 的第 8 周，碰到一个 pKVM 读不到我写的数据的 bug，排查了 3 天。这个 bug 揭示了一个我之前没完全理解的事实：**TrustZone 的 Secure/Non-Secure 不是权限模型，是 AXI 总线上的第二个地址空间**。

## 背景

我的 hypervisor 运行在 ARM 的 S-EL2，负责管理 Secure World 的虚拟机（Secure Partitions，SP）。同一颗芯片上，Google 的 pKVM 运行在 NS-EL2 管理 Normal World。

两者通过 EL3 固件中转通信，协议是 FF-A。其中 `PARTITION_INFO_GET` 调用让 pKVM 读 SPMC 写的分区描述符。

## Bug

我的内部测试客户端（BL33 harness，跑在 Normal World EL1）调用 `PARTITION_INFO_GET`，工作正常。

换成 pKVM 调用——**pKVM 读到全 0**。

GDB attach 到 QEMU，SPMC 端看地址 `0x42a16000`：

```
(gdb) x/6gx 0x42a16000
0x42a16000: 0x8001000000010001 0x0000000100000000
0x42a16010: 0x0008000000000000 0x0000000000000000
```

数据明明写进去了。pKVM 读同一个地址：全 0。

## TrustZone 的真正模型

读 ARM 架构手册（关于 Secure and Non-Secure memory 的章节），明白了。

我以前以为 TrustZone 是权限控制——Secure 代码可以访问 Normal 的内存，Normal 代码不能访问 Secure 的。

**实际不是。** TrustZone 在 AXI 总线级别引入了一个额外信号：**NS bit**。

- 从 S-EL2 发出的访问默认 NS=0（Secure）
- 从 NS-EL2/NS-EL1 发出的访问永远 NS=1（Non-Secure）

**同一块 DRAM 芯片**上的内存控制器（具体是 TZASC/TZC-400 之类的 TrustZone Address Space Controller）看到这个 NS bit，把物理地址**分区**：某些地址范围对 NS=1 请求响应，某些对 NS=0 响应。

所以地址 `0x42a16000` 从 Secure 世界看和从 Non-Secure 世界看**是同一块 DRAM 的不同语义区域**。两个世界各自有自己的数据。

## 修复

两件事：

1. **启用 S-EL2 Stage-1 MMU**
2. **在 MMU 页表里把 Normal World 的 DRAM 区域标记为 NS=1**

Stage-1 MMU 的 PTE 有个 NS bit。默认 0 表示"这次访问用 Secure"。设成 1 表示"这次访问标 NS=1 发到总线"。

修复后的映射：

```
0x00000000 - 0x08000000: Device Secure   (GIC, UART)
0x40000000 - 0x80000000: Normal NS=1     (pKVM's DRAM — 跨世界访问)
0x0E000000 - 0x10000000: Normal Secure   (SPMC 自己的代码和数据)
```

当 SPMC 写 `0x42a16000`，Stage-1 MMU 查页表看到 NS=1，AXI 事务标 NS=1，TZASC 路由到 Non-Secure 地址区域，pKVM 读到数据。

## 为什么 BL33 测试当时能通过

我之前跑 BL33 harness 测试时 `-machine virt` 没开严格的 `secure=on` 配置。那个模式下 QEMU 的 Secure/Non-Secure 隔离不完整。跑到 pKVM 场景时启用了完整 TF-A + `secure=on`，两个世界真正隔离了，bug 就暴露出来。

这段经历的教训是：**测试环境和生产环境的 Secure/NS 隔离模型可能不同**。如果你的测试 harness 和目标工作负载不在同一个 privilege level/world，要确认测试环境的分离强度和真实部署一致。

## 需要澄清的误解

有几个常见的误解我之前也有：

**误解 1**："Secure 和 Non-Secure 是不同的 DRAM 芯片"
**实际**：同一套 DRAM 颗粒。TZASC/NS controller 在 AXI 总线层做访问控制，按物理地址范围分区。

**误解 2**："SMC 指令是 memory barrier"
**实际**：ARMv8-A 规定 SMC 是 synchronous exception，只有 Context Synchronization Event，**对 memory ordering 没有保证**。跨 CPU 的数据共享需要显式 `dsb ish/sy`。

**误解 3**："SMC 会把后续处理路由到别的 CPU"
**实际**：SMC 是 synchronous exception，永远在发起 CPU 上同步处理。但 SP 被 `FFA_RUN` resume 时可以在另一个 pCPU 上执行——这是 SP 迁移，不是 SMC 迁移。

## 尾声

这个 bug 让我花了 3 天。每一个"它明明应该工作啊"的瞬间都是因为我默认了错误的心智模型。

如果你在做 ARM64 hypervisor 或者跨 world 内存共享：**每一个跨 world 的 buffer 都必须有显式 NS 属性**，不是通过权限控制，是通过总线标记。这是 TrustZone 的基础设计，也是最容易被忽略的细节。

---

代码：https://github.com/willamhou/hypervisor
博客：https://willamhou.github.io/hypervisor/
