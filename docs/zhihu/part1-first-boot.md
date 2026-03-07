# 从零到 "Hello from EL2!" — 四个文件启动一个 Hypervisor

## 写在前面

上两篇讲了为什么做这个项目，以及 AI 辅助系统编程的工作流。从这篇开始进入技术。

这是整个系列里最短的一篇——目标也最简单：在 QEMU 模拟器的串口上打印一句 "Hello from EL2!"。但别小看这件事。为了这一行输出，我们要解决几个在应用层开发中完全不存在的问题：

- 没有操作系统，没有运行时，没有 `main()` 函数签名
- 需要手写 ARM64 汇编设置栈和入口
- 需要自己决定二进制文件加载到内存的哪个地址
- 需要搞定交叉编译工具链（开发机是 x86_64，目标是 ARM64）

四个文件，两个 commit，一个晚上。其中第一个 commit 不能跑，第二个 commit 才对。这个调试过程本身就是裸机开发最真实的体验。

---

## ARM64 特权级——为什么是 EL2

ARM64 定义了四个特权级别（Exception Level），数字越大权限越高：

```
EL3  ──  安全监控器（Secure Monitor）—— 固件，TrustZone 的守门人
EL2  ──  Hypervisor —— 我们的代码跑在这里
EL1  ──  操作系统内核 —— Linux, guest
EL0  ──  用户态应用
```

Hypervisor 必须跑在 EL2，因为 EL2 提供了硬件虚拟化所需的全部机制：

1. **Stage-2 地址翻译**。guest 以为自己在访问物理地址 A，硬件自动把 A 翻译成真正的物理地址 B。hypervisor 通过配置翻译表来控制 guest 能看到什么内存。这是由 `VTTBR_EL2`（翻译表基地址寄存器）和 `VTCR_EL2`（翻译控制寄存器）驱动的，只有 EL2 能访问。

2. **陷阱配置**。`HCR_EL2` 是一个 64 位寄存器，每个 bit 控制一种 guest 操作是否陷入 EL2 处理。想拦截 guest 的 WFI（等待中断）？置 TWI 位。想拦截 SMC 调用？置 TSC 位。想拦截系统寄存器访问？也有对应的位。

3. **虚拟中断注入**。GICv3 的虚拟接口让 hypervisor 直接向 guest 注入中断，不需要修改 guest 的内部状态。

4. **VMID 标签的 TLB**。`VTTBR_EL2` 的高位可以编码 VM ID，硬件 TLB 会自动区分不同虚拟机的翻译缓存——切换 VM 时不用刷整个 TLB。

如果试图在 EL1 实现这些功能，全部需要软件模拟，性能差几个数量级。EL2 是硬件专门为 hypervisor 设计的。

### QEMU 怎么把我们送到 EL2

在真实硬件上，EL3 固件（比如 ARM Trusted Firmware）会配置好 CPU，然后降级到 EL2 把控制权交给 hypervisor。但在这个阶段，我们用 QEMU 的快捷方式跳过这些复杂度：

```bash
qemu-system-aarch64 \
  -machine virt,virtualization=on \
  -cpu max \
  -nographic \
  -kernel hypervisor.bin
```

关键是 `-machine virt,virtualization=on`。没有 `virtualization=on`，QEMU 默认从 EL1 开始执行内核。加了这个参数，QEMU 的内置固件会配置好 CPU，直接在 EL2 入口执行我们的二进制。

---

## 四个文件

整个 "Milestone 0" 只需要四个手写文件。其他的全由工具链生成。

### 1. boot.S — 汇编入口

CPU 上电后开始执行的第一段代码。在任何 Rust 代码跑之前，我们需要一个栈和清零的 BSS 段：

```armasm
.section .text.boot
.global _start

_start:
    // 检查 CPU ID——只有 CPU 0 继续，其他 halt
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF
    cbnz    x19, halt

    // 保存 QEMU 传入的 DTB 地址（x0）
    mov     x20, x0

    // 设置栈（128KB，向下增长）
    adrp    x0, stack_top
    add     x0, x0, :lo12:stack_top
    mov     sp, x0

    // 清零 BSS 段
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
clear_bss:
    cmp     x0, x1
    b.ge    clear_bss_done
    str     xzr, [x0], #8
    b       clear_bss
clear_bss_done:

    // 跳转到 Rust
    mov     x0, x20
    bl      rust_main

halt:
    wfe
    b       halt
```

