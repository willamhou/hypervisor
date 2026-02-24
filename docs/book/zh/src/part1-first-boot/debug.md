# 调试笔记：哪里出了问题

两个 commit 才拿到 "Hello from EL2!" —— 脚手架 commit（`609459b`）和修复 commit（`b2ff49f`）。中间出了什么问题？

## Bug 1：加载地址错误

初始链接脚本把基地址设成了 `0x80000000`：

```ld
. = 0x80000000;   /* "EL2 典型地址" — 错了 */
```

QEMU 的 `virt` 机器把 RAM 放在 `0x40000000`。用 `-kernel` 时，QEMU 把二进制加载到 RAM 起始位置。我们的二进制以为自己在 0x80000000，实际在 0x40000000——每条 `adr` 指令都算出了错误的地址。栈设置指向了垃圾内存。BSS 清零破坏了随机区域。

**修复**：把链接基地址改成 `0x40000000`。

```ld
. = 0x40000000;   /* QEMU virt: RAM 从这里开始 */
```

**教训**：链接脚本的基地址必须和二进制实际被加载的位置匹配。用 `-kernel` 的话，QEMU 加载到 RAM 起始地址。这事后看很明显，但在调试裸机代码时，"明显错误"和"正确"产生同样的症状：沉默。

## Bug 2：过度工程化的 UART 驱动

第一个 commit 包含了一个完整的 PL011 UART 驱动（`src/uart.rs`，112 行）：
- `Uart` 结构体，通过 `read_volatile`/`write_volatile` 的 `read_reg`/`write_reg`
- TX FIFO 满检查（忙等 `UART_FR.TXFF`）
- `fmt::Write` trait 实现
- `print!`/`println!` 宏

能链接，但没有输出。问题是：`core::fmt` 机制在 `no_std` + 我们的自定义 target 下触发了代码生成，在我们这个最小运行时上无法工作。`println!` 宏展开成复杂的格式化代码，在没有合适的内存模型之前根本跑不起来。

**修复**：把整个 UART 驱动替换成 10 行内联汇编：

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

没有 volatile 读取，没有 FIFO 检查，没有格式化 trait。就是 `str` 到 UART 数据寄存器。QEMU 的 PL011 在合理的输出速率下永远不会说 "FIFO 满"，所以忙等是多余的。

**教训**：在裸机环境中，从能工作的最小版本开始。只在简单版本能跑之后再加复杂度。那个完善的 UART 驱动代码是正确的，但对一个连栈都还没设好的系统来说太多了。

## Bug 3：EL2 检查被移除

原始的 `boot.S` 验证了 `CurrentEL == 2`，如果不是就跳到 `halt`。在修复 commit 中被移除了。为什么？

检查本身没问题。但它在真正的工作（栈设置）之前加了 4 条指令，而且更重要的是，它不可测试——如果我们不在 EL2，没有 UART 输出来告诉我们。`halt` 循环和"正常工作"的循环从外面看起来一模一样：串口控制台上什么都没有。

**教训**：当失败模式和成功模式无法区分时，不要加运行时检查。改用文档和 QEMU 参数（`virtualization=on`）来验证假设。

## AI 协作故事

这个里程碑是 AI 结对编程工作流还在磨合的阶段。第一个 commit 大部分由 Claude 生成：项目结构、Makefile、boot.S、UART 驱动、自定义 target spec。是一个合理的起点——所有正确的部件都在。

但它不能工作。加载地址错了，UART 驱动过度工程化了。修复 commit 展示了调试过程：把一切精简到最小，一次修一个东西。

这个模式在整个项目中反复出现：AI 生成一个全面的初稿，人类调试集成问题。AI 擅长"PL011 UART 驱动是这样工作的"，但搞错了"QEMU 把二进制加载到哪里"。架构知识是扎实的；平台特定的细节需要人工验证。
