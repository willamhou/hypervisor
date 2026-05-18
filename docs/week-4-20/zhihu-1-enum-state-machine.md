# Rust 的 enum + match 在我的状态机里抓到两个 bug

> 发布日期：4/22
> 以我写 ARM64 hypervisor 的一次状态机扩展为例，聊聊 Rust 的 match 穷尽性检查在系统编程里的实际价值——不过先要说清楚它的 limits。

---

我在写一个 ARM64 裸机 hypervisor，里面有个叫"安全分区"（Secure Partition，SP）的概念。每个 SP 是一个轻量虚拟机，有自己的生命周期：Reset → Idle → Running → Blocked → Preempted。5 个状态，合法转换 15 种。

这是典型的状态机。上周我加了一个新转换（`Blocked → Preempted`，处理链式抢占），Rust 编译器在两个我没想到的地方报错。这篇写一下这次经历——以及我**没**用 Rust 新手教程推荐的那种"enum 带字段"的写法。

## 实际代码（不是理想写法）

先放真实代码，避免拿示意代码讲故事：

```rust
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

这是 **C 风格 enum**，没有字段。为什么不用教程推荐的 `Running { entry_pc: u64 }`？

因为我需要 `AtomicU8` 存储。SPMC 跑在 multi-CPU，多个 CPU 可能同时尝试 dispatch 同一个 SP，状态更新必须用 CAS（compare-and-swap）。Rust 的 atomic 只支持 integer types。带字段的 enum 不是 `#[repr(C)]`，没有稳定 memory layout，不能 CAS。

这是系统编程的日常 trade-off：**类型表达力 vs 硬件操作约束**。

## 状态转换的约束

状态转换逻辑放在一个集中函数里：

```rust
fn valid_transition(from: SpState, to: SpState) -> bool {
    match (from, to) {
        (SpState::Reset, SpState::Idle) => true,
        (SpState::Idle, SpState::Running) => true,
        (SpState::Running, SpState::Idle) => true,
        (SpState::Running, SpState::Blocked) => true,
        (SpState::Running, SpState::Preempted) => true,
        (SpState::Blocked, SpState::Running) => true,
        (SpState::Preempted, SpState::Running) => true,
        _ => false,
    }
}
```

坦白说——这里我用了 `_ => false` 兜底。不是穷尽的。为什么？因为状态转换的"非法组合"太多（5×5 = 25 种），列出每一对会让代码非常啰嗦。

**match 穷尽性在这里没帮上忙**。这一段的 bug 只能靠测试抓。

## match 在 ExitReason 上帮上了忙

真正有用的是 SP 运行时产生的 exit 事件：

```rust
fn handle_sp_exit(sp: &mut SpContext, exit: ExitReason) -> DispatchResult {
    match exit {
        ExitReason::DirectResp { x4, x5, x6, x7 } => { /* return to caller */ }
        ExitReason::MemRetrieve { handle } => { /* handle locally, re-enter */ }
        ExitReason::MemRelinquish { handle } => { /* handle locally, re-enter */ }
        ExitReason::MemShare { descriptor } => { /* record, re-enter */ }
        ExitReason::ConsoleLog { ref buf } => { /* print, re-enter */ }
        ExitReason::DirectReq { target, .. } => { /* dispatch to target SP */ }
    }
}
```

这里 `ExitReason` 是一个带字段的 `enum`，每个 variant 有自己的 payload。没有 `_ =>` 兜底。

加链式抢占的时候，我给 `ExitReason` 加了一个新 variant（`IrqPreempt { saved_pc: u64 }`）。编译器立刻报错：

```
error[E0004]: non-exhaustive patterns: `IrqPreempt { .. }` not covered
   --> src/spmc_handler.rs:1163
```

——`handle_sp_exit` 和另一个 `handle_sp_exit_as_caller` 两处都漏了处理。都是真 bug：

- `handle_sp_exit`：会把 IrqPreempt 当成未知 exit 拒绝，正常流程丢失事件
- `handle_sp_exit_as_caller`：会把被抢占的 SP 当成"完成 direct_resp"，上层状态错乱

这两个 bug 在运行时很难复现（需要 chain preemption 精确时序），但编译期直接出来了。

## 经验

1. **穷尽性检查对带字段 enum 最有用**。`match exit_reason` 这种地方每个 variant 都有独立处理逻辑，加新 variant 时编译器能帮你找到所有需要更新的地方。

2. **对"状态转换对"这种笛卡尔积大的场景，match 帮不上忙**。这时候需要结合测试 + 文档描述转换图。

3. **原子操作和类型表达力有 trade-off**。你不能要求 `Running { entry_pc: u64 }` 同时能 `AtomicU8::compare_exchange`。选一边。

4. **`_ =>` 兜底不是罪恶**。但每次用都要问："如果我加了新 variant，这里应不应该改？"如果答案是"应该"，就别用 `_ =>`。

## Google 的 Rust 在 Android 里的定位

文章末尾有点 disclaimer：Google 的 Android 用 Rust 是**在 Android Virtualization Framework 里**（crosvm、virtmgr、RMI 桥接等组件），以及 **Pixel 调制解调器固件**。pKVM hypervisor 本身仍然是 Linux 内核里的 C。不要把"Rust 在 Android"等同于"pKVM 是 Rust"。

## 尾声

我的 hypervisor 用 Rust 写到 `unwrap()` 数量目前还有 6 个（都是在 test fixture 和启动期无法 panic 的路径），`_ =>` 兜底有 45 处（大部分是 MMIO 寄存器解码的 unknown-offset 路径，行为就是返回 0）。不是 "zero unwrap" 项目，但是**每一个 `unwrap()` 和 `_ =>` 都是明确的决策**，而不是懒得写。

这比"消灭 unwrap"这种口号更贴近系统编程的现实：**工具是手段，不是教条**。

---

代码：https://github.com/willamhou/hypervisor
博客：https://willamhou.github.io/hypervisor/