三件事：

**栈设置。** `adrp`+`add` 加载 `stack_top` 的地址（定义在 BSS 段里），栈向下增长，所以 SP 指向栈顶。ARM64 要求 16 字节栈对齐。

**BSS 清零。** Rust 的 `static` 变量和未初始化的全局变量都在 BSS 段。ELF loader 不会帮我们清零（因为根本没有 loader——QEMU 直接把裸二进制塞进内存）。我们用 `str xzr` 每次写 8 个零字节，循环清完整个 BSS。

**跳转到 Rust。** `bl rust_main` 调用 Rust 的入口函数。如果它返回了（不应该），就掉进 `halt` 循环，执行 WFE（Wait For Event）空转。

一个细节值得注意：QEMU 在启动时通过 x0 传入设备树（DTB）的地址。我们在做栈设置之前把它保存到 x20——x20 是 ARM64 调用规范里的 callee-saved 寄存器，不会被后续的函数调用覆盖。这个 DTB 地址后面解析硬件信息要用。

### 2. linker.ld — 内存布局

```ld
ENTRY(_start)

SECTIONS
{
    . = 0x40000000;   /* QEMU virt: RAM 从这里开始 */

    .text : {
        KEEP(*(.text.boot))   /* boot.S 必须排第一! */
        *(.text .text.*)
    }

    .rodata : { *(.rodata .rodata.*) }
    .data   : { *(.data .data.*) }

    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(.bss.stack)
        *(COMMON)
        __bss_end = .;
    }

    /DISCARD/ : { *(.comment) *(.eh_frame) }
}
```

三个关键决定：

**基地址 `0x40000000`。** QEMU `virt` 机器的 RAM 从 0x40000000 开始。用 `-kernel` 启动时，QEMU 把二进制加载到 RAM 起始位置。基地址写错的后果——后面调试部分会讲。

（注：后来引入 TF-A 安全引导链后，基地址改成了 `0x40200000`，因为 TF-A 的 `-bios` 模式会在 0x40000000 放一个设备树。但在 Milestone 0 阶段，0x40000000 是对的。）

**`KEEP(*(.text.boot))`。** 确保 boot.S 里的 `_start` 是二进制里最先出现的代码。没有 `KEEP`，链接器可能把它当"没人引用"的代码丢掉。没有放在最前面，CPU 上电后执行的就是某个随机函数——在裸机环境下，这意味着安静挂起。

**BSS 包含栈。** 16KB 的栈分配在 `.bss.stack` 里，在 BSS 范围内。所以 BSS 清零循环会同时清零栈，这没问题——栈本来就是未初始化的。

### 3. rust_main() — 第一行 Rust

```rust
#![no_std]
#![no_main]

use core::panic::PanicInfo;

fn uart_puts(s: &[u8]) {
    unsafe {
        let uart_base = 0x09000000usize;
        for &byte in s {
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) byte as u32,
                options(nostack),
            );
        }
    }
}

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    uart_puts(b"Hello from EL2!\n");
    loop { unsafe { core::arch::asm!("wfe"); } }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop { unsafe { core::arch::asm!("wfe"); } }
}
```

几个要点：

**`#![no_std]` + `#![no_main]`。** 无标准库，无 `fn main()`。这是一个 freestanding 二进制。

**`uart_puts` 用内联汇编。** QEMU `virt` 机器的 PL011 UART 在 `0x09000000`。往偏移 0x00（数据寄存器）写一个字节就能发到串口。我们用 `str`（store register）指令直接写。`{val:w}` 用 32 位的 `w` 寄存器变体，因为 PL011 期望 32 位写入。

为什么不用 Rust 的 `core::ptr::write_volatile`？其实也行，但内联汇编在这个阶段更直接——零依赖，零意外。

**`#[no_mangle] pub extern "C"`。** 这样 `boot.S` 才能通过 `bl rust_main` 调用。没有 `no_mangle`，Rust 会把符号名修饰（mangle）成一个人和汇编都认不出的东西。没有 `extern "C"`，调用约定可能不匹配。

