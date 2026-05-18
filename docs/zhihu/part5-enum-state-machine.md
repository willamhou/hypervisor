# Rust 的 match 穷尽性在状态机里没想象中好使

> 前置说明：本文讨论的是一个 **只在 QEMU virt 上验证过** 的 hypervisor 实验项目，不是生产系统，也没在真实硬件上跑过。经验适用于"想了解系统编程里 Rust 工具边界"的读者，不适合当作"生产级指南"读。
>
> 写 ARM64 hypervisor 10 周，我以为 Rust 的 match 穷尽性检查会是状态机扩展时的安全网。真上手以后发现：**它在状态机里基本帮不上忙，却在另一个我没在意的地方帮过我一次**。

---

我在写一个 ARM64 裸机 hypervisor，里面有个叫"安全分区"（Secure Partition，简称 SP）的东西。每个 SP 是一个受 SPMC 管理的轻量虚拟机，有自己的生命周期：Reset → Idle → Running → Blocked → Preempted。5 个状态，目前 8 种合法转换。

不久前我加了一种新转换：`Blocked → Preempted`，用来支持 SP 之间的链式抢占。按教科书的说法，这正好是 Rust `enum + match` 应该大显身手的场景——加一种状态 / 转换，编译器帮你找到所有没更新的代码。

真实情况是：**编译器一个字都没报**。

这篇写一下我为什么没用教程里那种"enum 带字段"的写法，`match` 穷尽性在这个状态机上为什么没帮上忙，以及它在哪里真正帮到了我。

---

## 先放真实代码

别拿理想代码讲故事。下面是我仓库里真实的 `SpState`：

```rust
// src/sp_context.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SpState {
    Reset = 0,
    Idle = 1,
    Running = 2,
    Blocked = 3,
    Preempted = 4,
}
```

这是典型的 **C 风格 enum**——`#[repr(u8)]`，每个变体只是一个 tag，没有字段。为什么不是教程推荐的 `Running { entry_pc: u64 }` / `Preempted { saved_ctx: VcpuContext }`？

因为我需要 **`AtomicU8` 存储**。

SPMC 跑在多个物理 CPU 上，不同 CPU 上的 SPMD（TF-A 的 Secure Partition Manager Dispatcher）可能同时把请求路由到同一个 SP。两个 CPU 同时尝试 `Idle → Running`，必须有一个失败退出，否则两个 CPU 会同时 ERET 进入同一个 SP，寄存器上下文当场覆盖。

我用 CAS 来做这个竞争：

```rust
pub fn try_transition(&self, expected: SpState, new_state: SpState) -> Result<(), SpState> {
    match self.state.compare_exchange(
        expected as u8,
        new_state as u8,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => Ok(()),
        Err(actual) => Err(SpState::try_from(actual).expect("corrupt SP state value")),
    }
}
```

`AtomicU8::compare_exchange` 只接受 `u8`——一个字节。即使我把 `SpState` 写成 `#[repr(u8, C)]` + 字段（layout 是稳定的），`Running { entry_pc: u64 }` 这种至少 16 字节，根本塞不进 `AtomicU8`。要么换成 `AtomicU64` + 整块 fat-struct CAS，要么放锁；我想保住 fast path 上的单字节 CAS，所以 payload 放别的地方，由状态转换本身门控。

这是系统编程里一个普普通通的 trade-off：**类型表达力 vs 硬件操作约束**。教程不太提，因为教程不跑在多 CPU 的 S-EL2 上。

---

## 为什么 match 穷尽性没帮上忙

状态转换的合法性检查集中在一个函数里：

```rust
// src/sp_context.rs
pub fn transition_to(&mut self, new_state: SpState) -> Result<(), &'static str> {
    let current = self.state();
    let valid = match (current, new_state) {
        (SpState::Reset, SpState::Idle) => true,
        (SpState::Idle, SpState::Running) => true,
        (SpState::Running, SpState::Idle) => true,
        (SpState::Running, SpState::Blocked) => true,
        (SpState::Blocked, SpState::Running) => true,
        (SpState::Blocked, SpState::Preempted) => true,  // ← 新加的一行
        (SpState::Running, SpState::Preempted) => true,
        (SpState::Preempted, SpState::Running) => true,
        _ => false,
    };
    // ...
}
```

