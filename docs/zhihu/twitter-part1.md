# Twitter Posts — Part 1

## 中文版

```
从零到 "Hello from EL2!" — 四个文件启动一个 ARM64 裸机 hypervisor。

四个文件：boot.S（汇编入口）、linker.ld（内存布局）、rust_main()（10 行 Rust）、build.rs（编译胶水）。

两个 commit：第一个不能跑——加载地址写错了，UART 驱动写复杂了。第二个对了。

裸机调试的现实：失败就是沉默。没有错误信息，没有崩溃转储。只有一个空白的终端。

系列第三篇，第一篇技术文。

👇
```

## English Version

```
From zero to "Hello from EL2!" — booting a bare-metal ARM64 hypervisor with 4 files.

boot.S (assembly entry), linker.ld (memory layout), rust_main() (10 lines of Rust), build.rs (build glue).

Two commits. First one: silence. Load address wrong (0x80000000 instead of 0x40000000). UART driver over-engineered (112 lines → 10 lines of inline asm).

Bare-metal debugging reality: failure = absence. No error message, no crash dump. Just a blank terminal.

Part 1 of the "Scratch a Rust Hypervisor" series. First technical article.

🧵👇
```

## 配图建议

终端截图，选一个：
- `make run` 输出 "Hello from EL2!" 的串口截图
- boot.S 代码高亮截图（_start 标签 + 栈设置 + BSS 清零 + bl rust_main）
- 两个 commit 的 diff 对比——112 行 UART 驱动 vs 10 行内联汇编
