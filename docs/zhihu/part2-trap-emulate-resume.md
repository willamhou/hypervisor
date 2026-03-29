# 陷入-模拟-恢复 — Hypervisor 的心跳

## 写在前面

上一篇写了四个文件、两个 commit、一句 "Hello from EL2!"。那是在 EL2 执行代码的证明，但还不是虚拟化。

虚拟化的本质是一个无限循环：guest 跑 → 碰到特权操作 → 硬件陷入 hypervisor → hypervisor 模拟该操作 → 恢复 guest 继续跑。ARM 架构手册把这叫做 **trap-and-emulate**。我更喜欢说"陷入-模拟-恢复"，因为第三步才是关键——恢复。模拟完了你得能回去。

这篇讲的就是这个循环。它是 hypervisor 的心跳。Part 0a 说过，这个循环本身一个周末就能写完，然后花接下来 9 个月跟周边的一切搏斗。

但在搏斗之前，你得先有心跳。

---

## 一个 Guest 能干什么？

在我们往 EL1 放一个真正的 Linux 内核之前，先用最小的代码回答一个问题：**一个 guest 从 EL2 的视角看，能产生哪些"动静"？**

ARM64 硬件会把以下操作从 EL1 陷入到 EL2（前提是 HCR_EL2 里对应的 bit 被置了）：

| 陷入原因 | ESR_EL2 EC 值 | 什么时候发生 |
|----------|---------------|-------------|
| WFI/WFE | 0x01 | Guest 等待中断或事件 |
| HVC | 0x16 | Guest 发起 hypervisor 调用 |
| SMC | 0x17 | Guest 发起安全监控器调用 |
| MSR/MRS | 0x18 | Guest 访问被 trap 的系统寄存器 |
| Data Abort | 0x24 | Guest 访问了未映射的地址（MMIO） |
| Instruction Abort | 0x20 | Guest 从未映射的地址取指 |

每一种陷入都是一次心跳。Hypervisor 必须正确处理每一次，然后把控制权还给 guest。

这些陷入原因在代码里对应一个枚举：

```rust
// src/arch/aarch64/regs.rs
pub enum ExitReason {
    Unknown,
    WfiWfe,           // Guest 等待中断
    HvcCall,          // Hypervisor 调用
    SmcCall,          // 安全监控器调用
    TrapMsrMrs,       // 系统寄存器访问
    InstructionAbort,
    DataAbort,        // MMIO 的入口
    Other(u64),
}
```

从 ESR_EL2 的高 6 位（Exception Class）解码出 `ExitReason`，然后 match。hypervisor 里大量的代码都在这个 match 的各个分支里。

---

## VcpuContext：一个 CPU 的全部状态

当 guest 陷入 EL2，硬件只帮你做了两件事：保存 PC 到 ELR_EL2，保存 PSTATE 到 SPSR_EL2。其它所有东西——30 个通用寄存器、SP、EL1 系统寄存器——都需要软件自己保存。

这就是 VcpuContext 的工作：

```rust
// src/arch/aarch64/regs.rs
#[repr(C)]
pub struct VcpuContext {
    pub gp_regs: GeneralPurposeRegs,  // x0-x30
    pub sys_regs: SystemRegs,         // ESR, FAR, SP_EL1, ELR_EL1, SPSR_EL1, ...
    pub sp: u64,
    pub pc: u64,                      // 恢复点 → ELR_EL2
    pub spsr_el2: u64,                // Guest PSTATE → SPSR_EL2
}
```

`#[repr(C)]` 不是装饰。这个结构体的内存布局必须和汇编代码里的偏移量一一对应——汇编用硬编码的数字偏移来 `ldp`/`stp` 寄存器。Rust 默认的字段布局不保证顺序，`repr(C)` 让它变成 C 语言的布局规则：按声明顺序排列。

换句话说，如果你在 Rust 侧加了一个字段，忘了更新汇编偏移量，结果不是编译错误——是安静的寄存器错位，guest 跑着跑着飞到一个随机地址。后来我们加了编译期偏移量断言来防御这种事：

```rust
// 编译期检查：如果偏移量不对，编译直接报错
const _: () = {
    assert!(core::mem::offset_of!(VcpuContext, pc) == 392);
    assert!(core::mem::offset_of!(VcpuContext, spsr_el2) == 400);
};
```

---