注意最后的 `_ => false`。这**不是**穷尽的 match——它用通配符把所有未列出的组合都当非法处理。

加 `Blocked → Preempted` 的那次 commit，diff 就是 1 行。编译器没报任何错，因为所有 25 种 `(from, to)` 组合在编译器看来都被覆盖了（8 种显式 + `_` 兜底）。

我本来可以把 `_ => false` 换成列出全部 17 种非法组合。一开始是这么想的——"穷尽才是 Rust-y"。但写到一半就放弃了：

```rust
// 这样写的话......
(SpState::Reset, SpState::Reset) => false,
(SpState::Reset, SpState::Running) => false,
(SpState::Reset, SpState::Blocked) => false,
// ... 再重复 15 行
```

没有任何新信息，还让未来加状态的时候要维护 N² 张表。`_ => false` 在这里就是文档：**显式列出的就是合法集，剩下的都不合法**。

**结论**：对简单 C 风格 enum + 状态转换二元组这种场景，`match` 穷尽性救不了你。这一层的 bug 只能靠单元测试抓（我对应的 `test_sp_context.rs` 里有 58 条 assertion，覆盖大部分合法转换 + 典型非法转换 + CAS 的成功/失败语义）。

---

## 它到底在哪救了我

真正被 `match` 穷尽性救到的地方，是设备 dispatch。

我的 hypervisor 用一个 `Device` enum 枚举所有虚拟设备，每次 guest 访问 MMIO 时，用 `match` 分发到对应实现：

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

这是**带字段**的 enum，每个变体里装的是对应设备的状态结构。对它的 `match` 也不用 `_` 兜底，因为每个 variant 都有独立的处理逻辑：

```rust
impl MmioDevice for Device {
    fn read(&mut self, offset: u64, size: u8) -> Option<u64> {
        match self {
            Device::Uart(d) => d.read(offset, size),
            Device::Gicd(d) => d.read(offset, size),
            Device::Gicr(d) => d.read(offset, size),
            Device::VirtioBlk(d) => d.read(offset, size),
            Device::VirtioNet(d) => d.read(offset, size),
            Device::Pl031(d) => d.read(offset, size),
        }
    }
    // write, base_address, size, pending_irq, ack_irq ...
}
```

当初为 Android boot 加 `Pl031`（PL031 RTC）时，我只改了 enum 定义。编译器立刻报了 **6 个错**——所有对 `Device` 做 `match` 的地方都缺 `Pl031` 分支：

```text
error[E0004]: non-exhaustive patterns: `&Device::Pl031(_)` not covered
  --> src/devices/mod.rs:51:15
error[E0004]: non-exhaustive patterns: `&mut Device::Pl031(_)` not covered
  --> src/devices/mod.rs:62:15
error[E0004]: non-exhaustive patterns: `&Device::Pl031(_)` not covered
  --> src/devices/mod.rs:73:15
// ... 6 处
```

其中有 2 处是我已经记不清楚什么时候加的 dispatch 分支，**完全不在我记忆里**。如果我用的是 C，缺的 `case` 默默走 `default` → guest 对 RTC 做任何 MMIO 都会走错路径——Android userspace 启动到一半 hang 住，错误信息会指向一个完全无关的地方。

C 当然可以做到类似的保护，但需要刻意：`-Wswitch-enum` + `-Werror` + 不写 `default`，三件套全有才行。Linux kernel 在 `W=3` 启用 `-Wswitch-default`（不是 `-Wswitch-enum`），并且 `-Werror` 还得 `CONFIG_WERROR` 开着。换句话说：Linux 默认拿不到 Rust 那种"加 variant 必须更新所有 match"的强制保护，得手动配。把这套用对的 C 项目能拿到接近的安全网，没用对的（绝大多数）就拿不到。

真正的差别是：**Rust 的穷尽性是语言强制的，C 的近似保护需要工程纪律**。在我这个 one-man project 里，"语言强制"等于零成本拿到这层保护——这个差别在有专门 C style guide 的成熟项目里会小一些，在我这种独立项目里很值。

不管哪种语言，这个 bug 在这次被编译器（而不是运行时）抓住，就是本文开头说的"帮到我的地方"。

