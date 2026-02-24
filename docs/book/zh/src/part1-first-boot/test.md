# 测试：串口输出作为生命迹象

## 第一个"测试"

这个阶段没有测试框架，没有断言，没有 CI。测试就是：`make run` 能不能在 QEMU 串口控制台产生预期输出？

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

如果你看到这个，整条链路都通了：
- `aarch64-linux-gnu-gcc` 编译了 `boot.S`
- `build.rs` 把它打包成 `libboot.a`
- Cargo 将 Rust 交叉编译到 `aarch64-unknown-none`
- 链接器把 `_start` 放在了加载地址
- QEMU 加载了二进制并在 EL2 开始执行
- 汇编设置好栈并跳转到 Rust
- Rust 通过内联汇编向 UART 写入字节
- QEMU 的 PL011 仿真将它们转发到你的终端

如果你什么都没看到——沉默。没有错误信息，没有崩溃转储。只是一个空白的终端。这就是裸机调试的现实：失败就是无。

## 为什么不用真正的测试框架？

后面会搭建的（Part 2 加了第一个基于 HVC 的测试，Part 3 加了正式的测试编排）。但现在，串口输出是我们唯一的反馈通道。Hypervisor 在 QEMU 上以 `-nographic` 运行，PL011 UART 被路由到 stdio。每个 `uart_puts()` 调用都是一个原始断言："我执行到了这个点。"

这种 "printf 调试" 方法即使有了正式的测试框架后仍然有用。当 EL2 出了问题，测试框架本身可能也坏了的时候，`uart_puts(b"GOT HERE\n")` 有时是唯一管用的工具。

## 退出 QEMU

`Ctrl+A` 然后 `X` 发送 QEMU 的终止序列。这不是 guest 发起的退出——是 QEMU 监控器杀死虚拟机。后面我们会加 `PSCI SYSTEM_RESET` 来干净关机，但这个阶段 hypervisor 只是永远循环 WFE。