## enter_guest：最关键的 50 行汇编

整个 hypervisor 里最重要的函数只有 50 行汇编。它做三件事：保存宿主状态、恢复 guest 状态、ERET。

```asm
// arch/aarch64/exception.S
.global enter_guest
enter_guest:
    // x0 = VcpuContext 指针
    msr     tpidr_el2, x0         // 存到 per-CPU 寄存器，中断处理用

    // 保存宿主的 callee-saved 寄存器
    stp     x29, x30, [sp, #-16]!
    stp     x27, x28, [sp, #-16]!
    stp     x25, x26, [sp, #-16]!
    stp     x23, x24, [sp, #-16]!
    stp     x21, x22, [sp, #-16]!
    stp     x19, x20, [sp, #-16]!

    // 恢复 guest 的 30 个通用寄存器
    ldp     x2, x3, [x0, #16]
    ldp     x4, x5, [x0, #32]
    // ... x6-x30

    // 恢复 guest PC 和 PSTATE
    ldr     x1, [x0, #392]       // pc → ELR_EL2
    msr     elr_el2, x1
    ldr     x1, [x0, #400]       // spsr_el2 → SPSR_EL2
    msr     spsr_el2, x1

    // 最后恢复 x0, x1（因为它们之前被用作临时寄存器）
    ldp     x0, x1, [x0, #0]

    // 进入 guest
    eret
```

`eret` 是 ARM64 的"异常返回"指令。它做的事情恰好是 enter_guest 的镜像：从 ELR_EL2 恢复 PC，从 SPSR_EL2 恢复 PSTATE，切换到 EL1。从这一刻起，CPU 开始执行 guest 代码。

当 guest 碰到下一次陷入，硬件自动切回 EL2，跳到异常向量表。异常处理代码把 guest 寄存器存回 VcpuContext，调用 Rust 的 `handle_exception()`，处理完再走一遍 enter_guest。

**这就是心跳。**

```
enter_guest()                    handle_exception()
    ↓                                ↓
    保存宿主                          读 ESR_EL2
    恢复 guest                        match ExitReason
    ERET → guest                      处理陷入
           ↓                          ↓
           guest 执行...              写回 VcpuContext
           ↓                          ↓
           陷入 (硬件)  ←────────────→ 恢复 guest / 退出
```

---

## Stage-2 页表：给 Guest 一个假的物理地址空间

Guest 以为自己有一整块物理内存。实际上，它的每一次内存访问都经过了两层翻译：

```
Guest VA  ──Stage-1(EL1)──→  Guest PA (IPA)  ──Stage-2(EL2)──→  真实 PA
              guest 自己管                     hypervisor 管
```

Stage-1 是 guest 内核自己的页表，跟一个普通 Linux 的页表没区别。Stage-2 是 hypervisor 加的第二层翻译——guest 完全不知道它的存在。

在我们的实现里，Stage-2 用的是 **identity mapping**：GPA（guest 物理地址）== HPA（真实物理地址）。这极大简化了实现——你不需要维护一个 GPA→HPA 的映射表，也不需要在 DMA 路径上做地址翻译。代价是 guest 的"物理地址空间"必须和真实硬件的物理布局对齐。对 QEMU virt 这种固定布局的平台来说，这不是问题。

页表用 2MB block 粒度映射：

```rust
// src/arch/aarch64/mm/mmu.rs
fn map_2mb_block(&mut self, addr: u64, attrs: MemoryAttributes) {
    let l0_index = ((addr >> 39) & PT_INDEX_MASK) as usize;
    let l1_index = ((addr >> 30) & PT_INDEX_MASK) as usize;
    let l2_index = ((addr >> 21) & PT_INDEX_MASK) as usize;

    // L0 → L1 → L2，L2 entry 直接是 2MB block descriptor
    self.l2_tables[l2_table_idx]
        .set_entry(l2_index, S2PageTableEntry::block(addr, attrs));
}
```

ARM 的 Stage-2 页表有四级（L0→L1→L2→L3），但 L2 entry 可以直接指向一个 2MB 的物理块而不必再展开到 L3。对于连续的大块内存（RAM、ROM），2MB block 足够了。只有需要精细控制的区域（比如后面要讲的 GICR trap）才需要拆到 4KB 的 L3 页。

