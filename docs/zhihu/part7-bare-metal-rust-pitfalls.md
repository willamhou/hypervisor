# 裸机 Rust 的三个"Rust 没问题，硬件有话说"的坑

> 在 OS 下面写 Rust，语言还是那个 Rust。但生成的机器码会撞上你没见过的硬件行为——三个真实的例子。

---

我在写一个 ARM64 bare-metal hypervisor，`no_std`，没有操作系统、没有 libc、没有运行时。在这个环境下 Rust 语言本身和你写 CLI 工具没区别：所有权、借用、trait、async——照常工作。但硬件变了，原来 OS 在你看不见的地方替你兜住的事情，现在全部浮出水面。

这篇写三个过去 10 周里让我反复吃瘪的坑。共同主题：**Rust 代码是对的，编译器是对的，硬件有它自己的规矩**。

---

## 坑 1：`debug_assert!` 里藏了一条 NEON 指令

**第 4 周。** SPMC 在 release 模式正常启动。换成 debug 模式，上电后第一次 `read_volatile(mmio_addr)` 就死。没有 panic，没有 fault 输出，串口完全静默，CPU 永远停在那里。

GDB attach 上去，发现 CPU 卡在 **EL3** 的异常处理器里（不是我写的代码）。查 `ESR_EL3`，异常 class 是 `0x07`——**FP/SIMD access trapped**。

问题是我的 hypervisor 从头到尾没用过一次浮点。`no_std`，没 `f32`/`f64`，没 `libm`，整个项目的 `Cargo.toml` 里没有任何浮点相关的依赖。

`ELR_EL3` 指向的地址反汇编出来是这样：

```text
  200140: cnt   v0.8b, v0.8b
  200144: addv  b0, v0.8b
  200148: umov  w0, v0.b[0]
```

`cnt v0.8b, v0.8b` 是 NEON SIMD 指令——**对一个 64 位寄存器里每字节做 popcount**。SIMD 到底是从哪来的？

顺着 `ELR_EL3` 倒推，发现这是 `read_volatile` 调用点内联展开的结果。Rust 的 `core::ptr::read_volatile` 内部走 `ub_checks::assert_unsafe_precondition!`，是否真的把对齐检查 emit 进二进制取决于编译时的 UB checks 开关（debug profile 默认开；release 默认关）。具体宏/函数名随 stdlib 版本变，我这版（较新的 nightly）触发到的是对齐检查这一支。对齐校验的调用链大致是：

```text
read_volatile(src)
  → ub_checks::maybe_is_aligned(addr, align)
    → addr.is_aligned_to(align)
      → align.is_power_of_two()
        → align.count_ones() == 1
```

`count_ones()` 就是 popcount。LLVM 在 AArch64 上把 popcount lower 成下面这串 NEON 指令（参考：LLVM 的 AArch64 popcount lowering）：

```text
cnt   v0.8b, v0.8b      ; per-byte popcount
addv  b0,    v0.8b       ; horizontal sum
umov  w0,    v0.b[0]     ; move scalar back to GPR
```

编译器选 NEON 不是 bug——`cnt` 是 ARMv8-A 上做 popcount 最快的方式，也是 LLVM 的默认 codegen。

在有 OS 的环境里这没问题——OS 启动时已经使能了 FP/SIMD，任何用户程序用 NEON 都直接放行。但我跑在 TF-A 之上，TF-A 的默认配置是 **`CPTR_EL3.TFP=1`**：意思是 "任何 EL2 及以下的 FP/SIMD 指令都陷入到 EL3 处理"。EL3 的默认 trap handler 没有针对这种陷入的处理逻辑，看见来源不明的 SIMD 异常就原地死循环。

release 模式下 `debug_assert!` 被优化掉，`cnt` 指令不出现，一切正常。

### 修复

TF-A 编译时设置：

```makefile
CTX_INCLUDE_FPREGS=1
```

这个 flag 让 TF-A 在 world switch 时保存/恢复 FP/SIMD 寄存器，同时清掉 `CPTR_EL3.TFP`。必须同时设 `ENABLE_SVE_FOR_NS=0` 和 `ENABLE_SME_FOR_NS=0`，否则 TF-A 构建会在 SVE/SME 相关的 feature gate 上报冲突（我花了半小时才理解它们是互斥的）。

### 教训

**在 OS 下面，编译器的代码生成是硬件契约的一部分**。你的高级语言代码里"我没写浮点"不代表二进制里没有浮点指令。Rust 的 debug 断言密度远比 C 高，任何 sanity check 都可能被降到你的 exception level 禁用的指令集上。

