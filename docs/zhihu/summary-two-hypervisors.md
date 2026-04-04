# 一颗芯片上跑两个 Hypervisor：用 3 万行 Rust 替换 Google 的 Hafnium

> 从零写一个 ARM64 S-EL2 安全分区管理器，和 Android pKVM 共存，10 周，35/35 端到端测试通过。

---

我写了一个 ARM64 裸机 hypervisor，它和 Google 的 pKVM **跑在同一颗芯片上**。pKVM 占 Normal World（NS-EL2），我的 hypervisor 占 Secure World（S-EL2）。两者通过 ARM 的 FF-A 协议通信，由 EL3 固件中转。

Secure World 本来有现成实现：Google 的 [Hafnium](https://hafnium.googlesource.com/hafnium/)，20 万行 C。我用 3 万行 `no_std` Rust 替换了它——没有运行时、没有 allocator crate、只依赖一个 DTB 解析库。能启动 Linux 到 BusyBox shell，管理 3 个安全分区，支持完整的 FF-A v1.1 消息传递和内存共享。

35 个端到端测试跑通了完整的四层栈：Linux 内核模块 → pKVM → TF-A → 我的 SPMC → 安全分区 → 原路返回。

## ARM 的两个世界

ARM 最新的芯片把 CPU 分成两个安全世界，每个世界在 EL2 都有自己的 hypervisor：

```
            Normal World          Secure World
           ┌────────────┐       ┌────────────┐
    EL0    │  用户态     │       │            │
           ├────────────┤       ├────────────┤
    EL1    │ Linux/Android│      │  安全分区   │
           │  内核       │       │  (SP)      │
           ├────────────┤       ├────────────┤
    EL2    │  pKVM       │       │  SPMC      │
           │  (NS-EL2)   │       │  (S-EL2)   │
           └──────┬──────┘       └──────┬──────┘
                  │      ┌──────┐       │
    EL3           └──────│ TF-A │───────┘
                         │ SPMD │
                         └──────┘
```

EL3 是信任根——ARM Trusted Firmware（TF-A）在这里，通过 SMC 调用在两个世界之间中转消息。通信协议是 [FF-A](https://developer.arm.com/documentation/den0077/latest) v1.1，定义了消息传递、内存共享、页面所有权转移和分区管理。我的 hypervisor 就填在 S-EL2 这个位置。

## 两个 Hypervisor，一颗芯片

这是大多数 hypervisor 项目不需要处理的问题：**共存**。

pKVM 和我的 SPMC 在同样的 4 个物理 CPU 上启动，各管各的世界。启动链：

```
TF-A BL1 (ROM) → BL2 (loader) → BL31 (SPMD @ EL3)
    → BL32 (我的 SPMC @ S-EL2，启动 SP1/SP2/SP3)
    → BL33 (pKVM @ NS-EL2 → Linux @ NS-EL1)
```

当 pKVM 的 Linux 客户机想和安全分区通信时，消息穿越四个异常级别和两次世界切换：

```
Linux (NS-EL1) → SMC → pKVM (NS-EL2) → SMC → SPMD (EL3)
    → ERET → SPMC (S-EL2) → ERET → SP1 (S-EL1)
    → SMC → SPMC → SMC → SPMD → ERET → pKVM → ERET → Linux
```

验证方式：Linux 通过 FF-A DIRECT_REQ 发送 `x4=0xBBBB`，SP1 加上 `0x1000`，Linux 读回 `0xCBBB`。一次往返，四个特权级别，两次世界切换。

让这一切工作起来需要解决几个在单 hypervisor 场景中不会遇到的问题：

**SPMD 是按 CPU 独立的。** TF-A 的 SPMD 为每个物理 CPU 维护独立状态。当 pKVM 通过 PSCI 启动从核时，每个从核进入 S-EL2 后必须调用 `FFA_MSG_WAIT` 完成握手。如果任何一个 CPU 跳过这个握手，SPMD 就会阻塞整个 PSCI 启动流程。这在任何文档中都没有记载——只有 TF-A 的源码里有。

**S-EL2 Stage-1 MMU 和 NS 位。** Secure World 有自己的物理地址空间。S-EL2 在 MMU 关闭时写 `0x42a16000`，命中的是 Secure DRAM；pKVM 的 RX 缓冲区在同一地址的 Non-Secure DRAM。**同一个地址，不同的内存。** 我必须启用 S-EL2 Stage-1 MMU，把所有 Normal World DRAM 标记为 `NS=1`。

**跨 CPU 缓存一致性。** pKVM 在 CPU 0 写描述符到 TX 缓冲区，发 SMC。SPMD 可能把调用路由到 CPU 2 的 S-EL2——这个 CPU 的 L1 缓存可能是旧的。即使加了 `DSB SY` 屏障，我还是必须把描述符拷贝到本地栈缓冲区再解析。直接读跨世界缓冲区会因为脏缓存导致 30% 的崩溃率。

## 为什么用 Rust

安全分区的生命周期是一个状态机：Reset → Idle → Running → Blocked → Preempted。C 里面这是一个 int 加一堆散落的 assert。Rust 里：

```rust
enum SpState { Reset, Idle, Running, Blocked, Preempted }
```

当我为 SP-to-SP 消息链添加 Blocked → Preempted 转换时，`match` 强制我处理每一种情况，编译期就抓到了两个 bug。

整个项目只有一个依赖：`fdt = "0.1.5"`（DTB 解析）。页表、GIC 模拟、virtio 驱动、SPMC 事件循环全部手写。`alloc` crate 配合 bump allocator 提供 `Box` 和 `Vec`。枚举分发替代 trait object，实现零开销 MMIO 路由。

## 技术细节

### Stage-2 页表的页面所有权

ARM 的 Stage-2 翻译把客户物理地址映射到真实物理地址。我用 PTE 的软件自定义位来跟踪页面所有权：

```
PTE bits [56:55]:
  00 = Owned          (页面属于此 VM)
  01 = SharedOwned    (已共享出去，发送方保留所有权)
  10 = SharedBorrowed (从其他 VM/SP 映射来的)
  11 = Donated        (不可撤销的转移)
```

这和 pKVM 的模型完全一致。VM 0 和 SP1 共享一个页面时：验证所有权（SW bits = `00`），设为 SharedOwned（`01`）+ 只读，映射到 SP1 的 Secure Stage-2 为 SharedBorrowed（`10`）。回收时：验证 SP1 已放弃，恢复为 Owned + 读写。

### SP-to-SP 消息和环检测

安全分区之间可以互发消息。SP1 → SP3 → SP2 → 响应。SPMC 路由每一跳，并通过 CallStack 检测环路：SP1 → SP3 → SP1 返回 `FFA_BUSY`，否则死锁。

难点在于抢占。Normal World 中断到达时 SP3 正在链路中间运行，SPMC 必须把 SP3 从 Running 转为 Preempted，SP1 从 Blocked 转为 Preempted（链式抢占），返回 `FFA_INTERRUPT`。Normal World 之后调用 `FFA_RUN` 恢复整个链路。

### `handle_sp_exit()` 循环

这是 SPMC 的核心。SPMC 派发请求给 SP 后，SP 运行到 trap——但 trap 不一定是响应。可能是内存操作、日志、或调用另一个 SP：

```rust
loop {
    enter_guest();  // ERET 到 S-EL1
    let exit = decode_exit();
    match exit {
        FFA_MSG_SEND_DIRECT_RESP => return response,
        FFA_MEM_RETRIEVE_REQ    => { 本地处理; 重入 SP },
        FFA_MEM_RELINQUISH      => { 本地处理; 重入 SP },
        FFA_MEM_SHARE           => { 记录共享; 重入 SP },
        FFA_CONSOLE_LOG         => { 输出到 UART; 重入 SP },
        FFA_MSG_SEND_DIRECT_REQ => { 派发到目标 SP; 重入 },
        _ => return error,
    }
}
```

SP 不知道它的 RETRIEVE_REQ 是被 SPMC 本地处理的。它做一次 SMC，拿到结果，继续执行。这就是端到端内存共享能工作的原因。

## 四个让我失眠的 Bug

### 1. 沉默的 SIMD 陷阱

第 4 周。SPMC 在 release 模式正常启动，debug 模式在第一个 `read_volatile` 就卡住。没有输出，没有异常，什么都没有。

GDB 调试几小时后发现：CPU 卡在 EL3 异常处理器里，ESR 显示 FP/SIMD 陷阱。但我的代码没有用浮点。

原来 Rust debug 模式的代码生成会为 `read_volatile` 的对齐检查发出 NEON 指令（`cnt v0.8b, v0.8b`——SIMD popcount）。TF-A 默认的 `CPTR_EL3.TFP=1` 会从所有异常级别陷入 EL3，而 EL3 的处理器没有准备好处理这个陷阱，于是永远循环。

**修复**：一个构建标志（`CTX_INCLUDE_FPREGS=1`）。**教训**：在操作系统之下，编译器的代码生成就是硬件约束。

### 2. NS 位和隐形写入

第 8 周。`PARTITION_INFO_GET` 从 BL33 测试工具调用时完美工作。SPMC 往调用方的 RX 缓冲区写 SP 描述符，调用方读回来，完全正确。

然后 pKVM 调用同一个函数。同样的代码路径，同样的描述符格式。pKVM 读到的是……全零。

写入成功了（没有异常），地址正确（GDB 验证过），但数据不在那里。

ARM 有**两个物理地址空间**。S-EL2 在 MMU 关闭时，所有内存访问走 Secure 物理地址空间。pKVM 的缓冲区在 `0x42a16000` 的 Non-Secure DRAM。写入命中的是 `0x42a16000` Secure，pKVM 读的是 `0x42a16000` Non-Secure。**同一个地址，不同的内存。**

在 QEMU 里，同一个地址字面上有两倍的内存，由一个 bit 选择。我做了这么多年 ARM 开发，直到这一刻才真正理解：Secure/Non-Secure 是**物理地址空间的分裂**，不只是权限模型。

### 3. 脏缓存和幽灵 Data Abort

第 11 周。pKVM 的 MEM_SHARE 有 70% 的概率成功。另外 30%，SPMC 在 `0x240f` 这种明显无效的地址上崩溃。

`addr2line` 定位到 `parse_mem_region`——描述符解析器。`composite_offset` 字段本该是 80，但读出来是垃圾值。SPMC 用 `base + 垃圾值` 做指针运算，Data Abort。

描述符在 pKVM 的 TX 缓冲区里——Normal World DRAM。pKVM 在 CPU 0 写入，发 SMC，SPMD 切换到 CPU 2 的 S-EL2。虽然 SMC 对发出方 CPU 是屏障，但**接收方 CPU 可能还有旧的 L1 缓存行**。

加了 `DSB SY`（全系统数据同步屏障）。还是崩。最终的修复：把整个描述符拷贝到本地栈缓冲区再解析。

```rust
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)); }
let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8, local_buf.as_mut_ptr(), total_length,
    );
}
let parsed = parse_mem_region(local_buf.as_ptr(), total_length);
```

这样即使拷贝捕获了旧数据，`parse_mem_region` 的边界检查也会干净地拒绝，而不是追着野指针跑进 Secure 内存。崩溃率从 30% 降到 0%。

### 4. SPMD 是按 CPU 隔离的

第 7 周。pKVM 在 CPU 0 正常启动。从核全部卡住。

FF-A 规范描述了 SPMC 初始化流程，但关于从核几乎什么都没说。读了 TF-A 的 `spmd_cpu_on_finish_handler()` 才发现：SPMD 为每个物理 CPU 维护**完全独立的状态**。每个进入 S-EL2 的从核必须调用 `FFA_MSG_WAIT` 完成握手。不然 SPMD 就不会完成 PSCI CPU_ON，Normal World 的从核也永远起不来。

**教训**：FF-A 规范告诉你 *what*，TF-A 的源码告诉你 *how*。

## 数据

| 指标 | 数值 |
|------|------|
| Rust 代码 | 26,000 行（96 个文件） |
| ARM64 汇编 | 3,400 行（9 个文件） |
| 单元测试断言 | 457 |
| BL33 集成测试 | 20/20 |
| pKVM 端到端测试 | 35/35 |
| 依赖 | 1 个（`fdt` crate） |
| 开发时间 | ~10 周（一个人） |
| 二进制大小 | 230KB（release，SPMC） |

## 试试看

```bash
git clone https://github.com/willamhou/hypervisor
cd hypervisor
make run          # 34 个测试套件，QEMU 上 ~5 秒
make run-linux    # 启动 Linux 6.12 到 shell
```

`make run-spmc` 和 `make run-pkvm-ffa-test` 需要构建 TF-A 和 AOSP 内核（通过 Docker，首次 ~30 分钟）。详见 [README](https://github.com/willamhou/hypervisor)。

---

英文博客（含更多技术细节）：[willamhou.github.io/hypervisor](https://willamhou.github.io/hypervisor/)

GitHub：[github.com/willamhou/hypervisor](https://github.com/willamhou/hypervisor)

欢迎提问和 star。