Stage-2 的另一个功能是**控制 guest 能看到什么**。把一段地址从 Stage-2 页表里删掉，guest 访问它就会触发 Data Abort——陷入 EL2。这正是 MMIO 模拟的入口。

---

## MMIO Trap-and-Emulate：Guest 写了一个不存在的地址

当 guest 写 UART 的数据寄存器（0x09000000），实际发生了什么？

1. Guest 执行 `str w0, [x1]`（x1 = 0x09000000）
2. Stage-1 翻译：VA → IPA（假设 guest 映射了这个地址）
3. Stage-2 翻译：IPA 0x09000000 → **没有映射！**
4. 硬件触发 Data Abort，陷入 EL2
5. Hypervisor 从 ESR_EL2 解码出这是一次 MMIO 写
6. 从指令编码里提取源寄存器（w0）和写入值
7. 查设备路由表，找到 VirtualUart
8. 调用 `VirtualUart.write(offset=0, value=字符)`
9. VirtualUart 把字符写到真实的物理 UART
10. 推进 guest PC + 4，恢复 guest

这 10 步每输出一个字符走一遍。这就是 trap-and-emulate——代价不小，但它让 guest 完全不需要知道底下的硬件是虚拟的。

设备路由用 enum dispatch 而不是 trait objects：

```rust
// src/devices/mod.rs
pub enum Device {
    Uart(pl011::VirtualUart),
    Gicd(gic::VirtualGicd),
    Gicr(gic::VirtualGicr),
    VirtioBlk(virtio::mmio::VirtioMmioTransport<virtio::blk::VirtioBlk>),
    VirtioNet(virtio::mmio::VirtioMmioTransport<virtio::net::VirtioNet>),
    Pl031(pl031::VirtualPl031),
}
```

为什么不用 `Box<dyn MmioDevice>`？因为我们是 `no_std`，没有全局分配器（至少在早期阶段没有）。即使后来加了堆分配器，enum dispatch 也比 vtable 间接调用快——CPU 的分支预测器更喜欢直接跳转。在 hypervisor 这种极度热路径上，每次 MMIO trap 省几纳秒是值得的。

DeviceManager 的路由逻辑也很直白——8 个 slot 的数组，线性扫描：

```rust
pub fn handle_mmio(&mut self, addr: u64, ...) -> Option<u64> {
    for slot in self.devices.iter_mut() {
        if let Some(dev) = slot {
            if dev.contains(addr) {
                let offset = addr - dev.base_address();
                return if is_write {
                    dev.write(offset, value, size);
                    None
                } else {
                    dev.read(offset, size)
                };
            }
        }
    }
    // 没有设备匹配 → 读返回 0，写忽略
    if is_write { None } else { Some(0) }
}
```

8 个 slot 线性扫描，不优雅。但设备数量就这么几个（UART、GIC、virtio-blk、virtio-net、RTC），O(N) 和 O(1) 的差距在 N=6 时不存在。

---

## VirtualUart：最简单的设备

UART 模拟是 hypervisor 里最简单的设备，却也是你最先写的设备——因为没有它你什么都看不到。

```rust
// src/devices/pl011/emulator.rs
impl VirtualUart {
    fn output_char(&self, ch: u8) {
        unsafe {
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) ch as u32,
                options(nostack),
            );
        }
    }
}
```

Guest 写虚拟 UART → Data Abort → hypervisor 模拟 → 写真实 UART。就这么简单。

但有一个细节：Linux 的 PL011 驱动在 probe 阶段会读 PrimeCell ID 寄存器（地址偏移 0xFE0-0xFFC）。如果读回来的值不对，驱动直接放弃，串口没了。所以 VirtualUart 必须正确模拟这些 ID 寄存器——4 个字节的 Peripheral ID + 4 个字节的 PrimeCell ID。一共 32 字节的只读数据，决定了 Linux 能不能认出这块 UART。

---

## 踩坑：HPFAR_EL2 — 一天的代价

Part 0b 的 AI 工作流篇提到过这个 bug，这里展开讲。

故事是这样的：MMIO trap-and-emulate 在裸机 guest（没有 MMU）上一直工作正常。但当 Linux 开启 MMU 之后，所有 MMIO 设备突然消失了。virtio-mmio 报 "Wrong magic value 0x00000000"。

原因很简单，但非常 tricky：

