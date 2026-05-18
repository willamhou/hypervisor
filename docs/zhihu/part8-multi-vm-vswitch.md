# 两台 Linux VM 在一颗 ARM CPU 上互 ping —— 一根 200 行的 L2 vSwitch 接起来

> 上一篇是 4 个 vCPU 跑一台 Linux。这一篇是 4 个 vCPU 跑一台 Linux × 2，加一根 200 行的 L2 vSwitch。**两台 VM 共用一颗物理 CPU，ICMP 互通。**整个 multi_vm feature 加 vSwitch 大概多了 600 行代码，其中 vSwitch + RX ring 占 200。

---

## 写在前面

前面几篇讲的 hypervisor 一直是一台 VM。Part 3 让 Linux 启起来，Part 4-7 都是围绕这一台 VM 展开的——SMP 调度、GIC 虚拟化、virtio-blk、TrustZone NS bit、跨核 cache、bare-metal Rust 的坑。

这一篇换主题：**多 VM**。

具体来说，这版的 hypervisor（`make run-multi-vm`）支持：
- 同一颗物理 CPU 上时间片切两台 Linux VM
- 每台 VM 各 4 个 vCPU
- 每台 VM 各自的 256MB 内存、各自的 virtio-blk 磁盘
- 一根虚拟以太网线（virtio-net）把两台 VM 接上同一个虚拟交换机
- 两台 VM 互发 ICMP，通

整个 multi-VM 这条分支加起来 600 行左右，其中 vSwitch + RX ring 占 200，其余是把单 VM 的 globals 改造成 per-VM。这 600 行里大部分时间不是花在写功能，而是花在搞清楚一件事：**怎么让"两台 VM"这件事在硬件层面真的隔离开**。

---

## VM 隔离的三件套

让两台 VM 不互相干扰的最低成本是：每台 VM 看见的"硬件世界"是独立的。在我的 hypervisor 里这意味着三件事：

### 1. 每台 VM 一份 `VmGlobalState`

之前所有的全局状态——pending SGI/SPI bitmap、当前在跑的 vCPU、online mask、PSCI 启动队列——都是按"只有一台 VM"写的，全是平铺的全局变量。多 VM 一来，每台 VM 都得有自己的一份：

```rust
// src/global.rs
pub struct VmGlobalState {
    pub pending_sgis: [AtomicU32; MAX_VCPUS],
    pub pending_spis: [AtomicU32; MAX_VCPUS],
    pub terminal_exit: [AtomicBool; MAX_VCPUS],
    pub vcpu_online_mask: AtomicU64,
    pub current_vcpu_id: AtomicUsize,
    pub pending_cpu_on: PendingCpuOn,
    pub preemption_exit: AtomicBool,
}

pub static VM_STATE: [VmGlobalState; MAX_VMS] =
    [VmGlobalState::new(), VmGlobalState::new()];

pub static CURRENT_VM_ID: AtomicUsize = AtomicUsize::new(0);

#[inline]
pub fn current_vm_state() -> &'static VmGlobalState {
    &VM_STATE[CURRENT_VM_ID.load(Ordering::Relaxed)]
}
```

异常处理路径里的"找当前 VM 的 pending SGI"原本是 `PENDING_SGIS[vcpu]`，现在变成 `current_vm_state().pending_sgis[vcpu]`——一个 `CURRENT_VM_ID` 的索引就把所有 per-VM 的状态都切到正确的那份。

切 VM 的时候只动一次 `CURRENT_VM_ID`，从这一刻起所有 `current_vm_state()` 调用都会索引到新 VM 那份 per-VM 状态——一条原子 store 搞定，没有数据拷贝、没有锁。

### 2. 每台 VM 一份 `DeviceManager`

第二件事是 MMIO 设备。两台 VM 都有自己的 UART、GIC、virtio-blk、virtio-net——都是按"VM 内的视角"虚拟化的，guest 看起来都是同一个地址（比如 `0x09000000` 是 UART），但 hypervisor 这边必须知道是哪台 VM 在访问。

```rust
// src/global.rs（节选；省略了 const init 和 cfg 分支）
pub static DEVICES: [GlobalDeviceManager; MAX_VMS] = [...];

pub fn current_devices() -> &'static GlobalDeviceManager {
    &DEVICES[CURRENT_VM_ID.load(Ordering::Relaxed)]
}
```

