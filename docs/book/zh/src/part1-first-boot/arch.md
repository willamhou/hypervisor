# 架构：ARM64 异常等级

## 特权模型

ARM64 定义了四个异常等级，每个都是严格的特权边界：

```
EL3  ─  安全监控器（固件，TrustZone）
EL2  ─  Hypervisor（我们的代码在这里）
EL1  ─  OS 内核（Linux，guest）
EL0  ─  用户空间（应用程序）
```

EL 越高 = 特权越大。EL2 的代码可以配置硬件陷阱来拦截 EL1 的操作——这是硬件虚拟化的根本机制。当 EL1 的 guest 执行一条敏感指令（访问系统寄存器、执行 WFI、触发 SMC）时，硬件会陷入 EL2，由 hypervisor 决定如何处理。

## 为什么是 EL2？

EL2 是专门为 hypervisor 设计的。它提供：

- **Stage-2 地址翻译**（`VTTBR_EL2`、`VTCR_EL2`）—— 第二层地址翻译，将 guest 物理地址（IPA）映射到真实物理地址。Guest 以为自己拥有从 0x40000000 开始的内存；hypervisor 控制那里真正放的是什么。
- **陷阱配置**（`HCR_EL2`）—— 一个 64 位寄存器，控制哪些 guest 操作会陷入 EL2。想拦截 WFI？设 TWI 位。想陷入 SMC？设 TSC 位。想陷入所有系统寄存器访问？也有对应的位。
- **虚拟中断注入**（`ICH_LR*_EL2`）—— GICv3 的虚拟接口让 hypervisor 无需直接修改 guest 状态就能注入中断。
- **VMID 标记的 TLB**（`VTTBR_EL2[63:48]`）—— 硬件 TLB 标记，使多个 VM 可以共存，无需在每次上下文切换时刷新 TLB。

这些在 EL1 都不存在。如果 hypervisor 尝试在 EL1 运行，所有这些都需要软件模拟——性能差几个数量级。

## QEMU 如何把我们送到 EL2

在真实硬件上，启动固件（通常在 EL3）会配置 HCR_EL2 然后降到 EL2，再交给 hypervisor。我们用 QEMU 的 `-machine virt` 加特定参数来跳过这些复杂性：

```bash
qemu-system-aarch64 \
  -machine virt,virtualization=on \
  -cpu max \
  -nographic \
  -kernel hypervisor.bin
```

关键是 `-machine virt,virtualization=on`。没有 `virtualization=on` 的话，QEMU 会在 EL1 启动内核。加上它，QEMU 内置的固件会配置好 CPU，直接在 EL2 进入我们的二进制。

我们可以在 `boot.S` 里读 `CurrentEL` 来验证：

```armasm
mrs     x0, CurrentEL
lsr     x0, x0, #2    // 提取 EL 字段（bits [3:2]）
cmp     x0, #2        // 应该是 2（EL2）
b.ne    halt           // 如果不是 EL2，出问题了
```

在第一版 `boot.S` 里有这个检查。后来被移除了——如果我们不在 EL2，反正也做不了什么有用的事，安静地挂起就行。

## 后面会用到的关键寄存器

目前我们不碰任何 EL2 系统寄存器。但预告一下后面会用到的：

| 寄存器 | 用途 | 首次使用 |
|--------|------|----------|
| `HCR_EL2` | Hypervisor 配置寄存器 — 陷阱控制 | Part 2（guest 执行） |
| `VTTBR_EL2` | Stage-2 翻译表基地址 | Part 2（内存） |
| `VTCR_EL2` | Stage-2 翻译控制 | Part 2（内存） |
| `VBAR_EL2` | 异常向量基地址 | Part 3（异常处理） |
| `ESR_EL2` | 异常综合征（退出原因） | Part 3（异常处理） |
| `ELR_EL2` | 异常链接寄存器（返回地址） | Part 3（异常处理） |
| `SPSR_EL2` | 保存的处理器状态 | Part 2（guest 入口） |

在这部分，我们只需要 `CurrentEL`（验证在 EL2）和 `SP`（栈指针，在汇编里设置）。
