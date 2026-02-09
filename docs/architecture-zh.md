# ARM64 虚拟机监控器架构参考手册

> **代码规模**: 约 7,700 行, 涵盖 30+ 源文件 (Rust + ARM64 汇编)
> **目标平台**: QEMU `virt` 虚拟机, ARMv8 虚拟化扩展
> **客户操作系统**: Linux 6.12 (arm64) 和 Zephyr RTOS

---

## 目录

1. [系统总览](#1-系统总览)
2. [启动与初始化](#2-启动与初始化)
3. [vCPU 管理](#3-vcpu-管理)
4. [虚拟机管理](#4-虚拟机管理)
5. [Stage-2 内存管理](#5-stage-2-内存管理)
6. [异常处理](#6-异常处理)
7. [GICv3 中断控制器](#7-gicv3-中断控制器)
8. [定时器虚拟化](#8-定时器虚拟化)
9. [设备模拟框架](#9-设备模拟框架)
10. [客户系统启动](#10-客户系统启动)
11. [架构抽象层](#11-架构抽象层)
12. [附录](#附录)

---

## 1. 系统总览

### 1.1 架构图

```
 ┌──────────────────────────────────────────────────────────────────┐
 │                         QEMU virt 虚拟机                         │
 │  CPU: max (ARMv8+VHE)   RAM: 1GB   GIC: v3   UART: PL011      │
 └──────────────────────────────────────────────────────────────────┘
        │
        │  硬件
 ═══════╪══════════════════════════════════════════════════════════════
        │  软件
        │
 ┌──────┴──────────────────────────────────────────────────────────┐
 │                    EL2 — 虚拟机监控器 (Hypervisor)                │
 │                                                                  │
 │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐ │
 │  │  boot.S  │→ │rust_main │→ │ 测试套件 │→ │  客户系统加载器  │ │
 │  │ (入口点) │  │ (初始化) │  │          │  │  (Zephyr/Linux)  │ │
 │  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘ │
 │                                                                  │
 │  ┌───────────────────────┐  ┌──────────────────────────────────┐ │
 │  │     异常处理层        │  │       虚拟机 / vCPU 层           │ │
 │  │  ┌────────────────┐   │  │  ┌──────┐ ┌──────┐ ┌─────────┐ │ │
 │  │  │ exception.S    │   │  │  │ Vm   │ │ Vcpu │ │调度器   │ │ │
 │  │  │ (向量表)       │   │  │  └──────┘ └──────┘ └─────────┘ │ │
 │  │  │ (上下文保存/恢复)│ │  └──────────────────────────────────┘ │
 │  │  └────────┬───────┘   │                                       │
 │  │           │            │  ┌──────────────────────────────────┐ │
 │  │  ┌────────▼───────┐   │  │         硬件接口层               │ │
 │  │  │ exception.rs   │   │  │  ┌───────┐ ┌─────┐ ┌──────────┐│ │
 │  │  │ (异常分发)     │   │  │  │GICv3  │ │定时 │ │  MMU     ││ │
 │  │  └────────┬───────┘   │  │  │(ICC/  │ │ 器  │ │(Stage-2) ││ │
 │  │           │            │  │  │ ICH)  │ │     │ │          ││ │
 │  │  ┌────────▼───────┐   │  │  └───────┘ └─────┘ └──────────┘│ │
 │  │  │ decode.rs      │   │  └──────────────────────────────────┘ │
 │  │  │ (MMIO 指令解码)│   │                                       │
 │  │  └────────┬───────┘   │  ┌──────────────────────────────────┐ │
 │  │           │            │  │        设备模拟层                │ │
 │  │  ┌────────▼───────┐   │  │  ┌────────┐ ┌────────┐          │ │
 │  │  │ DeviceManager  │◄──│──│  │ PL011  │ │ GICD   │          │ │
 │  │  │ (MMIO 路由)    │   │  │  │ (串口) │ │(分发器)│          │ │
 │  │  └────────────────┘   │  │  └────────┘ └────────┘          │ │
 │  └───────────────────────┘  └──────────────────────────────────┘ │
 │                                                                  │
 │  ┌──────────────────────────────────────────────────────────────┐ │
 │  │  内存管理: BumpAllocator → GlobalHeap → 页表                │ │
 │  └──────────────────────────────────────────────────────────────┘ │
 └──────────────────────────────────────────────────────────────────┘
        │
        │  ERET / 异常
 ═══════╪══════════════════════════════════════════════════════════════
        │
 ┌──────┴──────────────────────────────────────────────────────────┐
 │                    EL1 — 客户系统 (Guest)                        │
 │                                                                  │
 │  ┌──────────────────────────────────────────────────────────────┐ │
 │  │  Linux 6.12 (arm64)  或  Zephyr RTOS  或  测试用客户代码    │ │
 │  │                                                              │ │
 │  │  可见资源: 虚拟 CPU, 虚拟 GIC (ICC_* 经由 ICH), UART,      │ │
 │  │            恒等映射 RAM, 虚拟定时器 (PPI 27)                 │ │
 │  └──────────────────────────────────────────────────────────────┘ │
 └──────────────────────────────────────────────────────────────────┘
```

### 1.2 物理内存布局

```
 地址              大小        描述
 ────────────────────────────────────────────────────────
 0x0000_0000                  (QEMU 固件 ROM)
      ...
 0x0800_0000     64 KB        GIC 分发器 (GICD)
 0x0801_0000                  GIC CPU 接口 (GICC, GICv2)
 0x080A_0000                  GIC 重分发器 (GICR, GICv3)
      ...
 0x0900_0000      4 KB        PL011 串口 (UART)
      ...
 0x4000_0000                  ┌─ 虚拟机监控器代码 (.text, .rodata, .data)
                              │  (由 QEMU -kernel 加载)
                              │
 0x4000_4000     16 KB        │  虚拟机监控器栈 (向下增长)
      ...                     │
 0x4100_0000     16 MB        │  虚拟机监控器堆 (BumpAllocator)
      ...                     │
 0x4200_0000                  └─ 堆结束地址
      ...
 0x4700_0000                  Linux DTB (设备树)
 0x4800_0000                  ┌─ 客户代码 (内核 Image 或 Zephyr ELF)
                              │  (由 QEMU -device loader 加载)
      ...                     │
 0x6800_0000                  └─ Linux 客户结束地址 (512 MB)
```

### 1.3 模块依赖关系图

```
                        main.rs
                           │
              ┌────────────┼──────────────┐
              ▼            ▼              ▼
        guest_loader    tests/         exception::init()
              │                           │
              ▼                           ▼
            vm.rs ◄─────────────── exception.rs
              │                     │         │
     ┌────────┼────────┐           │         ▼
     ▼        ▼        ▼           │     decode.rs
  vcpu.rs  scheduler  mm/mmu.rs    │
     │                    │        │
     ▼                    ▼        ▼
  vcpu_interrupt.rs    defs.rs   global.rs ──► devices/mod.rs
     │                                          │         │
     ▼                                          ▼         ▼
  gicv3.rs ◄──────────────────────────────  pl011/     gic/
     │                                      emulator   distributor
     ▼
  timer.rs

  arch/traits.rs ◄── (由 gicv3, mmu, regs, timer 实现)
  platform.rs    ◄── (被 vm, guest_loader, heap, uart, gic 使用)
  lib.rs         ◄── (uart_puts, uart_put_hex — 全局使用)
```

### 1.4 源文件索引

| 文件 | 行数 | 用途 |
|------|------:|------|
| `arch/aarch64/boot.S` | 44 | 入口点、栈设置、BSS 清零 |
| `arch/aarch64/exception.S` | 469 | 异常向量表、enter_guest、上下文保存/恢复 |
| `arch/aarch64/linker.ld` | 35 | 链接器脚本, 基地址 0x4000_0000 |
| `src/lib.rs` | 79 | crate 根模块, uart_puts/uart_put_hex 工具函数 |
| `src/main.rs` | 170 | rust_main 入口, 测试编排, 客户启动分发 |
| `src/platform.rs` | 30 | QEMU virt 平台常量 (地址、大小) |
| `src/global.rs` | 52 | 异常处理器访问的全局 DeviceManager |
| `src/arch/mod.rs` | 12 | 架构模块根 |
| `src/arch/traits.rs` | 76 | 可移植 trait 定义 |
| `src/arch/aarch64/mod.rs` | 20 | ARM64 模块根, 重导出 |
| `src/arch/aarch64/defs.rs` | 94 | 系统寄存器和 PTE 位域的命名常量 |
| `src/arch/aarch64/regs.rs` | 409 | VcpuContext, GeneralPurposeRegs, SystemRegs, ExitReason |
| `src/arch/aarch64/hypervisor/exception.rs` | 926 | 异常分发、PSCI、MMIO、MSR/MRS、WFI、IRQ 处理 |
| `src/arch/aarch64/hypervisor/decode.rs` | 135 | MMIO 模拟指令解码器 |
| `src/arch/aarch64/mm/mmu.rs` | 500 | Stage-2 页表、IdentityMapper、DynamicIdentityMapper |
| `src/arch/aarch64/peripherals/timer.rs` | 182 | ARM 通用定时器, 虚拟定时器访问 |
| `src/arch/aarch64/peripherals/gic.rs` | 164 | GICv2 回退方案 (基于 MMIO 的分发器/CPU 接口) |
| `src/arch/aarch64/peripherals/gicv3.rs` | 657 | GICv3 系统寄存器 (ICC_\*), 虚拟接口 (ICH_\*) |
| `src/vcpu.rs` | 268 | 虚拟 CPU, 状态机, enter_guest 封装 |
| `src/vcpu_interrupt.rs` | 210 | VirtualInterruptState, HCR_EL2.VI 回退 |
| `src/vm.rs` | 303 | 虚拟机, 内存初始化, 调度器集成 |
| `src/scheduler.rs` | 124 | 轮转 (Round-robin) vCPU 调度器 |
| `src/devices/mod.rs` | 238 | MmioDevice trait, DeviceManager 路由器 |
| `src/devices/pl011/emulator.rs` | 235 | 虚拟 UART (透传到真实 PL011) |
| `src/devices/gic/distributor.rs` | 144 | 虚拟 GIC 分发器 (GICD 模拟) |
| `src/mm/allocator.rs` | 51 | Bump 分配器 (简单, 不回收) |
| `src/mm/heap.rs` | 68 | 全局堆单例 |
| `src/guest_loader.rs` | 328 | GuestConfig, Linux/Zephyr 启动, EL1 寄存器初始化 |
| `src/uart.rs` | 121 | UART 驱动 (直接硬件访问) |
| `build.rs` | 74 | 汇编编译, libboot.a 创建 |

---

## 2. 启动与初始化

### 2.1 功能说明

将虚拟机监控器从上电状态带入可以创建虚拟机和运行客户代码的就绪状态。启动序列从 EL2 的汇编代码开始, 之后转入 Rust 代码。

### 2.2 启动流程

```
 上电 (QEMU -kernel 将虚拟机监控器加载到 0x4000_0000)
      │
      ▼
 ┌──────────────────────────────────────────────────────────────┐
 │  _start (boot.S)                                             │
 │  1. 设置 SP 为 stack_top (0x4000_0000 + 0x4000)             │
 │  2. 清零 BSS 段 (__bss_start → __bss_end)                   │
 │  3. bl rust_main                                             │
 └──────────────────────────────────────────────────────────────┘
      │
      ▼
 ┌──────────────────────────────────────────────────────────────┐
 │  rust_main() (main.rs)                                       │
 │                                                              │
 │  1. exception::init()                                        │
 │     ├─ 写入 VBAR_EL2 ← &exception_vector_table             │
 │     └─ 配置 HCR_EL2 (RW|SWIO|FMO|IMO|AMO|FB|BSU|          │
 │                        TWI|TWE|APK|API)                      │
 │                                                              │
 │  2. gicv3::init()                                            │
 │     ├─ ICC_SRE_EL2 = SRE | Enable                           │
 │     ├─ ICC_SRE_EL1 = SRE                                    │
 │     ├─ GicV3VirtualInterface::init()                         │
 │     │  ├─ ICH_HCR_EL2 = 1 (En)                              │
 │     │  ├─ ICH_VMCR_EL2 = VPMR=0xFF | VENG1=1                │
 │     │  └─ 清空所有 List Register                             │
 │     ├─ ICC_CTLR_EL1.EOImode = 1                             │
 │     └─ GicV3SystemRegs::enable() (PMR=0xFF, IGRPEN1=1)      │
 │                                                              │
 │  3. timer::init_hypervisor_timer()                           │
 │     └─ CNTHCTL_EL2 |= EL1PCTEN | EL1PCEN                   │
 │                                                              │
 │  4. mm::heap::init()                                         │
 │     └─ BumpAllocator::new(0x4100_0000, 0x100_0000)          │
 │                                                              │
 │  5. 运行测试套件 (12 个测试)                                │
 │                                                              │
 │  6. [feature = "guest"]     → run_guest(zephyr_default)      │
 │     [feature = "linux_guest"] → run_guest(linux_default)     │
 │                                                              │
 │  7. loop { wfe }                                             │
 └──────────────────────────────────────────────────────────────┘
```

### 2.3 异常向量表布局

```
 VBAR_EL2 (2KB 对齐, .text.exception 段)

 偏移     向量                           处理函数
 ──────── ─────────────────────────────  ─────────────────────────
 +0x000   Sync,  当前 EL, SP_EL0        → exception_handler
 +0x080   IRQ,   当前 EL, SP_EL0        → exception_handler
 +0x100   FIQ,   当前 EL, SP_EL0        → exception_handler
 +0x180   SError,当前 EL, SP_EL0        → exception_handler

 +0x200   Sync,  当前 EL, SP_ELx        → exception_handler
 +0x280   IRQ,   当前 EL, SP_ELx        → exception_handler
 +0x300   FIQ,   当前 EL, SP_ELx        → exception_handler
 +0x380   SError,当前 EL, SP_ELx        → exception_handler

 +0x400   Sync,  低异常级, AArch64      → exception_handler  ← 客户陷入
 +0x480   IRQ,   低异常级, AArch64      → irq_exception_handler ← 物理 IRQ
 +0x500   FIQ,   低异常级, AArch64      → exception_handler
 +0x580   SError,低异常级, AArch64      → exception_handler

 +0x600   Sync,  低异常级, AArch32      → exception_handler  (不支持)
 +0x680   IRQ,   低异常级, AArch32      → exception_handler
 +0x700   FIQ,   低异常级, AArch32      → exception_handler
 +0x780   SError,低异常级, AArch32      → exception_handler

 每个条目: .align 7 (128 字节), 包含一条跳转指令。
 总计: 16 条目 x 128 字节 = 2048 字节 (2KB)。
```

### 2.4 关键类型

**启动代码中未定义类型。** 启动阶段为纯汇编代码。

### 2.5 源文件

- `arch/aarch64/boot.S` — 入口点、栈设置、BSS 清零
- `arch/aarch64/exception.S` — 向量表、enter_guest、上下文保存/恢复
- `arch/aarch64/linker.ld` — 内存布局、段放置
- `src/main.rs` — rust_main、测试分发、客户启动
- `build.rs` — 汇编编译, 创建 libboot.a

---

## 3. vCPU 管理

### 3.1 功能说明

代表一个虚拟处理器。管理客户寄存器上下文、执行生命周期和虚拟中断状态。

### 3.2 关键类型

```rust
// src/vcpu.rs

pub enum VcpuState {
    Uninitialized,   // 尚未配置
    Ready,           // 可以被调度
    Running,         // 在 EL1 执行中
    Stopped,         // 已终止
}

pub struct Vcpu {
    id: usize,                          // 唯一标识符 (0..7)
    state: VcpuState,                   // 生命周期状态
    context: VcpuContext,               // 全部客户寄存器 (repr(C))
    virt_irq: VirtualInterruptState,    // 待处理虚拟 IRQ/FIQ
}
```

### 3.3 vCPU 状态机

```
                     ┌─────────────────┐
                     │  Uninitialized   │
                     │  (未初始化)      │
                     └────────┬────────┘
                              │ new(id, entry, sp)
                              ▼
            ┌────────► ┌───────────┐ ◄──────────┐
            │          │   Ready   │             │
            │          │  (就绪)   │             │
            │          └─────┬─────┘             │
            │                │ run()             │
            │                ▼                   │
            │          ┌───────────┐             │
            │          │  Running  │─────────────┘
            │          │  (运行中) │  客户退出 → state = Ready
            │          │  (EL1)    │
            │          └─────┬─────┘
            │                │ 致命错误
            │                ▼
            │          ┌───────────┐
            └──────────│  Stopped  │
              reset()  │  (已停止) │
                       └───────────┘
```

### 3.4 VcpuContext 内存布局 (repr(C))

此布局**在 Rust 和汇编之间共享**。偏移量在 `exception.S` 中被硬编码。

```
 偏移    字段                大小    说明
 ──────  ──────────────────  ──────  ─────────────────────────
      0  gp_regs.x0           8     ┐
      8  gp_regs.x1           8     │
     16  gp_regs.x2           8     │
     24  gp_regs.x3           8     │
     ..  ...                  ...    │ GeneralPurposeRegs
    224  gp_regs.x28          8     │ (31 x 8 = 248 字节)
    232  gp_regs.x29 (FP)     8     │
    240  gp_regs.x30 (LR)     8     ┘
    248  sys_regs.sp_el1       8     ┐
    256  sys_regs.elr_el1      8     │
    264  sys_regs.spsr_el1     8     │
    272  sys_regs.sctlr_el1    8     │
    280  sys_regs.ttbr0_el1    8     │
    288  sys_regs.ttbr1_el1    8     │ SystemRegs
    296  sys_regs.tcr_el1      8     │ (17 x 8 = 136 字节)
    304  sys_regs.mair_el1     8     │
    312  sys_regs.vbar_el1     8     │
    320  sys_regs.contextidr   8     │
    328  sys_regs.tpidr_el1    8     │
    336  sys_regs.tpidrro_el0  8     │
    344  sys_regs.tpidr_el0    8     │
    352  sys_regs.esr_el2      8     │
    360  sys_regs.far_el2      8     │
    368  sys_regs.hcr_el2      8     │
    376  sys_regs.cntvoff_el2  8     ┘
    384  sp                    8     客户栈指针
    392  pc                    8     客户程序计数器 (ELR_EL2)
    400  spsr_el2              8     陷入时的客户 PSTATE

 总计: 408 字节

 汇编访问 (exception.S):
   [x0, #0]    → x0, x1 (stp/ldp 对)
   [x0, #248]  → sp_el1
   [x0, #256]  → elr_el1
   [x0, #264]  → spsr_el1
   [x0, #384]  → sp
   [x0, #392]  → pc (ELR_EL2)
   [x0, #400]  → spsr_el2
```

### 3.5 公开 API

| 函数 | 签名 | 描述 |
|------|------|------|
| `new` | `(id: usize, entry: u64, sp: u64) -> Self` | 创建就绪状态的 vCPU |
| `run` | `(&mut self) -> Result<(), &'static str>` | 进入客户, 退出时返回 |
| `stop` | `(&mut self)` | 转为 Stopped 状态 |
| `reset` | `(&mut self, entry: u64, sp: u64)` | 重置为 Ready 状态 |
| `inject_irq` | `(&mut self, irq_num: u32)` | 入队虚拟中断 |
| `has_pending_interrupt` | `(&self) -> bool` | 检查待处理状态 |
| `clear_irq` | `(&mut self)` | 清除待处理中断 |
| `context_mut` | `(&mut self) -> &mut VcpuContext` | 访问寄存器 |

### 3.6 实现要点

- `run()` 调用汇编 `enter_guest()`, 后者通过 ERET 进入 EL1
- `enter_guest()` 返回值: 0 = 正常退出, 1 = WFI 退出
- 进入客户前, `apply_to_hcr()` 设置 HCR_EL2.VI 位 (仅限传统模式; GICv3 使用 List Register)
- 退出后, 待处理中断自动清除 (硬件处理 EOI)

### 3.7 源文件

- `src/vcpu.rs` — Vcpu 结构体、状态机、run/stop/reset
- `src/arch/aarch64/regs.rs` — VcpuContext, GeneralPurposeRegs, SystemRegs, ExitReason
- `src/vcpu_interrupt.rs` — VirtualInterruptState, HCR_EL2 辅助函数

---

## 4. 虚拟机管理

### 4.1 功能说明

将 vCPU、Stage-2 内存和模拟设备组织成一个虚拟机。提供生命周期管理和调度器集成。

### 4.2 关键类型

```rust
// src/vm.rs

pub const MAX_VCPUS: usize = 8;

pub enum VmState {
    Uninitialized,    // 无 vCPU, 无内存
    Ready,            // 已配置, 可运行
    Running,          // 执行中
    Paused,           // 已暂停
    Stopped,          // 已终止
}

pub struct Vm {
    id: usize,
    state: VmState,
    vcpus: [Option<Vcpu>; MAX_VCPUS],   // 最多 8 个 vCPU
    vcpu_count: usize,
    memory_initialized: bool,
    scheduler: Scheduler,
}
```

### 4.3 虚拟机状态机

```
 ┌─────────────────┐
 │  Uninitialized   │ ◄── Vm::new(id)
 │  (未初始化)      │
 └────────┬────────┘
          │ create_vcpu() / add_vcpu()
          ▼
 ┌─────────────┐           ┌─────────────┐
 │    Ready    │ ◄────────│   Paused     │
 │   (就绪)   │ resume()  │  (已暂停)   │
 └──────┬──────┘           └──────▲──────┘
        │ run()                   │ pause()
        ▼                         │
 ┌─────────────┐─────────────────┘
 │   Running   │
 │  (运行中)   │
 └──────┬──────┘
        │ 客户退出 → Ready
        │ stop()
        ▼
 ┌─────────────┐
 │   Stopped   │
 │  (已停止)   │
 └─────────────┘
```

### 4.4 init_memory 流程

```
 vm.init_memory(guest_mem_start, guest_mem_size)
      │
      ├─ IdentityMapper::reset()
      │
      ├─ map_region(start_aligned, size_aligned, NORMAL)
      │    └─ 客户 RAM: 恒等映射, 写回缓存
      │
      ├─ map_region(0x0800_0000, 16MB, DEVICE)
      │    └─ GIC: 分发器 + 重分发器
      │
      ├─ map_region(0x0900_0000, 2MB, DEVICE)
      │    └─ UART: PL011 串口控制台
      │
      └─ init_stage2(&mapper)
           ├─ HCR_EL2 |= VM           (启用 Stage-2)
           ├─ Stage2Config::install()  (VTCR_EL2, VTTBR_EL2)
           └─ tlbi vmalls12e1is        (刷新 TLB)
```

### 4.5 公开 API

| 函数 | 签名 | 描述 |
|------|------|------|
| `new` | `(id: usize) -> Self` | 创建 VM, 安装全局 DeviceManager |
| `init_memory` | `(&mut self, start: u64, size: u64)` | 设置 Stage-2 页表 |
| `create_vcpu` | `(&mut self, id: usize) -> Result<&mut Vcpu>` | 按 ID 创建 vCPU |
| `add_vcpu` | `(&mut self, entry: u64, sp: u64) -> Result<usize>` | 自动分配 ID 创建 vCPU |
| `run` | `(&mut self) -> Result<()>` | 运行 vCPU 0 |
| `schedule` | `(&mut self) -> Option<usize>` | 选择下一个 vCPU (轮转) |
| `run_current` | `(&mut self) -> Result<()>` | 运行已调度的 vCPU |
| `yield_current` | `(&mut self)` | 让出当前 vCPU |
| `block_current` | `(&mut self)` | 阻塞当前 vCPU |
| `unblock` | `(&mut self, vcpu_id: usize)` | 唤醒被阻塞的 vCPU |

### 4.6 源文件

- `src/vm.rs` — Vm 结构体, 内存初始化, 调度器集成
- `src/scheduler.rs` — 轮转调度器 (Round-robin Scheduler)

---

## 5. Stage-2 内存管理

### 5.1 功能说明

使用 ARM64 页表实现 Stage-2 地址翻译 (IPA → PA)。所有映射均为恒等映射 (IPA == PA)。使用 2MB 块描述符 (无 4KB 页)。

### 5.2 关键类型

```rust
// src/arch/aarch64/mm/mmu.rs

#[repr(transparent)]
pub struct S2PageTableEntry(u64);       // 64 位页表项

#[repr(C, align(4096))]
pub struct PageTable {
    entries: [S2PageTableEntry; 512],   // 512 个条目 x 8 字节 = 4KB
}

pub struct IdentityMapper {             // 静态分配 (在 BSS 段中)
    l0_table: PageTable,                // 1 张表
    l1_table: PageTable,                // 1 张表
    l2_tables: [PageTable; 4],          // 最多 4 张 L2 表
    l2_count: usize,
}

pub struct DynamicIdentityMapper {      // 堆分配
    l0_table: u64,                      // 堆分配表的地址
    l1_table: u64,
    l2_tables: [u64; 4],
    l2_count: usize,
}

pub struct MemoryAttributes { bits: u64 }

// 预定义属性:
//   NORMAL:   MemAttr=0b1111, S2AP=RW, SH=Inner, AF=1
//   DEVICE:   MemAttr=0b0000, S2AP=RW, SH=Non,   AF=1
//   READONLY: MemAttr=0b1111, S2AP=RO, SH=Inner, AF=1
```

### 5.3 页表层级结构

```
 48 位 IPA:  [47:39]   [38:30]   [29:21]    [20:0]
              L0 索引   L1 索引   L2 索引    块内偏移 (2MB)

 ┌────────────────┐
 │  L0 表         │  512 个条目, 每个覆盖 512GB
 │  (VTTBR_EL2)   │  仅使用条目 [0] (前 512GB)
 └───────┬────────┘
         │ 表描述符 → L1 地址
         ▼
 ┌────────────────┐
 │  L1 表         │  512 个条目, 每个覆盖 1GB
 │                │  使用条目 [0], [1], [2]:
 │                │  GIC (0x0-1GB), UART (0x0-1GB),
 │                │  客户 RAM (1GB-2GB)
 └───────┬────────┘
         │ 表描述符 → L2 地址
         ▼
 ┌────────────────┐
 │  L2 表 (最多4张)│  512 个条目, 每个覆盖 2MB 块
 │                │  带属性的块描述符
 └────────────────┘

 页表项格式 (Stage-2 块描述符):
 ┌──────────────────────────────────────────────────────────────────┐
 │ 63   ...   48 │ 47          12 │ 11  10 │ 9  8 │ 7  6 │ 5  2 │1│0│
 │    (高位)     │   输出地址      │  --  AF│  SH  │ S2AP │MemAttr│T│V│
 └──────────────────────────────────────────────────────────────────┘
 V   = 有效位 (1 = 条目有效)
 T   = 表/块 (0 = 块, 1 = 表)
 MemAttr[3:0] = 内存类型 (0b1111=普通, 0b0000=设备)
 S2AP[1:0]    = Stage-2 访问权限 (01=只读, 11=读写)
 SH[1:0]      = 共享性 (00=非共享, 11=内部共享)
 AF           = 访问标志 (必须为 1)
```

### 5.4 VTCR_EL2 配置

```
 VTCR_EL2 = T0SZ(16) | SL0(Level0) | IRGN0(WB) | ORGN0(WB) |
            SH0(Inner) | TG0(4KB) | PS(48-bit)

 字段       值      含义
 ─────────  ──────  ─────────────────────────────────
 T0SZ       16      48 位 IPA 空间 (64 - 16 = 48)
 SL0        2       从 Level 0 开始
 IRGN0      0b01    内部写回缓存
 ORGN0      0b01    外部写回缓存
 SH0        0b11    内部共享
 TG0        0b00    4KB 粒度
 PS         0b101   48 位物理地址空间
```

### 5.5 公开 API

| 函数 | 签名 | 描述 |
|------|------|------|
| `IdentityMapper::new` | `() -> Self` | 创建空映射器 (const) |
| `map_region` | `(&mut self, start: u64, size: u64, attrs: MemoryAttributes)` | 映射 2MB 对齐的区域 |
| `reset` | `(&mut self)` | 清除所有映射 |
| `config` | `(&self) -> Stage2Config` | 获取 VTCR/VTTBR 值 |
| `install` | `(&self)` | 写入 VTCR_EL2 和 VTTBR_EL2 |
| `init_stage2` | `(mapper: &IdentityMapper)` | 启用 Stage-2 (HCR_EL2.VM=1, TLB 刷新) |

### 5.6 实现要点

- `IdentityMapper` 使用静态分配 (在 BSS 段中) — 无需堆
- `DynamicIdentityMapper` 从全局堆分配页表
- 最多 4 张 L2 表 → 4 x 512 x 2MB = 4TB 覆盖范围 (绰绰有余)
- `init_stage2` 设置 HCR_EL2.VM=1 并使用 `tlbi vmalls12e1is` 刷新 TLB

### 5.7 源文件

- `src/arch/aarch64/mm/mmu.rs` — 页表、IdentityMapper、DynamicIdentityMapper
- `src/mm/allocator.rs` — BumpAllocator (为 DynamicIdentityMapper 分配页)
- `src/mm/heap.rs` — 全局堆单例 (基于 BumpAllocator)

---

## 6. 异常处理

### 6.1 功能说明

在 EL2 拦截所有客户陷入, 解码异常原因, 并分发到相应的处理函数。这是核心的陷入-模拟循环。

### 6.2 异常分发流程图

```
 客户 @ EL1 执行指令
      │
      │ 异常 / 陷入 / IRQ
      ▼
 ┌──────────────────────────────────────────────────────────┐
 │  exception.S: exception_handler / irq_exception_handler  │
 │  1. 将 x0-x3 压栈保存                                    │
 │  2. 加载 current_vcpu_context 指针                       │
 │  3. 保存 x0-x30, sp_el1, elr_el1, spsr_el1              │
 │  4. 保存 ELR_EL2 → context.pc                            │
 │  5. 保存 SPSR_EL2 → context.spsr_el2                     │
 │  6. bl handle_exception / handle_irq_exception           │
 └──────────────┬─────────────────────────┬─────────────────┘
                │ 返回 true              │ 返回 false
                ▼                         ▼
        恢复上下文,                  guest_exit:
        ERET → 客户               恢复宿主寄存器,
                                   返回 enter_guest 调用方

 ┌──────────────────────────────────────────────────────────┐
 │  handle_exception(context) → bool                        │
 │                                                          │
 │  1. 读取 ESR_EL2 和 FAR_EL2                             │
 │  2. 检查异常循环计数器 (> 100 → 致命错误)               │
 │  3. 从 ESR_EL2[31:26] 提取 EC                           │
 │                                                          │
 │     EC=0x01 (WFI/WFE)                                    │
 │     ├─ handle_wfi_with_timer_injection()                 │
 │     ├─ 若定时器待处理: 注入 IRQ, 前进 PC → true          │
 │     └─ 无定时器: → false (退出到宿主)                    │
 │                                                          │
 │     EC=0x16 (HVC)                                        │
 │     ├─ 从 ESR[15:0] 提取 HVC 立即数                     │
 │     ├─ 0x4A48: Jailhouse 调试控制台                      │
 │     ├─ x0 & 0x80000000: PSCI 调用                        │
 │     └─ x0 = 0/1: 自定义超级调用                          │
 │                                                          │
 │     EC=0x18 (MSR/MRS 陷入)                               │
 │     ├─ 解码 ISS: Op0, Op1, CRn, CRm, Op2, Rt            │
 │     ├─ MRS (读取): emulate_mrs → 写入 Rt                 │
 │     └─ MSR (写入): 读取 Rt → emulate_msr                │
 │                                                          │
 │     EC=0x24/0x25 (数据中止)                              │
 │     ├─ 读取 FAR_EL2 (故障地址)                          │
 │     ├─ ISV=1: 从 ISS 解码 (SAS,SRT,WnR)                 │
 │     ├─ ISV=0: 解码 context.pc 处的指令                   │
 │     └─ 路由到 DeviceManager::handle_mmio()               │
 │                                                          │
 │     EC=0x20/0x21 (指令中止)                              │
 │     └─ 转储 EL1 状态, 转储 S2 页表 → false              │
 │                                                          │
 │     EC=0x07/0x09/0x19 (FP/SVE 陷入)                      │
 │     └─ 跳过指令 (不应发生) → true                        │
 │                                                          │
 │     其他 EC                                               │
 │     └─ 记录日志并退出 → false                            │
 └──────────────────────────────────────────────────────────┘
```

### 6.3 IRQ 异常处理

```
 客户运行时的物理 IRQ (IMO=1 → 陷入到 EL2)
      │
      ▼
 irq_exception_handler (exception.S)
      │ 与同步处理相同的保存/恢复流程
      ▼
 handle_irq_exception(context) → bool
      │
      ├─ 确认中断: ICC_IAR1_EL1 → intid
      ├─ 若 intid >= 1020: 虚假中断 → 返回 true
      │
      ├─ intid == 27 (虚拟定时器 PPI):
      │   ├─ mask_guest_vtimer()  (CNTV_CTL_EL0.IMASK=1)
      │   ├─ inject_hw_interrupt(27, 27, 0xA0)   HW=1
      │   │      └─ 虚拟 EOI 自动去激活物理中断
      │   ├─ 不要修改 SPSR_EL2  ← 关键规则
      │   └─ EOIR(27)  (仅优先级降级, EOImode=1)
      │
      └─ 其他 intid:
          ├─ 记录警告
          ├─ EOIR(intid)
          └─ DIR(intid)  (显式去激活, 非 HW)
```

### 6.4 PSCI 模拟

| 函数 ID | 处理方式 | 返回值 |
|---------|---------|--------|
| `0x84000000` | PSCI_VERSION | `0x00000002` (v0.2) |
| `0x8400000A` | PSCI_FEATURES | SUCCESS 或 NOT_SUPPORTED |
| `0x84000002` | CPU_OFF | 退出客户 |
| `0xC4000003` | CPU_ON | SUCCESS (桩实现) |
| `0xC4000004` | AFFINITY_INFO | 0 (ON) |
| `0x84000006` | MIGRATE_INFO_TYPE | 2 (不支持) |
| `0x84000008` | SYSTEM_OFF | 退出客户 |
| `0x84000009` | SYSTEM_RESET | 退出客户 |
| `0x84000001` | CPU_SUSPEND | SUCCESS (视为 WFI) |

### 6.5 关键规则: 绝对不要修改客户 SPSR_EL2

```
 !! 不要清除 SPSR_EL2 中的 PSTATE.I (位 7) !!

 原因: 客户控制自己的中断屏蔽。

 如果客户在禁用中断 (PSTATE.I=1) 的情况下持有自旋锁,
 而我们强制清除 I 位来传递定时器 IRQ:

   客户持有自旋锁 → I=1
        ↓ 虚拟机监控器清除 SPSR_EL2 中的 I 位
   ERET → 客户以 I=0 恢复
        ↓ 定时器 IRQ 立即触发
   客户 IRQ 处理程序运行
        ↓ 处理程序尝试获取相同的自旋锁
   死锁 (queued_spin_lock_slowpath)

 正确行为:
   - 虚拟 IRQ 在 List Register 中保持待处理状态
   - 客户以原始 PSTATE (I=1) 恢复 (ERET)
   - 当客户执行 spin_unlock + local_irq_restore 时
   - 硬件自动传递待处理的虚拟 IRQ
```

### 6.6 异常循环防护

```
 static EXCEPTION_COUNT: AtomicU32 (每次成功处理后重置)
 const MAX_CONSECUTIVE_EXCEPTIONS: u32 = 100

 每次异常时:
   count = EXCEPTION_COUNT.fetch_add(1)
   if count > 100:
     打印致命诊断信息 (ESR, FAR, PC)
     Loop { wfe }   ← 硬停机
```

### 6.7 源文件

- `src/arch/aarch64/hypervisor/exception.rs` — 异常分发、PSCI、MSR/MRS、MMIO、WFI、IRQ
- `src/arch/aarch64/hypervisor/decode.rs` — 指令解码器 (ISV 和手动解码)
- `arch/aarch64/exception.S` — 向量表、上下文保存/恢复、enter_guest

---

## 7. GICv3 中断控制器

### 7.1 功能说明

管理 GICv3 硬件, 用于虚拟机监控器操作 (物理中断) 和客户虚拟化 (通过 List Register 注入虚拟中断)。

### 7.2 关键类型

```rust
// src/arch/aarch64/peripherals/gicv3.rs

pub struct GicV3SystemRegs;          // ICC_* 寄存器访问 (EL2 物理)
pub struct GicV3VirtualInterface;    // ICH_* 寄存器访问 (虚拟注入)

pub const VTIMER_IRQ: u32 = 27;     // 虚拟定时器 PPI
pub const PTIMER_IRQ: u32 = 30;     // 物理定时器 PPI
```

### 7.3 List Register 布局 (64 位)

```
 63  62  61  60  59       48  47       32  31             0
 ┌───┬───┬───┬───────────────┬────────────┬────────────────┐
 │St │HW │Grp│   Priority    │   pINTID   │     vINTID     │
 │[1:0]│   │   │    [7:0]     │   [9:0]    │    [31:0]      │
 └───┴───┴───┴───────────────┴────────────┴────────────────┘

 字段       位域      值
 ─────────  ────────  ──────────────────────────────────────
 State      [63:62]   00=无效, 01=待处理,
                      10=活跃, 11=待处理+活跃
 HW         [61]      0=软件, 1=硬件关联
 Group      [60]      0=组0, 1=组1
 Priority   [55:48]   0x00=最高, 0xFF=最低
 pINTID     [41:32]   物理 INTID (HW=1 时有效)
 vINTID     [31:0]    客户可见的虚拟 INTID
```

### 7.4 HW=1 中断注入流程

```
 物理定时器触发 (INTID 27, IMO=1 陷入到 EL2)
      │
      ▼
 1. 虚拟机监控器确认: ICC_IAR1_EL1 → 27
      │
      ▼
 2. 屏蔽定时器: CNTV_CTL_EL0.IMASK = 1
      │
      ▼
 3. 找到空闲 LR, 写入:
    ┌────────────────────────────────────────────────────────┐
    │ State=01(待处理) │ HW=1 │ Grp=1 │ Prio=0xA0 │         │
    │ pINTID=27 │ vINTID=27                                  │
    └────────────────────────────────────────────────────────┘
      │
      ▼
 4. 优先级降级: ICC_EOIR1_EL1(27)
    (EOImode=1: EOIR 仅降级优先级, 不去激活)
      │
      ▼
 5. ERET → 客户恢复执行
      │
      ▼
 6. 客户看到待处理虚拟 IRQ (当 PSTATE.I=0 时)
      │
      ▼
 7. 客户 IRQ 处理程序运行, 读取 ICC_IAR1_EL1 → 27
      │                       (虚拟, 来自 ICH_LR)
      ▼
 8. 客户写入 ICC_EOIR1_EL1(27)  (虚拟 EOI)
      │
      ▼
 9. 硬件自动去激活物理 INTID 27
    因为 LR 中 HW=1 且 pINTID=27
      │
      ▼
 10. LR State → 无效 (可供重用)
```

### 7.5 EOImode=1 去激活流程

```
 ┌──────────────────────────────────────────────────────┐
 │                   EOImode=1                          │
 │  ICC_EOIR1_EL1 → 仅优先级降级                       │
 │  ICC_DIR_EL1   → 去激活 (显式)                       │
 │                                                      │
 │  对于 HW=1 中断 (定时器):                            │
 │    客户虚拟 EOI → 自动去激活物理中断                 │
 │    虚拟机监控器不调用 DIR                             │
 │                                                      │
 │  对于非 HW 中断:                                     │
 │    虚拟机监控器调用 EOIR (优先级降级)                │
 │    然后调用 DIR (去激活)                              │
 └──────────────────────────────────────────────────────┘
```

### 7.6 GIC 初始化序列

```
 gicv3::init()
    │
    ├─ 检查 ID_AA64PFR0_EL1[27:24] >= 1 (GICv3 可用?)
    │   └─ 否则: 回退到 gic::init() (GICv2)
    │
    ├─ ICC_SRE_EL2 = SRE(位 0) | Enable(位 3)
    │   └─ 在 EL2 启用系统寄存器接口
    │   └─ 允许 EL1 访问 ICC_* 寄存器
    │
    ├─ ICC_SRE_EL1 = SRE(位 0)
    │   └─ 在 EL1 启用系统寄存器接口
    │
    ├─ GicV3VirtualInterface::init()
    │   ├─ ICH_HCR_EL2 = 1 (En = 启用虚拟 GIC)
    │   ├─ ICH_VMCR_EL2 = VPMR(0xFF) | VENG1(1)
    │   └─ 清空所有 LR (写入 0)
    │
    ├─ ICC_CTLR_EL1 |= EOImode (位 1)
    │   └─ 分离优先级降级和去激活
    │
    └─ GicV3SystemRegs::enable()
        ├─ ICC_PMR_EL1 = 0xFF (允许所有优先级)
        └─ ICC_IGRPEN1_EL1 = 1 (启用组 1 中断)
```

### 7.7 公开 API (GicV3SystemRegs)

| 函数 | 描述 |
|------|------|
| `read_sre_el2() → u32` | 读取 ICC_SRE_EL2 |
| `write_sre_el2(u32)` | 写入 ICC_SRE_EL2 |
| `read_iar1() → u32` | 确认中断 (返回 INTID) |
| `write_eoir1(u32)` | 中断结束 (优先级降级) |
| `write_dir(u32)` | 去激活中断 (显式) |
| `read_ctlr() → u32` | 读取 ICC_CTLR_EL1 |
| `write_ctlr(u32)` | 写入 ICC_CTLR_EL1 |
| `write_pmr(u32)` | 设置优先级掩码 |
| `write_igrpen1(bool)` | 启用/禁用组 1 |
| `enable()` | PMR=0xFF, IGRPEN1=1 |
| `disable()` | IGRPEN1=0 |

### 7.8 公开 API (GicV3VirtualInterface)

| 函数 | 描述 |
|------|------|
| `read_hcr() → u32` | 读取 ICH_HCR_EL2 |
| `write_hcr(u32)` | 写入 ICH_HCR_EL2 |
| `read_lr(n) → u64` | 读取 List Register n (0-3) |
| `write_lr(n, u64)` | 写入 List Register n |
| `inject_interrupt(intid, priority) → Result` | 软件注入 (HW=0) |
| `inject_hw_interrupt(vintid, pintid, priority) → Result` | 硬件注入 (HW=1) |
| `clear_interrupt(intid)` | 清除匹配 INTID 的 LR |
| `pending_count() → usize` | 统计待处理 LR 数量 |
| `find_free_lr() → Option<usize>` | 查找无效状态的 LR |
| `num_list_registers() → u32` | 来自 ICH_VTR_EL2[4:0]+1 |

### 7.9 源文件

- `src/arch/aarch64/peripherals/gicv3.rs` — GICv3 系统寄存器接口, List Register 管理
- `src/arch/aarch64/peripherals/gic.rs` — GICv2 回退方案 (基于 MMIO 的 GICD/GICC)

---

## 8. 定时器虚拟化

### 8.1 功能说明

为客户提供 ARM 通用定时器 (PPI 27 虚拟定时器) 访问。通过 GICv3 List Register 将定时器中断作为虚拟中断注入。

### 8.2 CNTV_CTL_EL0 寄存器布局

```
 ┌──────────────────────────────────────────────┐
 │ 63                    3     2      1      0  │
 │  ───── 保留位 ──────  ISTATUS  IMASK  ENABLE │
 └──────────────────────────────────────────────┘
 ENABLE   [0]  = 1: 定时器启用
 IMASK    [1]  = 1: 中断被屏蔽 (抑制)
 ISTATUS  [2]  = 1: 定时器条件满足 (只读)

 定时器触发条件: ENABLE=1 && ISTATUS=1 && IMASK=0
 虚拟机监控器屏蔽方式: 设置 IMASK=1 停止连续触发
```

### 8.3 定时器中断生命周期

```
 ┌────────────┐
 │ 客户配置   │  CNTV_TVAL_EL0 = ticks
 │   定时器   │  CNTV_CTL_EL0 = ENABLE
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ 定时器     │  计数器到达比较值
 │ 触发       │  ISTATUS=1, 物理 IRQ 27 被断言
 └─────┬──────┘
       │ IMO=1 → 陷入到 EL2
       ▼
 ┌────────────┐
 │ 虚拟机监控 │  handle_irq_exception():
 │ 器屏蔽     │  CNTV_CTL_EL0.IMASK = 1
 │ 定时器     │  (停止连续触发)
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ 注入虚拟   │  ICH_LR: State=Pending, HW=1,
 │ 中断       │  vINTID=27, pINTID=27, Prio=0xA0
 └─────┬──────┘
       │ EOIR(27) = 优先级降级
       │ ERET → 客户
       ▼
 ┌────────────┐
 │ 客户 IRQ   │  客户看到待处理 vIRQ
 │ 处理程序   │  确认: ICC_IAR1 → 27
 │ 运行       │  处理定时器事件
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ 客户 EOI   │  ICC_EOIR1(27) → 虚拟 EOI
 │            │  HW=1 → 自动去激活物理中断
 └─────┬──────┘
       │
       ▼
 ┌────────────┐
 │ 客户重新   │  写入新的 CNTV_TVAL_EL0
 │ 配置定时器 │  清除 IMASK (CNTV_CTL = ENABLE)
 └────────────┘
       │
       └────────→ (循环重复)
```

### 8.4 定时器寄存器访问函数

| 函数 | 寄存器 | 描述 |
|------|--------|------|
| `get_frequency()` | `CNTFRQ_EL0` | 计数器频率 (Hz) |
| `get_counter()` | `CNTVCT_EL0` | 当前虚拟计数值 |
| `get_ctl()` / `set_ctl()` | `CNTV_CTL_EL0` | 定时器控制 |
| `get_cval()` / `set_cval()` | `CNTV_CVAL_EL0` | 比较值 |
| `get_tval()` / `set_tval()` | `CNTV_TVAL_EL0` | 倒计数值 |
| `is_guest_vtimer_pending()` | `CNTV_CTL_EL0` | ENABLE && ISTATUS && !IMASK |
| `mask_guest_vtimer()` | `CNTV_CTL_EL0` | 设置 IMASK 位 |
| `init_hypervisor_timer()` | `CNTHCTL_EL2` | 允许 EL1 访问计数器/定时器 |
| `init_guest_timer()` | `CNTHCTL_EL2`, `CNTVOFF_EL2` | 客户定时器访问, 偏移=0 |

### 8.5 源文件

- `src/arch/aarch64/peripherals/timer.rs` — 定时器寄存器访问、初始化、待处理检查

---

## 9. 设备模拟框架

### 9.1 功能说明

模拟客户通过 MMIO 交互的硬件设备。提供基于 trait 的可插拔设备模拟框架。

### 9.2 关键类型

```rust
// src/devices/mod.rs

pub trait MmioDevice {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64>;
    fn write(&mut self, offset: u64, value: u64, size: u8) -> bool;
    fn base_address(&self) -> u64;
    fn size(&self) -> u64;
    fn contains(&self, addr: u64) -> bool;   // 提供默认实现
}

pub struct DeviceManager {
    uart: pl011::VirtualUart,    // 0x0900_0000, 4KB
    gicd: gic::VirtualGicd,      // 0x0800_0000, 64KB
}
```

### 9.3 陷入-模拟数据流

```
 客户 @ EL1: str w0, [x1]   (x1 = 0x0900_0000 = UART DR)
      │
      │ Stage-2 翻译 → 数据中止 (MMIO 区域)
      ▼
 ┌──────────────────────────────────────────────────────────┐
 │  exception.S: exception_handler                          │
 │  保存上下文, 调用 handle_exception(context)              │
 └───────────────────────┬──────────────────────────────────┘
                         │ EC=0x24 (来自低异常级的数据中止)
                         ▼
 ┌──────────────────────────────────────────────────────────┐
 │  handle_mmio_abort(context, FAR_EL2=0x09000000)          │
 │                                                          │
 │  ISV=1?                                                  │
 │  ├─ 是: 从 ESR_EL2 ISS 解码                             │
 │  │   SAS[23:22]=大小, SRT[20:16]=寄存器, WnR[6]=方向    │
 │  └─ 否: 解码 context.pc 处的指令                         │
 │       └─ 对 ARM64 LDR/STR 编码进行模式匹配              │
 └───────────────────────┬──────────────────────────────────┘
                         │ MmioAccess::Store { reg=0, size=4 }
                         ▼
 ┌──────────────────────────────────────────────────────────┐
 │  global::DEVICES.handle_mmio(0x09000000, value, 4, true) │
 │                                                          │
 │  uart.contains(0x09000000)?  → 是                        │
 │  offset = 0x09000000 - 0x09000000 = 0x000 (UARTDR)      │
 │  uart.write(0x000, value, 4)                             │
 │    └─ output_char(value as u8)  → 物理 UART             │
 └───────────────────────┬──────────────────────────────────┘
                         │
                         ▼
 context.pc += 4         跳过故障指令
 return true             继续客户执行
```

### 9.4 MmioAccess 指令解码

```rust
// src/arch/aarch64/hypervisor/decode.rs

pub enum MmioAccess {
    Load { reg: u8, size: u8, sign_extend: bool },
    Store { reg: u8, size: u8 },
}

// ISV=1 路径 (ESR_EL2 ISS 字段):
//   SAS[23:22] → 大小: 00=1字节, 01=2字节, 10=4字节, 11=8字节
//   SRT[20:16] → 寄存器编号 (0-30)
//   WnR[6]     → 0=读(Load), 1=写(Store)
//   SSE[23]    → 符号扩展

// ISV=0 路径 (手动指令解码):
//   模式: (insn & 0x3B000000) == 0x39000000
//   → 带无符号立即偏移的 LDR/STR
//   大小来自 insn[31:30], Rt 来自 insn[4:0]
//   方向来自 insn[22]
```

### 9.5 PL011 虚拟 UART

```
 寄存器映射 (基地址: 0x0900_0000):

 偏移    名称        读写   描述
 ──────  ──────────  ──     ───────────────────────────────
 0x000   UARTDR      RW     数据: 写=TX 到物理串口, 读=从物理串口 RX
 0x018   UARTFR      R      标志: TXFE=1(始终就绪), RXFE=来自真实 UART
 0x024   UARTIBRD    RW     整数波特率 (存储, QEMU 忽略)
 0x028   UARTFBRD    RW     小数波特率 (存储, 忽略)
 0x02C   UARTLCR_H   RW     线路控制 (存储)
 0x030   UARTCR      RW     控制寄存器 (默认: 0x0301 = 启用)
 0x038   UARTIMSC    RW     中断掩码设置/清除
 0x03C   UARTRIS     R      原始中断状态
 0x040   UARTMIS     R      屏蔽后中断状态 (= RIS & IMSC)
 0x044   UARTICR     W      中断清除

 设计: 透传 — TX 写入直接到物理 UART,
 RX 读取来自物理 UART。无缓冲。
```

### 9.6 虚拟 GIC 分发器

```
 寄存器映射 (基地址: 0x0800_0000):

 偏移      名称            读写  描述
 ────────  ──────────────  ──    ──────────────────────────
 0x000     GICD_CTLR       RW    分发器控制
 0x004     GICD_TYPER       R     类型: ITLinesNumber=31 (1024 个 IRQ), CPU=1
 0x100-    GICD_ISENABLER  RW    设置使能 (32 个寄存器 x 32 位 = 1024 个 IRQ)
 0x17F
 0x180-    GICD_ICENABLER  RW    清除使能 (相同布局)
 0x1FF
 *         (所有其他)       -     RAZ/WI (读零/写忽略)

 内部状态:
   ctlr: u32           (启用/禁用分发器)
   enabled: [u32; 32]  (1024 个中断使能位)
```

### 9.7 全局设备访问

```rust
// src/global.rs

pub struct GlobalDeviceManager {
    devices: UnsafeCell<Option<DeviceManager>>,
}

pub static DEVICES: GlobalDeviceManager = GlobalDeviceManager::new();

// 异常处理器访问 (不需要 &mut self):
//   global::DEVICES.handle_mmio(addr, value, size, is_write)
//
// 安全性: 同一时间只有一个 vCPU 运行 → 实质上是单线程的。
```

### 9.8 源文件

- `src/devices/mod.rs` — MmioDevice trait, DeviceManager 路由器
- `src/devices/pl011/emulator.rs` — VirtualUart (透传 UART)
- `src/devices/gic/distributor.rs` — VirtualGicd (中断使能/禁用)
- `src/global.rs` — GlobalDeviceManager (异常处理器访问)

---

## 10. 客户系统启动

### 10.1 功能说明

加载并启动真实操作系统 (Linux, Zephyr) 作为客户。处理 ELF 解析、ARM64 Image 头部解析、EL1 寄存器初始化和 Linux 启动协议。

### 10.2 关键类型

```rust
// src/guest_loader.rs

pub enum GuestType {
    Zephyr,    // Zephyr RTOS (ELF 或原始二进制)
    Linux,     // Linux 内核 (ARM64 Image 格式)
}

pub struct GuestConfig {
    pub guest_type: GuestType,
    pub load_addr: u64,        // QEMU 加载内核的位置
    pub mem_size: u64,         // 客户 RAM 大小
    pub entry_point: u64,      // 内核入口地址
    pub dtb_addr: u64,         // 设备树地址
}
```

### 10.3 客户启动流程

```
 run_guest(config)
      │
      ├─ 创建 VM: Vm::new(0)
      │   └─ 安装全局 DeviceManager
      │
      ├─ 初始化内存: vm.init_memory(load_addr, mem_size)
      │   └─ Stage-2 页表, HCR_EL2.VM=1
      │
      ├─ 创建 vCPU: vm.create_vcpu(0)
      │   ├─ 设置 PC = entry_point
      │   ├─ 设置 SP = load_addr + mem_size - 0x1000
      │   └─ [Linux] x0 = dtb_addr, x1-x3 = 0
      │
      ├─ 初始化客户定时器: init_guest_timer()
      │   ├─ CNTHCTL_EL2 |= EL1PCTEN
      │   └─ CNTVOFF_EL2 = 0
      │
      ├─ [Linux] 初始化 EL1 系统寄存器:
      │   ├─ SCTLR_EL1 = 0x30D0_0800 (RES1, MMU 关闭)
      │   ├─ 清零: TCR, TTBR0, TTBR1, MAIR, VBAR
      │   ├─ CPACR_EL1 = 3 << 20 (FP/SIMD 启用)
      │   ├─ CPTR_EL2: 清除 TZ, TFP, TSM, TCPAC
      │   ├─ MDCR_EL2 = 0
      │   ├─ VPIDR_EL2 = MIDR_EL1 (真实 CPU ID)
      │   └─ VMPIDR_EL2 = MPIDR_EL1
      │
      ├─ [Linux] 清除 HCR_EL2 中的 TWI/TWE
      │   └─ 客户自行处理 WFI (不陷入)
      │
      ├─ [Linux] 重置异常计数器
      │
      └─ vm.run()
          └─ vcpu.run() → enter_guest() → ERET 到客户
```

### 10.4 Linux ARM64 Image 头部解析

```
 偏移    大小  字段
 ──────  ────  ──────────────────
 0x00      4   code0 (跳转指令)
 0x04      4   code1
 0x08      8   text_offset (相对加载地址的偏移)
 0x10      8   image_size
 0x18      8   flags
 0x20      8   res2
 0x28      8   res3
 0x30      8   res4
 0x38      4   magic (0x644d5241 = "ARMd" 小端)
 0x3C      4   res5

 入口点 = kernel_addr + text_offset
 (如果 text_offset 非零且 < 0x100000)
```

### 10.5 HCR_EL2 配置差异

| 特性 | 测试客户 | Linux 客户 |
|------|---------|-----------|
| TWI (陷入 WFI) | 设置 | **清除** |
| TWE (陷入 WFE) | 设置 | **清除** |
| VM (Stage-2) | init_memory 时设置 | init_memory 时设置 |
| APK/API (PAC) | 设置 | 设置 |
| IMO/FMO/AMO | 设置 | 设置 |

### 10.6 源文件

- `src/guest_loader.rs` — GuestConfig, linux_default, zephyr_default, run_guest

---

## 11. 架构抽象层

### 11.1 功能说明

定义可移植的 trait 来抽象硬件特定操作, 为未来支持其他架构 (如 RISC-V) 提供可能。

### 11.2 Trait 定义

```rust
// src/arch/traits.rs

pub trait InterruptController {
    fn init(&mut self);
    fn enable(&mut self);
    fn disable(&mut self);
    fn acknowledge(&mut self) -> u32;
    fn eoi(&mut self, intid: u32);
    fn deactivate(&mut self, intid: u32);
    fn set_priority_mask(&mut self, mask: u8);
}

pub trait VirtualInterruptController {
    fn inject_interrupt(&mut self, intid: u32, priority: u8) -> Result<(), &'static str>;
    fn inject_hw_interrupt(&mut self, vintid: u32, pintid: u32, priority: u8)
        -> Result<(), &'static str>;
    fn clear_interrupt(&mut self, intid: u32);
    fn pending_count(&self) -> usize;
}

pub trait GuestTimer {
    fn init_hypervisor(&mut self);
    fn init_guest(&mut self);
    fn is_pending(&self) -> bool;
    fn mask(&mut self);
    fn get_frequency(&self) -> u64;
    fn get_counter(&self) -> u64;
}

pub trait Stage2Mapper {
    fn map_region(&mut self, ipa: u64, size: u64, mem_type: MemoryType)
        -> Result<(), &'static str>;
    fn reset(&mut self);
    fn install(&self);
    fn root_table_addr(&self) -> u64;
}

pub trait VcpuContextOps {
    fn new(entry: u64, sp: u64) -> Self;
    fn pc(&self) -> u64;
    fn set_pc(&mut self, val: u64);
    fn sp(&self) -> u64;
    fn set_sp(&mut self, val: u64);
    fn get_reg(&self, n: u8) -> u64;
    fn set_reg(&mut self, n: u8, val: u64);
    fn advance_pc(&mut self);
}

pub trait ExceptionInfo {
    fn is_wfi(&self) -> bool;
    fn is_hypercall(&self) -> bool;
    fn is_data_abort(&self) -> bool;
    fn is_instruction_abort(&self) -> bool;
    fn fault_address(&self) -> Option<u64>;
}

pub enum MemoryType { Normal, Device, ReadOnly }
```

### 11.3 Trait 实现

| Trait | 实现者 | 位置 |
|-------|--------|------|
| `VcpuContextOps` | `VcpuContext` | `src/arch/aarch64/regs.rs` |
| `ExceptionInfo` | `ExitReason` | `src/arch/aarch64/regs.rs` |
| `Stage2Mapper` | `DynamicIdentityMapper` | `src/arch/aarch64/mm/mmu.rs` |

### 11.4 平台常量

```rust
// src/platform.rs

pub const UART_BASE: usize = 0x0900_0000;
pub const UART_SIZE: u64   = 0x1000;

pub const GICD_BASE: u64   = 0x0800_0000;
pub const GICD_SIZE: u64   = 0x1_0000;
pub const GICC_BASE: u64   = 0x0801_0000;
pub const GIC_REGION_BASE: u64 = 0x0800_0000;
pub const GIC_REGION_SIZE: u64 = 8 * BLOCK_SIZE_2MB;   // 16MB

pub const GUEST_RAM_BASE: u64  = 0x4000_0000;
pub const GUEST_LOAD_ADDR: u64 = 0x4800_0000;
pub const LINUX_DTB_ADDR: u64  = 0x4700_0000;
pub const LINUX_MEM_SIZE: u64  = 512 * 1024 * 1024;    // 512MB
pub const ZEPHYR_MEM_SIZE: u64 = 128 * 1024 * 1024;    // 128MB
pub const GUEST_STACK_RESERVE: u64 = 0x1000;

pub const HEAP_START: u64 = 0x4100_0000;
pub const HEAP_SIZE: u64  = 0x100_0000;                 // 16MB
```

### 11.5 命名常量 (defs.rs)

| 类别 | 常量 |
|------|------|
| HCR_EL2 | `VM`, `SWIO`, `FMO`, `IMO`, `AMO`, `FB`, `BSU_INNER`, `TWI`, `TWE`, `RW`, `APK`, `API` |
| ESR_EL2 | `EC_SHIFT`, `EC_MASK`, `ISS_MASK`, `HVC_IMM_MASK` |
| 异常类型 | `EC_UNKNOWN`, `EC_WFI_WFE`, `EC_TRAPPED_SIMD_FP`, `EC_TRAPPED_SVE`, `EC_HVC64`, `EC_MSR_MRS`, `EC_SVE_TRAP`, `EC_IABT_LOWER`, `EC_IABT_SAME`, `EC_DABT_LOWER`, `EC_DABT_SAME` |
| SPSR_EL2 | `SPSR_EL1H_DAIF_MASKED` (0x3C5), `SPSR_EL1H` (0b0101) |
| CPTR_EL2 | `CPTR_TZ`, `CPTR_TFP`, `CPTR_TSM`, `CPTR_TCPAC` |
| ICC 寄存器 | `ICC_SRE_SRE`, `ICC_SRE_ENABLE`, `ICC_CTLR_EOIMODE`, `ICC_PMR_ALLOW_ALL` |
| GICv3 LR 字段 | `LR_STATE_SHIFT`, `LR_STATE_MASK`, `LR_HW_BIT`, `LR_GROUP1_BIT`, `LR_PRIORITY_SHIFT`, `LR_PINTID_SHIFT`, `LR_PINTID_MASK`, `LR_VINTID_MASK`, `VTR_LISTREGS_MASK`, `GIC_SPURIOUS_INTID` |
| IRQ 优先级 | `IRQ_DEFAULT_PRIORITY` (0xA0) |
| VTCR_EL2 | `T0SZ_48BIT`, `SL0_LEVEL0`, `IRGN0_WB`, `ORGN0_WB`, `SH0_INNER`, `TG0_4KB`, `PS_48BIT` |
| CNTHCTL_EL2 | `EL1PCTEN`, `EL1PCEN` |
| 页表 | `PTE_VALID`, `PTE_TABLE`, `PTE_ADDR_MASK`, `PAGE_OFFSET_MASK`, `PT_INDEX_MASK`, `BLOCK_SIZE_2MB`, `BLOCK_MASK_2MB` |
| 指令 | `AARCH64_INSN_SIZE` (4) |

### 11.6 源文件

- `src/arch/traits.rs` — 可移植 trait 定义
- `src/arch/aarch64/defs.rs` — 命名常量
- `src/platform.rs` — QEMU virt 平台地址和大小

---

## 附录

### 附录 A: HCR_EL2 位域参考

```
 位    名称   设置?  用途
 ────  ─────  ────   ────────────────────────────────────────────
   0   VM      *     启用 Stage-2 翻译
                     (* 由 init_stage2 设置, 非 exception::init)
   1   SWIO    是    Set/Way 无效化覆盖
   3   FMO     是    将物理 FIQ 路由到 EL2
   4   IMO     是    将物理 IRQ 路由到 EL2
   5   AMO     是    将物理 SError 路由到 EL2
   6   VF      -     虚拟 FIQ 待处理 (传统模式, GICv3 下清除)
   7   VI      -     虚拟 IRQ 待处理 (传统模式, GICv3 下清除)
   9   FB      是    强制广播 TLB/缓存维护
  10   BSU     是    屏障共享性升级 = 内部共享
  12   DC      否    默认可缓存性 — 未设置 (会导致过时 PTE 错误)
  13   TWI     是*   陷入 WFI (*Linux 客户下清除)
  14   TWE     是*   陷入 WFE (*Linux 客户下清除)
  31   RW      是    EL1 为 AArch64
  40   APK     是    不陷入 PAC 密钥寄存器
  41   API     是    不陷入 PAC 指令
```

### 附录 B: ESR_EL2 异常类型速查表

```
 EC     名称                描述                           处理方式
 ─────  ──────────────────  ─────────────────────────────  ─────────────
 0x00   EC_UNKNOWN          未知/未分类                    记录, 退出
 0x01   EC_WFI_WFE          WFI/WFE 被陷入                定时器注入
 0x07   EC_TRAPPED_SIMD_FP  FP/SIMD 访问被陷入            跳过指令
 0x09   EC_TRAPPED_SVE      SVE/SME 访问被陷入             跳过指令
 0x16   EC_HVC64            HVC 指令 (AArch64)             PSCI / 自定义
 0x18   EC_MSR_MRS          系统寄存器被陷入               模拟读/写
 0x19   EC_SVE_TRAP         SVE 被陷入 (CPTR_EL2.TZ)       跳过指令
 0x20   EC_IABT_LOWER       来自 EL1 的指令中止            转储, 退出
 0x21   EC_IABT_SAME        来自 EL2 的指令中止            转储, 退出
 0x24   EC_DABT_LOWER       来自 EL1 的数据中止            MMIO 模拟
 0x25   EC_DABT_SAME        来自 EL2 的数据中止            转储, 退出
```

### 附录 C: List Register 编码参考

```
 位域      字段          描述
 ────────  ────────────  ──────────────────────────────────────────
 [63:62]   State         00=无效 01=待处理 10=活跃 11=待处理+活跃
 [61]      HW            1=硬件关联 (pINTID 有效)
 [60]      Group         0=组 0, 1=组 1
 [59:56]   (保留)
 [55:48]   Priority      0x00=最高, 0xFF=最低
 [47:42]   (保留)
 [41:32]   pINTID        物理 INTID (仅 HW=1 时有效)
 [31:0]    vINTID        客户可见的虚拟 INTID

 常用值:
   软件注入:  State=01 | Group=1 | Prio=0xA0 | vINTID
   硬件注入:  State=01 | HW=1 | Group=1 | Prio=0xA0 | pINTID | vINTID
```

### 附录 D: 构建系统

```
 构建目标 (Makefile):
   make              构建虚拟机监控器 (cargo build --target aarch64-unknown-none)
   make run          在 QEMU 中运行 (仅测试, 无客户)
   make run-guest    运行 Zephyr ELF  (feature: guest)
   make run-linux    运行 Linux Image (feature: linux_guest)
   make debug        运行并在端口 1234 启动 GDB 服务器
   make clippy       运行代码检查
   make fmt          格式化代码

 QEMU 配置:
   qemu-system-aarch64
     -machine virt,virtualization=on,gic-version=3
     -cpu max
     -smp 1
     -m 1G
     -nographic
     -kernel target/aarch64-unknown-none/debug/hypervisor

 Feature 标志 (Cargo.toml):
   default      = []           无客户 (仅测试)
   guest        = []           启用 Zephyr 客户
   linux_guest  = []           启用 Linux 客户

 构建流水线 (build.rs):
   1. aarch64-linux-gnu-gcc -c boot.S → boot.o
   2. aarch64-linux-gnu-gcc -c exception.S → exception.o
   3. aarch64-linux-gnu-ar crs libboot.a boot.o exception.o
   4. 使用 --whole-archive 链接 (包含所有汇编符号)
```