实战 checklist：

1. 裸机项目默认用 release build 开发。要 debug 用 `opt-level = "z"` + 手动加 `println!`
2. 如果非要用 debug build，检查反汇编里是否有 `v0`-`v31` 寄存器引用
3. 确认你的 exception level 能执行这些指令（`CPTR_ELx` 不 trap）

这不是 Rust 独有的问题——Clang/GCC 的 debug 模式同样可能选 NEON。只是 Rust 的 safety check 更密，触发概率更高。

---

## 坑 2：`SMC` 不是 memory barrier

**第 11 周。** pKVM 通过 FF-A 和我的 SPMC 共享内存做 `MEM_SHARE`。pKVM 在某个 pCPU 上把 FF-A 描述符写到 TX buffer，然后 `smc #0` 进 EL3，EL3 切到 S-EL2，我读这个 buffer 解析。

大部分时候（~70%）工作正常。少部分时候（~30%）SPMC 读出来的 `composite_memory_region_offset` 是垃圾值（`0x240f` 之类）。SPMC 用 `base + offset` 做指针运算——Data Abort。

第一反应是解析器 bug。`addr2line` 定位到 `parse_mem_region`——函数逻辑是对的，读到的原始字节就是错的。

这里有一个经常被当真的误解：**"SMC 指令是 memory barrier"**。它不是。ARMv8-A 明确规定：

> An SMC instruction is a Synchronous exception. It causes a Context Synchronization Event, but no Data Synchronization Barrier or Instruction Synchronization Barrier.

`Context Synchronization Event` 只保证 CPU 自己的流水线/预测状态对指令执行序列一致，**对跨 CPU 的内存可见性没有任何保证**。

回到我的 bug。关键是这个顺序：

```text
pCPU_A  pKVM: 写描述符到 TX buffer → smc #0 → 进 EL3
pCPU_A  SPMD: 切到 S-EL2 on pCPU_A
pCPU_A  SPMC: 记录 FFA_MEM_SHARE 请求，返回 handle 给 pKVM

时间流逝...

pCPU_B  pKVM: 调用 FFA_RUN(SP2, handle) 把 SP2 调到 pCPU_B
pCPU_B  SPMC: ERET 进入 SP2，SP2 去读那个 TX buffer → ???
```

TX buffer 在 Normal World DRAM，pKVM 在 pCPU_A 写、pCPU_B 的 SPMC 读。这里有个容易踩的细节要先讲清楚：**`dsb` 屏障只 order 执行它的那个 CPU 的 memory 访问，不会反向去 flush 别人 L1**。让 pCPU_A 的写最终对 pCPU_B 可见，靠的是 ARM 的 Inner Shareable cache coherency 协议；写入方在合适时机做 `dsb ish/sy`、读取方按需做 `dsb` + 可能的 cache maintenance，两边凑齐才完整。单凭一条 `smc` 既不是屏障，也不触发任何一致性流程。

我先在读取侧加了 `dsb sy`，比之前稳一些，但还是偶尔崩。原因不难猜：写入侧（pKVM）在我控制不了的代码里，能不能保证写完做了正确的屏障，这事我没法假设。再叠加跨世界、跨 CPU、共享 NWd DRAM 这一堆变量，只靠"读取前一条 barrier"显然不够。

### 修法：barrier + 本地拷贝 + bounded parse

真实代码（`src/spmc_handler.rs:2370-2432`）：

```rust
// src/spmc_handler.rs
// DSB SY: ensure NWd's TX buffer writes are visible to S-EL2.
// pKVM's per-CPU SPMD may enter S-EL2 on a different physical CPU
// than the one that wrote the descriptor — L1 D-cache can be stale.
// SAFETY: DSB SY is a barrier instruction with no side effects.
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }

let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8,
        local_buf.as_mut_ptr(),
        total_length as usize,
    );
}

// 绝不直接从共享 buffer 解析——所有解析都在 local_buf 上
let parsed = parse_mem_region(local_buf.as_ptr(), total_length);
```

加这个修改之后，测试里原来间歇性的 Data Abort 不再复现。

为什么"拷贝"解决了"barrier 没解决"的问题？

注意别讲过头：`copy_nonoverlapping` 不保证拷贝到的字节代表 producer 某一刻的快照——拷贝过程仍可能横跨新旧 cache line 各读一些。它真正改变的是后续解析阶段：解析器从此只读一份**不会再变的本地副本**。`parse_mem_region` 在这份固定的字节流上做 bounded check，要么解析成功，要么因为 offset/长度越界返回 `FFA_INVALID_PARAMETERS`——不会再去追共享 buffer 里的指针，也不会因再次读时字节又变了而崩。