异常处理里 MMIO 解码出地址之后，dispatch 是从 `current_devices()` 路由的，`CURRENT_VM_ID` 切了之后所有 dispatch 自动指到正确的那份设备状态。VM 0 的 virtio-blk 队列和 VM 1 的 virtio-blk 队列各走各的，互不干扰。

### 3. 每台 VM 一份 Stage-2 + VMID

最关键的一件——内存隔离。

ARM Stage-2 翻译让每台 VM 有自己的页表，guest 的"物理地址"经过 Stage-2 翻译才到真实的物理地址。两台 VM 的 Stage-2 必须独立：VM 0 的 `0x48000000` 映射到主机的 0x48000000，VM 1 的 `0x68000000` 映射到主机的 0x68000000。

但有个细节——TLB。CPU 不会因为 Stage-2 表换了就自动 flush TLB；如果不打 VMID（Virtual Machine Identifier）标签，VM 0 切到 VM 1 时 VM 1 会读到 VM 0 的 TLB 残留。

ARM 的解法是 VMID：每台 VM 有一个 16 位 ID，写在 `VTTBR_EL2` 的高 16 位里，硬件会用 VMID 标记 TLB 条目。VM 切换时只换 `VTTBR_EL2`（包括 VMID 部分），TLB 自动按 VMID 区分。

```rust
// src/arch/aarch64/mm/mmu.rs
pub fn new_with_vmid(page_table_addr: u64, vmid: u16) -> Self {
    let vtcr = VTCR_T0SZ_48BIT
        | VTCR_SL0_LEVEL0
        | VTCR_IRGN0_WB
        | VTCR_ORGN0_WB
        | VTCR_SH0_INNER
        | VTCR_TG0_4KB
        | VTCR_PS_48BIT;

    // VTTBR_EL2: VMID 在 bits[63:48]，页表基址在 bits[47:1]
    let vttbr = (page_table_addr & 0x0000_FFFF_FFFF_FFFE) | ((vmid as u64) << 48);

    Self { vttbr, vtcr }
}
```

VM 0 用 VMID=0，VM 1 用 VMID=1。切 VM 的时候 `vm.activate_stage2()` 写一次 `VTTBR_EL2`，硬件就按新 VMID 路由 TLB。

**第一次踩坑**：VMID 必须在 `VTTBR_EL2` 的 `[63:48]`，不是低位。我第一次写错了把 VMID 放低位，编译器啥都没说，跑起来 VM 1 直接读到 VM 0 的内存——这是 ARM ARM 里的细节，写错的代价是"看起来跑通了，但内存被搞乱了"，下层完全没报错。靠对照规范才发现。

---

## 调度：两层 round-robin

有了这三件套，调度就简单了。`run_multi_vm()` 是个外层循环，里面再调用单 VM 的 `run_one_iteration()`：

```rust
// src/vm.rs（节选；省略 logging、不健康分支、终止条件）
pub fn run_multi_vm(vms: &mut [Vm]) {
    for vm in vms.iter_mut() {
        vm.state = VmState::Running;
        crate::global::vm_state(vm.id)
            .vcpu_online_mask
            .fetch_or(1, Ordering::Release);
    }

    let mut done = [false; MAX_VMS];
    loop {
        let mut all_done = true;
        for vm in vms.iter_mut() {
            if done[vm.id] { continue; }
            all_done = false;

            // 切到这台 VM 的上下文
            crate::global::CURRENT_VM_ID.store(vm.id, Ordering::Release);
            vm.activate_stage2();

            // 跑一轮：选 vCPU、运行、处理 exit
            if vm.run_one_iteration() {
                done[vm.id] = true;
                vm.state = VmState::Ready;
            }
        }
        if all_done { break; }
    }
}
```

外层 round-robin 在两台 VM 之间，内层 round-robin 在每台 VM 的 4 个 vCPU 之间。每次外层换 VM 只做两件事：`CURRENT_VM_ID.store()` + `activate_stage2()`——前者把所有 per-VM 全局状态切过去，后者把 Stage-2 + VMID 切过去。十几条指令的开销。

时间片粒度由 `run_one_iteration()` 的退出策略决定：vCPU 主动 WFI、抢占定时器到期、PSCI 调用、普通 IRQ/MMIO trap 处理完之后的 yield——任何一个都让一轮结束，外层就有机会切到下一台 VM。

---

## 二层虚拟交换机：让两台 VM 看见对方

到这里两台 VM 已经能各自跑 Linux 了，但它们彼此还看不见。互 ping 需要把它们接到同一个虚拟网络。

