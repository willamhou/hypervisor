# 实现：最初的 4 个文件

第一个可工作的 hypervisor 只需要 4 个手写文件：汇编启动代码、链接脚本、Rust 入口点和构建脚本。其他一切由工具链生成。

## boot.S — 入口点

CPU 从这里开始执行。在任何 Rust 代码运行之前，我们需要一个栈和清零的 BSS：

```armasm
.section .text.boot
.global _start

_start:
    // 设置栈（16KB，向下增长）
    adr     x0, stack_top
    mov     sp, x0

    // 清零 BSS 段
    adr     x0, __bss_start
    adr     x1, __bss_end
clear_bss:
    cmp     x0, x1
    b.ge    clear_bss_done
    str     xzr, [x0], #8
    b       clear_bss
clear_bss_done:

    // 跳转到 Rust
    bl      rust_main

halt:
    wfe
    b       halt
```

三件事情：

1. **栈设置** — `adr` 加载 `stack_top` 的地址（定义在 BSS 中）。栈向下增长，所以 SP 指向 16KB 区域的顶部。ARM64 要求 16 字节栈对齐，通过栈段的 `.align 16` 保证。

2. **BSS 清零** — Rust 的 `static mut` 变量和未初始化全局变量在 BSS 段。ELF 加载器不会帮我们清零（根本没有加载器——QEMU 直接加载原始二进制）。我们用 `str xzr` 每次 8 字节清零 `[__bss_start, __bss_end)` 范围。

3. **跳转到 Rust** — `bl rust_main` 调用 Rust 中的 `#[no_mangle] pub extern "C" fn rust_main()`。如果它返回了（不应该），会落到 `halt` 的 WFE（Wait For Event）循环。

注意：第一版 `boot.S` 还检查了 `CurrentEL` 来验证我们在 EL2。修复 commit（`b2ff49f`）移除了它——因为没有回退路径，增加复杂度没有价值。

## linker.ld — 内存布局

```ld
ENTRY(_start)

SECTIONS
{
    . = 0x40000000;   /* QEMU virt: RAM 从这里开始 */

    .text : {
        KEEP(*(.text.boot))   /* boot.S 必须在最前面！ */
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

关键决策：

- **基地址 `0x40000000`** — QEMU 的 `virt` 机器把 RAM 放在 0x40000000。使用 `-kernel` 时，QEMU 将二进制加载到这个地址。（后来加了 TF-A 启动后，改成 `0x40200000` 以避开 QEMU 在 0x40000000 生成的 DTB。）

- **`KEEP(*(.text.boot))`** — 确保 `boot.S` 的 `_start` 是二进制中的第一段代码。没有 `KEEP`，链接器可能把它当"未使用"丢弃。不放在最前面的话，CPU 会从加载地址开始执行碰巧在那里的任意函数。

- **BSS 包含栈** — 16KB 的栈在 `.bss.stack` 里，在 BSS 范围内。这意味着 BSS 清零循环也会清零栈，这没问题（反正是未初始化的）。

## rust_main() — 第一行 Rust 代码

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

- **`#![no_std]` + `#![no_main]`** — 没有标准库，没有 `fn main()`。这是一个独立运行的二进制。

- **`uart_puts` 使用内联汇编** — QEMU `virt` 上的 PL011 UART 在 `0x09000000`。向偏移 0x00（数据寄存器）写一个字节就能发送到串口控制台。我们通过内联 `asm!` 的 `str`（store register）指令来写，因为此时还没有正式的 UART 驱动。`{val:w}` 格式使用 32 位的 `w` 寄存器变体，因为 PL011 需要 32 位写入。

- **`#[no_mangle] pub extern "C"`** — 让 `boot.S` 可以通过 `bl rust_main` 调用。没有 `no_mangle`，Rust 会修饰符号名。没有 `extern "C"`，调用约定可能不同。

- **`-> !`**（永不返回）— `rust_main` 永远循环。如果它某种方式返回了，`boot.S` 的 `halt` 循环会兜底。

第一个 commit（`609459b`）有一个更完整的 UART 驱动（`src/uart.rs`），支持 `fmt::Write`、`print!`/`println!` 宏和 TX FIFO 等待。修复 commit（`b2ff49f`）把它精简成了上面的最小内联汇编版本。为什么？完整版有链接问题——`fmt` 机制拉入了太多 `core` 库的内容，超出了裸机二进制当时能支撑的范围。对 Milestone 0 来说，简单就是好。

## build.rs — 胶水代码

```rust
fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    // 用 aarch64-linux-gnu-gcc 交叉编译 boot.S
    Command::new("aarch64-linux-gnu-gcc")
        .args(&["-c", "arch/aarch64/boot.S", "-o",
                boot_o.to_str().unwrap(),
                "-nostdlib", "-ffreestanding"])
        .status().expect("Failed to compile boot.S");

    // 打包成 libboot.a
    Command::new("aarch64-linux-gnu-ar")
        .args(&["crs", boot_a.to_str().unwrap(),
                boot_o.to_str().unwrap()])
        .status().expect("Failed to create archive");

    // 告诉 Cargo 链接它
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=boot");
}
```

这是 ARM64 汇编和 Rust 之间的桥梁：

1. `aarch64-linux-gnu-gcc` 将 `boot.S` 交叉编译成目标文件
2. `aarch64-linux-gnu-ar` 打包成 `libboot.a`
3. Cargo 将 `libboot.a` 与 Rust 二进制链接

`boot.S` 的 `_start` 符号成为 ELF 入口点（由链接脚本的 `ENTRY(_start)` 指定）。Cargo 原生不支持汇编文件，所以 `build.rs` 负责交叉编译这一步。