**`-> !`（永不返回）。** `rust_main` 最后是死循环。如果它不知怎么返回了，`boot.S` 的 `halt` 循环兜底。

### 4. build.rs — 汇编和 Rust 的桥梁

```rust
fn main() {
    // 用 aarch64-linux-gnu-gcc 交叉编译 boot.S
    Command::new("aarch64-linux-gnu-gcc")
        .args(&["-c", "arch/aarch64/boot.S", "-o", /* ... */,
                "-nostdlib", "-ffreestanding"])
        .status().expect("Failed to compile boot.S");

    // 打包成 libboot.a
    Command::new("aarch64-linux-gnu-ar")
        .args(&["crs", /* libboot.a */, /* boot.o */])
        .status().expect("Failed to create archive");

    // 告诉 Cargo 链接它
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=boot");
}
```

Cargo 不原生支持汇编文件。`build.rs` 是桥梁：

1. `aarch64-linux-gnu-gcc` 把 `boot.S` 编译成目标文件
2. `aarch64-linux-gnu-ar` 打包成静态库 `libboot.a`
3. Cargo 链接时把 `libboot.a` 和 Rust 代码合在一起

`_start` 符号通过链接脚本的 `ENTRY(_start)` 成为 ELF 入口点。整个链条：gcc 编译汇编 → ar 打包 → Cargo 链接 → QEMU 加载。

---

## "测试"：串口输出就是断言

这个阶段没有测试框架，没有断言，没有 CI。测试就是：`make run` 之后串口上有没有输出。

```
$ make run
Starting QEMU...
========================================
  ARM64 Hypervisor - Milestone 0
========================================

Hello from EL2!

System Information:
  - Exception Level: EL2 (Hypervisor)
  - Architecture: AArch64
  - Target: QEMU virt machine

Project initialized successfully!
========================================
```

如果你看到这段文字，说明整条链路全通了：gcc 编译了汇编，build.rs 打包了静态库，Cargo 交叉编译了 Rust，链接器把 `_start` 放在了加载地址，QEMU 加载了二进制在 EL2 开始执行，汇编设置了栈并跳到了 Rust，Rust 用内联汇编往 UART 写了字节，QEMU 的 PL011 模拟把它们转发到了你的终端。

如果你什么都没看到——沉默。没有错误信息，没有崩溃转储。只有一个空白的终端。

**这就是裸机调试的现实：失败就是什么都没有。**

这种"printf 调试"看着原始，但它在整个项目后期依然有用。当 EL2 出了问题，测试框架本身可能也坏了的时候，`uart_puts(b"GOT HERE\n")` 有时候是唯一能用的工具。

---

## 调试：两个 Commit 之间发生了什么

第一个 commit（`609459b`）——项目脚手架和启动代码。第二个 commit（`b2ff49f`）——修复。中间出了什么问题？

### Bug 1：加载地址写错了

初始的链接脚本把基地址设成了 `0x80000000`：

```ld
. = 0x80000000;   /* "EL2 通常用这个地址" —— 错 */
```

QEMU `virt` 机器的 RAM 从 `0x40000000` 开始。用 `-kernel` 时，QEMU 把二进制加载到 RAM 开头。我们的二进制以为自己在 0x80000000，实际在 0x40000000——每一条 `adr` 指令算出的地址都是错的。栈指向了不存在的内存，BSS 清零在破坏随机区域。

**现象**：串口没有任何输出。沉默。

**修复**：把基地址改成 `0x40000000`。

**教训**：链接脚本的基地址必须和二进制实际被加载的位置一致。用 `-kernel` 时，QEMU 加载到 RAM 起始位置。这听起来很显然，但在裸机环境下，"显然错"和"正确"产生的外部现象是一样的——沉默。

### Bug 2：UART 驱动写复杂了

第一个 commit 包含了一个完整的 PL011 UART 驱动（`src/uart.rs`，112 行）：
- `Uart` 结构体，`read_reg`/`write_reg` 用 `read_volatile`/`write_volatile`
- TX FIFO 满检测（忙等 `UART_FR.TXFF`）
- 实现了 `fmt::Write` trait
- `print!`/`println!` 宏