```
Guest MMU OFF:  FAR_EL2 = IPA    ← 恰好是对的（VA == IPA）
Guest MMU ON:   FAR_EL2 = VA     ← 错了！这是 guest 内核虚拟地址
```

当 guest MMU 开启后，Stage-1 翻译 VA→IPA，Stage-2 翻译 IPA→PA（或 fault）。Data Abort 时，`FAR_EL2` 保存的是触发 fault 的**虚拟地址**，不是 IPA。真正的 IPA 在 `HPFAR_EL2` 里：

```rust
// HPFAR_EL2[43:4] = IPA 的页号
// FAR_EL2[11:0] = 页内偏移
let ipa_page = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8;
let page_offset = far_el2 & 0xFFF;
let addr = ipa_page | page_offset;
```

`HPFAR_EL2` 存的是 IPA 右移 12 位后左移 4 位的值（ARM 的寄存器设计就是这么...别问为什么），需要左移 8 位还原。然后低 12 位的页内偏移还是要从 `FAR_EL2` 取——因为 `HPFAR_EL2` 只有页级精度。

修复是一行代码的事。但定位它花了一天。

### 为什么难定位

现象是"virtio-mmio 读到 magic = 0"。直觉是设备没注册、或者 Stage-2 映射错了。顺着这个方向查了半天——Stage-2 映射没问题（virtio 地址 0x0a000000 在映射范围内），设备注册也没问题。

问题出在更上游：MMIO trap 的地址解析。当 handle_mmio_abort 收到的 addr 是一个 guest 内核虚拟地址（0xFFFF...something），没有任何设备匹配，函数返回默认值 0。

**裸机开发的一个反直觉规律**：当系统的多个层都正确地完成了自己的工作，但组合起来结果错了，问题通常在**接口的语义**上——这里是"FAR_EL2 到底是 VA 还是 IPA"。

### AI 在这里的表现

这是 AI 辅助系统编程的一个典型案例。我跟 Claude 描述了现象："virtio-mmio 读 magic 为 0，但设备已注册"。AI 的第一反应是检查设备注册逻辑、Stage-2 映射范围、virtio-mmio 初始化顺序——全是合理的下游排查方向。

当我自己看了一眼 handle_mmio_abort 的 addr 参数值（0xFFFF...），问题就清楚了。这个值太大了，不可能是物理地址。AI 不知道这个——因为它没有在运行时看到寄存器值的能力。一旦我指出"FAR_EL2 在 guest MMU on 的时候是 VA"，AI 立刻给出了 HPFAR_EL2 的正确用法和位移计算。

这里的模式很清晰：**AI 对 "HPFAR_EL2 怎么用" 的知识是准确的，但它不具备"这个运行时值看起来不对"的直觉。** 架构知识 OK，运行时诊断需要人。

---

## 踩坑：SPSR_EL2 绝对不能碰

这个 bug 没有 HPFAR_EL2 那么戏剧性，但更阴险。

背景：Guest 在执行关键代码时会用 `msr daifset, #2` 来屏蔽 IRQ（设置 PSTATE.I = 1）。这是 Linux 内核里的自旋锁实现——持锁期间关中断，避免死锁。

正常流程：guest 关中断 → 执行临界区 → 打开中断。PSTATE.I 的值反映在 SPSR_EL2 里（因为每次陷入 EL2 时硬件自动把当前 PSTATE 存到 SPSR_EL2）。

出问题的场景：hypervisor 在处理某个 exit 时，"好心地"清除了 SPSR_EL2 的 I bit，想让 guest 能收到中断。ERET 恢复 guest 时 PSTATE.I 被清零——此时 guest 以为自己还持着锁、中断还是关的，但实际上中断已经开了。

结果：中断打断了自旋锁的临界区，中断处理程序试图获取同一把锁 → 死锁。

教训很简单：

```rust
// DO NOT modify SPSR_EL2 (guest's saved PSTATE).
// Guest controls its own interrupt masking via DAIF.
// Overriding PSTATE.I causes spinlock deadlocks in the guest.
```

一行注释，值一个下午的调试。

---

## 测试：在裸机上"断言"

传统软件的测试是运行一个函数、比较返回值。裸机 hypervisor 的"测试"是：启动一个 guest，让它跑完，检查它是不是活着。

我们的测试策略分两层：