我没用 TUN/TAP（那是 host 联网的方案），也没接 host 的 bridge。直接在 hypervisor 里写了个 200 行的二层交换机：

```rust
// src/vswitch.rs（节选）
const MAC_TABLE_SIZE: usize = 16;

struct MacEntry {
    mac: [u8; 6],
    port_id: usize,
    valid: bool,
}

pub struct VSwitch {
    mac_table: [MacEntry; MAC_TABLE_SIZE],
    mac_count: usize,
    port_count: usize,
}

impl VSwitch {
    fn forward(&mut self, src_port: usize, frame: &[u8]) {
        if frame.len() < 14 { return; }

        let dst_mac = &frame[0..6];
        let src_mac = &frame[6..12];

        // 1. 学习：src_mac → src_port
        self.learn(src_mac, src_port);

        // 2. 广播/多播 → flood 所有除了源的端口
        if dst_mac[0] & 1 != 0 {
            self.flood(src_port, frame);
            return;
        }

        // 3. 单播：查表 → 找到就发那个端口；找不到就 flood
        ...
    }
}
```

逻辑就是教科书上的 L2 switch：MAC 学习 + 广播 flood + 单播按表转发。16 项 MAC table、2 个端口（VM 0 + VM 1），跑得开。

每台 VM 的 virtio-net 设备是发送端：guest 把以太网帧写到 virtio TX queue，hypervisor 的 virtio-net 后端拿到帧、调 `vswitch_forward(src_port, frame)`。vSwitch 在 forward 路径里**只做一件事**：把帧塞进目标端口的 RX ring（`PORT_RX[dst].store(frame)`）。它不直接 inject 到 guest 的 RX queue，因为那一步必须等到目标 VM 真的被调度才行。

### RX 路径：per-port SPSC ring + 延迟注入

把帧塞回 guest 这一步比较麻烦，因为 forward 是在 EL2 异常上下文里发生（处理 virtio TX MMIO 通知），但 receive 必须等到那台 VM 真的被调度——这时候我们才能在 EL2 安全地走 `inject_rx()` + `inject_spi(49)` 把帧塞进 guest 的 RX 队列。

中间需要一个 buffer。我用 per-port SPSC（single-producer single-consumer）ring buffer：

```rust
// src/vswitch.rs
pub static PORT_RX: [NetRxRing; MAX_PORTS] = [
    NetRxRing::new(), // VM 0 的入向帧
    NetRxRing::new(), // VM 1 的入向帧
];
```

producer 是 vSwitch（forward 路径里 `PORT_RX[dst].store(frame)`）。consumer 是 `run_one_iteration()` 主循环里的 `drain_net_rx()`，每次调度回某台 VM 之前调用：从 `PORT_RX[vm_id].take()` 取出累积的帧，然后调用 `DEVICES[vm_id].inject_net_rx()` 把帧写进那台 VM 的 virtio-net RX 描述符链 + 注入 SPI 49（virtio-net IRQ）。

SPSC 的好处是 producer/consumer 之间不需要锁——atomic head/tail 加 release/acquire ordering 就够了。代价是约束严：必须保证全局只有一个 producer 和一个 consumer。在我的设计里这天然成立——vSwitch forward 单线程跑（异常上下文），drain 也是单线程跑（主循环）。

### 实测

VM 0 ping VM 1：

```text
$ ping 10.0.0.2
PING 10.0.0.2 (10.0.0.2): 56 data bytes
64 bytes from 10.0.0.2: seq=0 ttl=64 time=8.234 ms
64 bytes from 10.0.0.2: seq=1 ttl=64 time=2.117 ms
64 bytes from 10.0.0.2: seq=2 ttl=64 time=2.054 ms
```

第一个 RTT 偏高是 ARP 解析（VM 0 不知道 VM 1 的 MAC，先广播 ARP request、等 reply、cache 命中再 ICMP）。后面稳定在 ~2ms——QEMU + 时间片切换的开销。

---

## 第二个坑：vSwitch 接口为什么不能依赖 `DEVICES`

vSwitch 的 forward 路径**不能去碰 `DEVICES`**——这条约束写进了它的接口设计：`vswitch_forward()` 只动 `PORT_RX`（store 入向帧）和它自己的 MAC table，绝不去查任何外部设备状态。RX 注入推迟到 `drain_net_rx()`，那时调用方早已退出 `DEVICES` 的访问上下文，单独按 `DEVICES[vm_id]` 拿一次再注入。