链接通过了，但没有输出。问题：`core::fmt` 的格式化机制在 `no_std` + 我们的自定义 target 下触发了额外的代码生成，而这些代码在我们极简的运行时下没法正常工作。

**修复**：把 112 行的驱动替换成 10 行内联汇编。

```rust
fn uart_puts(s: &[u8]) {
    unsafe {
        let uart_base = 0x09000000usize;
        for &byte in s {
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) byte as u32,
                options(nostack),
            );
        }
    }
}
```

没有 volatile 读，没有 FIFO 检测，没有格式化 trait。直接 `str` 到 UART 数据寄存器。QEMU 的 PL011 模拟在合理的输出速率下永远不会报 "FIFO 满"，所以忙等是多余的。

**教训**：裸机开发的第一法则——从能跑的最小方案开始。先证明最简单的方式能工作，再加复杂度。那个精致的 UART 驱动本身是正确的代码，但对于一个连栈都刚设好的系统来说，太多了。

### AI 在这里的表现

这个 milestone 是 AI 协作工作流还在磨合的阶段。第一个 commit 基本是 Claude 生成的：项目结构、Makefile、boot.S、UART 驱动、自定义 target spec。所有的零件都在——但组装出来不能跑。

加载地址写错，是因为 AI 用了一个"通用的 EL2 地址"，没有去查 QEMU `virt` 机器的具体内存布局。UART 驱动过度工程，是因为 AI 默认你要"完整的"串口支持，而不是"能发一个字节就行"。

这个模式在后续的项目里反复出现：**AI 生成一个全面的初稿，人来调试集成问题。** AI 对 "PL011 UART 驱动怎么写" 的知识是扎实的，但对 "QEMU 把二进制加载到哪里" 这种平台细节需要验证。架构知识 OK，平台细节不靠谱。

还有一个反直觉的经验：当你面对沉默（什么输出都没有），**AI 倾向于在下游找问题**——"UART 的地址写错了吗？"、"格式化逻辑有 bug 吗？"——而真正的问题在上游：二进制根本没有被加载到正确的位置。裸机 bug 的根因经常在比你正在看的代码更早的地方。

---

## 精简版：整个链路一图看完

```
aarch64-linux-gnu-gcc  ─── 编译 ──→  boot.o
         │
aarch64-linux-gnu-ar   ─── 打包 ──→  libboot.a
         │
cargo build (build.rs)  ─── 链接 ──→  hypervisor (ELF)
         │
qemu-system-aarch64     ─── 加载 ──→  内存 @ 0x40000000
         │
CPU 上电 @ EL2  ──→  _start  ──→  设置栈  ──→  清零 BSS  ──→  bl rust_main
         │
rust_main()  ──→  uart_puts("Hello from EL2!")  ──→  str 到 0x09000000
         │
QEMU PL011 模拟  ──→  串口转发到 stdout  ──→  你看到了输出
```

从源码到屏幕上的一行字，经过了两个编译器、一个链接器、一个模拟器、一段汇编、一段 Rust、一个硬件寄存器地址。任何一个环节出错，结果都是同一个——沉默。

---

## 小结

这一篇做了几件事：

1. **搞清楚了 ARM64 特权级模型**——EL2 是 hypervisor 的专属级别，提供 Stage-2 翻译、陷阱配置、虚拟中断注入
2. **用四个文件搭出了最小可运行的裸机 Rust 程序**——boot.S、linker.ld、rust_main、build.rs
3. **踩了两个典型的裸机坑**——加载地址和过度工程
4. **建立了一条从代码到输出的端到端链路**——后续所有功能都建立在这条链路上

目前我们的 hypervisor 只能打印一行字然后死循环。但这一行字证明了：**工具链通了，EL2 进去了，Rust 在裸机上跑了。**

下一篇：Part 2 — vCPU 抽象。怎么让虚拟 CPU 跑起来，怎么进入 guest 再回来。

---

*不免疏漏之处，欢迎各位朋友交流指正。*

*项目地址：[GitHub](https://github.com/phasewalk1/hypervisor)（待发布）*
