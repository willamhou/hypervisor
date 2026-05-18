# 裸机 Rust 的 3 个"Rust 没问题，硬件有话说"的坑

> 发布日期：4/26
> Rust 编译器在 bare-metal 上还是那个 Rust 编译器，但它生成的代码会遇到你没见过的硬件行为。

---

我写了一个 ARM64 bare-metal hypervisor，用 `no_std` Rust。没有操作系统、没有 libc、没有运行时。在这个环境下，Rust 语言本身和写 CLI 没区别，但**硬件不一样了**，很多你平时不会想到的细节会变成 bug。

下面是 10 周开发里踩过的 3 个坑，每个都是"Rust 本身行为正确，但和硬件假设冲突"。

## 坑 1：Debug 模式的 NEON popcount

第 4 周。SPMC 在 release 模式正常，debug 模式一启动就挂。没有输出、没有 fault、完全死机。

GDB attach 之后发现 CPU 卡在 EL3 的异常向量表里。`ESR_EL3` 显示 `EC=0x07`——FP/SIMD exception。

但我的 hypervisor 不用浮点。`no_std`、没有 `f32/f64`、整个代码库没有一个浮点操作。

看 `ELR_EL3`（异常发生时的 PC），定位到一个 `read_volatile(mmio_addr)` 调用。反汇编：

```
  200140:	cnt	v0.8b, v0.8b
  200144:	addv	b0, v0.8b
  200148:	umov	w0, v0.b[0]
```

`cnt v0.8b, v0.8b` 是 NEON SIMD 指令——**popcount**。

为什么 `read_volatile` 会有 popcount？

追下去：`read_volatile` 在 debug 模式下会做 `debug_assert!(is_aligned(addr))`，这个 assert 包含 `debug_assert!(align.is_power_of_two())`。`is_power_of_two` 的实现是 `popcount(x) == 1`。LLVM 在 AArch64 上把 popcount 降到 NEON `cnt` 指令。

release 模式下 `debug_assert!` 被消除，NEON 指令不出现。

TF-A 默认把 `CPTR_EL3.TFP` 置位，拦截 EL2 及以下的所有 FP/SIMD 指令。S-EL2 执行 NEON → trap 到 EL3 → EL3 处理器没准备好处理这种 trap → 死循环。

修复：TF-A 编译时加 `CTX_INCLUDE_FPREGS=1`，清掉 `CPTR_EL3.TFP`。

**教训**：Rust 的 debug 断言可能含你想不到的指令（popcount → NEON）。在 bare-metal 下任何 debug assert 都可能触发意料外的硬件行为。要么全 release 构建，要么确认 FP/SIMD 在你的 exception level 可用。

## 坑 2：跨 pCPU 的 buffer 可见性

第 11 周。pKVM 和我的 SPMC 之间通过 FF-A 共享内存。pKVM 在 CPU 0 上写一个描述符，然后 `smc #0` 到 EL3，EL3 路由到 S-EL2 的我。

这里关键：**SP 可能在之后被 `FFA_RUN` 在 CPU 2 上 resume**（SP 调度允许跨 pCPU）。当 SP 在 CPU 2 继续执行，并通过 FF-A 要读 pKVM 写的那个 buffer——CPU 2 的 L1 cache 可能还没更新。

30% 的时候我读出来的 `composite_offset` 是垃圾值（`0x240f` 之类），明显不是 80。

**一个常见误解**：SMC 指令是一种 memory barrier。**不是**。ARMv8-A 规定 SMC 只有 Context Synchronization Event，对 memory ordering 没有任何保证。跨 CPU 可见性需要显式 `dsb ish` 或 `dsb sy`。

我加了 `DSB SY` 后还是偶尔出错。最后的解决方案是**先拷贝到本地栈，再解析**：

```rust
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)); }
let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8, local_buf.as_mut_ptr(), total_length,
    );
}
let desc = parse_mem_region(local_buf.as_ptr(), total_length);
```

拷贝过程本身是多次 read，ARM 的内存模型保证 read-after-read 单 CPU 一致。即使拷贝到的 `local_buf` 是某个旧快照，至少它是**自洽的**——边界检查能正常判断，不会像随机垃圾那样让 parser 解引用到野指针。

从 30% crash 降到 0。

**教训**：跨 world/跨 pCPU 的共享 buffer，就算加了 `DSB SY`，最稳妥的做法是先本地拷贝再解析。这样即使捕获到 stale 快照，至少是自洽的。

## 坑 3：QEMU `-bios` 模式没给 BL33 传 DTB

第 2 周。hypervisor 开始跑了，准备解析 DTB。

```rust
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_addr as *const u8) };
    // ...
}
```

QEMU 启动时 x0 是 DTB 地址——`-kernel` 模式下这样。切到 `-bios` 模式（跑 TF-A），DTB 地址是 **0**。

查 QEMU 源码发现：`-bios` 模式下 QEMU 只把 DTB 作为 BL2 的 `HW_CONFIG` 传给 BL31（EL3），**不传给 BL33**（我的 hypervisor 是 BL33）。

修复：让 BL33 写死 QEMU virt 的默认值，DTB 不存在时 fallback：

```rust
pub const UART_BASE: u64 = 0x0900_0000;
pub const GICD_BASE: u64 = 0x0800_0000;
pub const GICR_BASE: u64 = 0x080A_0000;
```

**教训**：bootloader 的启动约定没有一致标准。`-kernel` 和 `-bios` 传参不同；真实硬件是另一套；TF-A 各种配置又是另一套。你的 hypervisor 必须对"没拿到 DTB"这种情况有 fallback。

## 总结

Rust 在 bare-metal 下没有惊喜——语言行为和你写 CLI 时一样。但你会反复遇到：

1. **编译器 codegen 选择**（debug assert、SIMD、allocator 使用）有对"正常 OS 环境"的假设
2. **硬件内存模型**的细节（cache coherency、NS bit、SMC 不是 barrier）
3. **Bootloader 环境**的不一致（DTB 传递、启动约定、寄存器初始状态）

每个坑单独看都不难，但真遇到的时候要花几天。写 bare-metal Rust 最关键的技能不是 Rust 本身，是**看到"不可能"的现象时，能想到去查 ARM 手册和 TF-A 源码，而不是怀疑编译器 bug**。

---

代码：https://github.com/willamhou/hypervisor
博客：https://willamhou.github.io/hypervisor/