---

## 什么时候 match 穷尽性真的有用

复盘这次状态机扩展 + Device 扩展，我归纳了一下：

**穷尽性 match 救你一命的场景：带字段 enum + 每个变体有独立处理逻辑。**

- `Device::{Uart, Gicd, ..., Pl031}` — 每个设备的 `read/write` 实现完全不同
- `MmioAccess::{Read { reg, size }, Write { reg, size, val }}` — 读写语义不一样
- `ExitReason::{HvcCall, SmcCall, DataAbort, WfiWfe, ...}` — exception 类型对应不同 handler

这些场景的共同点是：**新增一个变体意味着全代码库都可能有遗漏的处理分支**，而且每个分支的正确实现都不一样（不是简单的"错误 vs 合法"这种二元输出）。

**穷尽性 match 帮不上你的场景：简单 tag enum + 笛卡尔积判断。**

- 状态机 `(from, to)` 转换表 — N² 爆炸，`_ => false` 才是可读的
- 权限矩阵 `(user_role, action)` — 同上
- 输入 sanity check `match(input) { valid_range => ..., _ => reject }` — tautological

这一类场景本质上是"显式枚举一小部分合法情形，剩下全拒绝"。写成 `_ => fallback` 没有损失信息量，反而更清晰。

---

## 几条经验

**1. `#[repr(u8)]` 是 hypervisor / kernel / 驱动里的日常，别为 atomic 的 trade-off 道歉。**

Twitter 上每次出现 "Rust 状态机" 类推文，评论区总会有人说"应该用 typestate 模式 / phantom type / 带字段 enum"。这些在 userspace 是好建议，在跑在 `AtomicU8` 上的多 CPU SPMC 里不成立。选边，记录原因。

**2. `_ => fallback` 不是罪，但每次写都要再问一遍。**

"如果我未来加了新 variant，这里应不应该强制我更新？"

- 应该 → 别 `_`，列出所有 variant
- 不应该（比如状态机的非法 pair、MMIO 的 unknown offset） → `_ => default` 是文档

**3. 状态机的正确性从来不是 Rust 的赠礼，是测试 + 文档 + 代码审查的赠礼。**

我的 `test_sp_context.rs` 里有专门的测试覆盖大部分合法转换 + 若干关键非法转换 + CAS 的成功/失败语义。这些不是 Rust 生成的，是我写的。Rust 让我少写一些防御性代码（比如不用担心 `SpState` 会有"第 6 个值"——`try_from_u8` 会 reject），但合法转换表是不是正确，Rust 不知道。

**4. 真正救你的是"带字段 enum + 每个 variant 独立 handler"的组合。**

这是 Rust 的招牌能力。识别出你代码里哪些地方符合这个模式，把它们写对，比去纠结状态机要不要用 typestate 划算得多。

---

## 尾声

我的 hypervisor 不是"zero unwrap" 项目。`src/` 里大概 6 个 `unwrap()`（集中在启动期无法 panic 的路径，比如 `vm.rs`、`guest_loader.rs`），测试目录里更多——但每一个都有具体理由。`_ => default` 兜底分支几十处，绝大多数在 MMIO 寄存器解码的 unknown-offset 路径。

每一个 `unwrap()` 和 `_ =>` 都是在那一刻做过判断的结果，不是懒得写。这比"消灭所有 unwrap"的口号更贴近系统编程的现实：**工具是手段，不是教条**。

Rust 给你一把好用的武器，但它不能替你思考。状态机转换表合不合法，是你脑子里的事，不是编译器的事。

---

代码：<https://github.com/willamhou/hypervisor>

博客：<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第五篇。之前的文章：*

- *Part 0a: [为什么写一个 Hypervisor](./part0a-why.md)*
- *Part 0b: [AI 辅助系统编程](./part0b-ai-workflow.md)*
- *Part 1: [从零到 "Hello from EL2!"](./part1-first-boot.md)*
- *Part 2: [陷入-模拟-恢复](./part2-trap-emulate-resume.md)*
- *Part 3: [让 Linux 启动](./part3-linux-boot.md)*
- *Part 4: [裸机四大坑](./part4-war-stories.md)*