**第一层：宿主侧单元测试。** 不需要启动 guest，直接测试数据结构和逻辑。比如测试 Stage-2 页表能不能正确映射一个地址，测试 DeviceManager 能不能正确路由一个 MMIO 请求。这些测试跑在 `make run` 里，快且确定性强。

**第二层：Guest 交互测试。** 写一个极小的 guest（几十行汇编），让它做一个特定操作（比如发一个 HVC、写一个 MMIO 地址），然后在 hypervisor 侧检查结果。比如：

```rust
pub fn run_test_guest() {
    // 测试：guest 发 HVC #0，hypervisor 应该能看到 ExitReason::HvcCall
    // Guest 代码（编译好的二进制，内联在 Rust 里）
    let guest_code: [u32; 2] = [
        0xd4000002,  // hvc #0
        0xd503205f,  // wfe（停住）
    ];
    // ... 加载到 guest 内存，启动，检查退出原因
}
```

这种测试的信噪比很高——如果 guest 没有按预期陷入，要么是 HCR_EL2 的 trap 位没设对，要么是 enter_guest 的 ERET 没到正确的地址。排查范围很小。

到 Part 2 结束时，我们有约 30 个这样的断言。不多，但每一个都在保护一条关键路径。

---

## 精简版：整个循环一图看完

```
                 ┌──────────────────────────────────────────┐
                 │              Hypervisor (EL2)            │
                 │                                          │
                 │  ┌─────────┐      ┌──────────────────┐  │
     ┌───────────│──│  enter  │──────│  handle_exception │──│──────┐
     │           │  │  guest  │      │                   │  │      │
     │           │  └─────────┘      │  match exit {     │  │      │
     │           │                   │    WFI → yield    │  │      │
     │           │                   │    HVC → hypercall│  │      │
     │           │                   │    SMC → forward  │  │      │
     │           │                   │    DataAbort →    │  │      │
     │           │                   │      MMIO emulate │  │      │
     │           │                   │  }                │  │      │
     │           │                   └──────────────────┘  │      │
     │           └──────────────────────────────────────────┘      │
     │    ERET                                          trap       │
     ▼                                                    ▲        │
┌─────────────────────────────────────────────────────────────────┐│
│                        Guest (EL1)                              ││
│                                                                 ││
│  Linux 内核 / 裸机 guest                                         ││
│  ├── 正常执行...                                                 ││
│  ├── str w0, [uart]  → Stage-2 fault → Data Abort ──────────────┘│
│  ├── wfi             → HCR_EL2.TWI trap ─────────────────────────┘
│  └── smc #0          → HCR_EL2.TSC trap ────────────────────────┘
└──────────────────────────────────────────────────────────────────┘
```

Stage-2 页表决定了哪些地址让 guest 直接访问（RAM），哪些故意不映射以触发 Data Abort（MMIO 设备）。VcpuContext 保存和恢复 CPU 的全部状态。`enter_guest()` 和异常向量表构成了 EL1↔EL2 的跳板。

这就是 hypervisor 的心跳。它在每个 MMIO 访问、每次 WFI、每次系统调用时跳动。Linux 每秒产生成千上万次这样的陷入——打印一行日志就是几十次。

下一篇：让这个心跳驱动一个真正的 Linux 内核启动。GICv3 虚拟化、virtio-blk、4 个 vCPU 的调度。那是 95% 的工程量。

---

## 小结

| 概念 | 一句话 |
|------|--------|
| VcpuContext | Guest 的全部 CPU 状态，`repr(C)` 保证和汇编偏移对齐 |
| enter_guest | 50 行汇编，保存宿主 → 恢复 guest → ERET |
| Stage-2 | 第二层地址翻译，identity map，2MB block 粒度 |
| MMIO | 故意不映射 → Data Abort → hypervisor 模拟设备 |
| HPFAR_EL2 | Guest MMU 开启后，FAR_EL2 是 VA，IPA 在 HPFAR_EL2 |
| SPSR_EL2 | 不要碰。Guest 的中断屏蔽状态是它自己的事 |
| 测试 | 宿主侧单元测试 + 极小 guest 交互测试 |

这些东西合在一起，大约花了两天。Sprint 1.1（vCPU 框架）到 Sprint 1.4（设备模拟），在 2026 年 1 月 26 日一天之内完成了主要 commit。第二天调通了中断注入。

两天写完心跳。接下来的故事，是那另外 95%。