这个 pattern 把问题的后果从"不可恢复的 Data Abort"降级成"偶发的 `FFA_INVALID_PARAMETERS`"，前者极难 debug，后者一目了然。

### 教训

跨 world + 跨 pCPU 的共享 buffer，不要直接解析。**先 `dsb sy`，再拷贝到本地栈/静态 buffer，再让所有解析逻辑只读这份本地副本**。

这个 pattern 真正稳住的不是"看到的一定是 producer 那个时刻的快照"——拷贝过程仍可能横跨多个新旧 cache line。它稳住的是 **解析阶段不再去反复追指针**：bounded parse 跑在一份不会再变的本地字节流上，要么解析成功，要么因 offset/长度越界返回错误码——不会去追野指针。换句话说：跨核可见性问题没消失，但"读到坏数据"的后果从"不可恢复的 Data Abort"降级成"返回 `FFA_INVALID_PARAMETERS`"——后者要好 debug 几个数量级。

顺带澄清一个相关的误解：**SMC 不会把请求路由到别的 CPU**。它是同步异常，永远在发起 CPU 上处理。但 SP 被 `FFA_RUN` 恢复时可以跑在任意 pCPU 上——这是 SP 调度，不是 SMC 迁核。把两者搞混会让你写出错误的 barrier 逻辑。

---

## 坑 3：QEMU `-bios` 模式不传 DTB 给 BL33

**第 2 周。** hypervisor 里准备解析 DTB。开始很顺：

```rust
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_addr as *const u8) };
    let uart = find_uart(&fdt);
    let gic = find_gic(&fdt);
    // ...
}
```

QEMU 在 `-kernel` 模式下启动时，按 Linux kernel boot 约定：x0 = DTB 物理地址。拿到就能解析。

跑了几周都好好的。到第 4 周切到 `-bios` 模式（为了跑 TF-A），boot 直接卡在 `fdt::Fdt::from_ptr` 里。打印 x0 的值——**0**。

查 QEMU 源码（`hw/arm/virt.c`）+ TF-A 的 QEMU 平台文档发现：

- `-kernel` 模式：QEMU 自己构造 DTB，放在 RAM 里，地址通过 x0 传给 kernel entry
- `-bios` 模式：QEMU 把 DTB 留在内存里，BL2 把它当作 `HW_CONFIG`。**BL33 在 x0 拿到什么，完全取决于 TF-A 的 build option**：
  - `ARM_LINUX_KERNEL_AS_BL33=1`：FDT 地址会放进 BL33 的 x0（Linux-as-BL33 路径）
  - TF-A QEMU port 的默认（没设 `ARM_LINUX_KERNEL_AS_BL33`）：BL33 入口寄存器装的是 MPIDR low bits——boot CPU 的 `x0` 等于 0，从核拿到的是自己的 MPIDR。这里根本没有 DTB 指针

我这套链是 SPMC + 自定义 hypervisor 当 BL33，没用 `ARM_LINUX_KERNEL_AS_BL33`。第一版代码假设 `x0 = DTB` 就是错的——实际拿到的是 boot CPU 的 MPIDR low bits（也就是 0）。通用的教训是：**BL33 收到的 x0 不能假设有固定语义，完全看 TF-A 怎么配的**。

这不是 QEMU 的 bug——是"bootloader 启动约定"这回事本来就没有统一标准。`-kernel` 模式遵循 Linux 的 boot protocol，`-bios` 模式遵循 TF-A 的 `HW_CONFIG` 约定（再叠加 BL33 的 build option），真实硬件是各厂商自己的 UEFI/coreboot 约定。如果你的 hypervisor 写死了"我从 x0 拿 DTB"，你就绑死了一种环境。

### 修法：所有启动参数走 fallback

真实代码在 `src/dtb.rs`：