为什么这条约束这么硬？因为它要同时兼容两条 feature 分支——`multi_vm`（两台 VM 时间片）和 `multi_pcpu`（一台 VM 跑在 4 个物理 CPU 上）。后者下 `DEVICES` 是 `SpinLock` 保护的（多 pCPU 真的可能同时访问），forward 路径已经在持有 `DEVICES` 锁的上下文里跑（virtio TX 处理）；此时如果 vSwitch 自己再去查 `DEVICES`，立刻 reentrant deadlock。前者下虽然 `DEVICES` 是 `UnsafeCell`、不会自锁，但同样的 pattern 也会让 forward 路径直接看到 inconsistent 的设备状态。

接口设计原则一句话：**任何在异常上下文里跑的回调，如果可能被某个 lock 包着，就不能自己再去拿同名 lock，哪怕是间接的**。vSwitch 必须无锁、或者只锁自己的数据结构（`PORT_RX` 用 atomic 不需要锁，`MAC table` 在 reset/forward 之间是单线程访问）。

裸机里这条约束反复出现：`inject_spi()` 不能拿 `DEVICES` 锁（已经在 IRQ handler 持有的状态里）、`flush_pending_spis_to_hardware()` 同理、`drain_net_rx()` 必须放到 lock 外面。设计接口时如果不一开始就划清楚谁能在异常上下文跑、不能拿哪些锁，每加一个新设备就要改一次。

---

## 数字

| 模块 | 代码行数 | 关键挑战 |
|---|---|---|
| `VmGlobalState` 改造 | ~80 | 把 flat globals 重构成 per-VM array |
| `run_multi_vm()` 调度 | ~40 | 外层 VM round-robin + 内层 vCPU |
| `Stage2Config::new_with_vmid()` | ~20 | VMID 编码到 VTTBR[63:48] |
| `VSwitch` (vswitch.rs) | ~150 | MAC learning + flood + unicast |
| `NetRxRing` SPSC | ~80 | 无锁 ring buffer |
| virtio-net 后端 | ~250 | RX/TX queue + virtio header |

合计约 **600 行新代码**（不含测试），其中 200 行是 vSwitch + RX ring，剩下是把单 VM 改造成 multi-VM。30+ 个新单元测试覆盖每个模块。

---

## 这一步通了，已经存在的另一条分支

走到 multi-VM + vSwitch，hypervisor 已经有了一台"小 cluster"的样子。但 `multi_vm` 这条分支有几个明显的限制：

1. **单 pCPU 时间片**：两台 VM 还在抢一颗物理 CPU。仓库里另一条 `multi_pcpu` 分支让 4 个 vCPU 真的跑在 4 个 pCPU 上，1:1 affinity——实际上和 `multi_vm` 是互斥的两条 feature flag，对应不同的运行模型。
2. **L2 only**：vSwitch 是二层的，没有 IP 路由、没有 NAT。要互联网得另外接 host bridge。
3. **MAC table 16 项**：够两台 VM 玩，到 10+ VM 时表就不够了——不过那时候应该换 hash 表。

下一篇展开 `multi_pcpu` 这条已实现的分支：让 4 个 vCPU 各自跑在独立的物理 CPU 上、1:1 affinity、PSCI 真的发出 SMC `cpu_on` 唤醒物理核心，TPIDR_EL2 替代全局 vCPU context 指针，还有跨 pCPU 的物理 GICD_IROUTER 编程。

---

代码：<https://github.com/willamhou/hypervisor>

博客：<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第八篇。之前的文章：*

- *Part 0a: [为什么写一个 Hypervisor](./part0a-why.md)*
- *Part 0b: [AI 辅助系统编程](./part0b-ai-workflow.md)*
- *Part 1: [从零到 "Hello from EL2!"](./part1-first-boot.md)*
- *Part 2: [陷入-模拟-恢复](./part2-trap-emulate-resume.md)*
- *Part 3: [让 Linux 启动](./part3-linux-boot.md)*
- *Part 4: [裸机四大坑](./part4-war-stories.md)*
- *Part 5: [Rust enum 状态机的真相](./part5-enum-state-machine.md)*
- *Part 6: [TrustZone 的 NS 位](./part6-trustzone-ns-bit.md)*
- *Part 7: [bare-metal Rust 三个坑](./part7-bare-metal-rust-pitfalls.md)*