```rust
// src/dtb.rs
/// Global platform info with QEMU virt defaults.
static PLATFORM_INFO: PlatformInfoCell = PlatformInfoCell {
    inner: UnsafeCell::new(PlatformInfo {
        uart_base: 0x0900_0000,
        gicd_base: 0x0800_0000,
        gicr_base: 0x080A_0000,
        gicr_size: 0,
        num_cpus: 4,
        ram_base: 0x4000_0000,
        ram_size: 0x4000_0000,
    }),
    initialized: AtomicBool::new(false),
};

pub fn init(dtb_addr: usize) {
    if let Some(info) = parse_host_dtb(dtb_addr) {
        unsafe { *PLATFORM_INFO.inner.get() = info; }
        PLATFORM_INFO.initialized.store(true, Ordering::Release);
    }
    // DTB 解析失败时，defaults 保持不变，hypervisor 继续跑
}

fn validate_dtb_address(addr: usize) -> bool {
    if addr == 0 { return false; }
    if !(0x4000_0000..0x8000_0000).contains(&addr) { return false; }
    let magic = unsafe { core::ptr::read_volatile(addr as *const u32) };
    u32::from_be(magic) == 0xD00D_FEED
}
```

两点注意：

1. **Defaults 是 `static` 初始化的**，不是"DTB 解析失败就 panic"。如果我在 `-bios` 模式跑硬编码的 QEMU virt 默认值，一切照常工作。
2. **`validate_dtb_address` 做三重校验**——非零、在 RAM 范围内、magic 正确。因为 x0 可能是 0，可能是随机 junk，可能指向一段完全不相关的内存。一个坏 DTB 地址让 fdt crate panic 没意义，不如直接走 defaults。

### 教训

写 hypervisor 不像写 userspace 程序——你不能假设环境"为你准备好了一切"。DTB 可能没收到；GIC 寄存器初始值可能是随机的；UART 可能还没被上一级 bootloader 使能。

**每一个你想"应该已经设好"的东西，都要有一条 fallback 路径或 panic message**。默认配置 + 尝试从环境获取更新 + 解析失败走默认——这个 pattern 已经能让我的 hypervisor 在 QEMU `-kernel` 和 QEMU `-bios` 两种 boot 路径下走同一份代码。

要扩到真硬件还差一截：现在的 `validate_dtb_address()` 把 DTB 地址硬编码限制在 QEMU virt 的 RAM 范围（`0x4000_0000..0x8000_0000`）。换台机器、换块板子，这个范围就要重设；想做"真正通用的 fallback"，得改成读 `/memory` 节点 / UEFI memmap，或者直接改成"任何不在某个 known-bad 区段的地址都接受 magic 校验"。我现在没做，因为目标平台只有 QEMU virt——但要做生产级的 hypervisor，这层是补不掉的。

---

## 归纳：bare-metal Rust 的三类盲区

复盘这三个坑，它们的共性：

1. **编译器 codegen 假设了 "正常 OS 环境"**  
   Debug assert 用 NEON、allocator 依赖页错误处理、panic handler 假设有 stdout——这些假设在裸机下会变成 bug。

2. **硬件内存模型的细节比你记得的多**  
   Cache 一致性、NS 位、SMC 不是屏障、`dsb ish` vs `dsb sy` 的区别、Inner vs Outer Shareable——单独看都不难，组合起来能让你调一整天。

3. **Bootloader 启动约定没有标准**  
   `-kernel` / `-bios` / UEFI / coreboot / TF-A 各管各的。通过 x0/x1 传什么、寄存器初始状态、堆栈指针——每个环境都不一样。

每个坑单独看都有不长的解释。真卡住的时候，要花几天是因为**你的心智模型默认了某个在这一层不成立的前提**——以为 SMC 是 barrier，以为 `read_volatile` 只读内存，以为启动时 DTB 一定在 x0。

写 bare-metal Rust 最重要的能力不是 Rust，是：**看到"不可能"的现象时，能想到去查 ARM ARM、TF-A 源码、LLVM 降指令规则，而不是怀疑编译器有 bug**。抽象层下面没有 OS 帮你兜底。你的对手不是代码，是你对硬件的心智模型跟硬件实际行为之间的差距。

缩小那个差距没有捷径——**反汇编、读手册、读固件源码**。

---

代码：<https://github.com/willamhou/hypervisor>

博客：<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第七篇，也是本周长文的最后一篇。之前的文章：*

- *Part 0a: [为什么写一个 Hypervisor](./part0a-why.md)*
- *Part 0b: [AI 辅助系统编程](./part0b-ai-workflow.md)*
- *Part 1: [从零到 "Hello from EL2!"](./part1-first-boot.md)*
- *Part 2: [陷入-模拟-恢复](./part2-trap-emulate-resume.md)*
- *Part 3: [让 Linux 启动](./part3-linux-boot.md)*
- *Part 4: [裸机四大坑](./part4-war-stories.md)*
- *Part 5: [Rust enum 状态机的真相](./part5-enum-state-machine.md)*
- *Part 6: [TrustZone 的 NS 位](./part6-trustzone-ns-bit.md)*
