# 四个 CPU、一块磁盘 — 让 Linux 启动

## 写在前面

Part 0a 说过一句话：核心循环只占 5% 的工作量。Part 2 就是那 5%——陷入-模拟-恢复，一个周末搞定。

这篇是那另外 95%。

让 Linux 6.12 启动到 BusyBox shell，需要正确模拟 GICv3 中断控制器（几十个寄存器、三种不同的 MMIO 区域、每个 CPU 独立的 redistributor）、实现 virtio-blk 磁盘设备、写一个能在 4 个 vCPU 之间切换的调度器、处理 PSCI 电源管理、让 guest 能通过 CPU_ON 唤醒 secondary CPU……

每一个子系统单拎出来都不复杂。但它们必须同时正确地工作，Linux 才能启动。差任何一个，内核就挂在某个阶段——通常是沉默的挂起，没有任何错误信息。

时间线：从 2 月 7 日第一次启动 Linux 到 2 月 14 日完成 GIC 全虚拟化，大约一周。这一周是整个项目代码密度最高的阶段。

---

## DTB：Linux 怎么知道硬件长什么样

Linux 内核不知道自己跑在什么硬件上。启动时，bootloader 把一个 DTB（Device Tree Blob）的地址放在 x0 寄存器里，内核解析 DTB 来发现：UART 在哪、GIC 在哪、内存有多大、几个 CPU。

我们的 hypervisor 也需要做同样的事。QEMU 启动时会生成一个描述硬件的 DTB，hypervisor 在初始化时解析它：

```rust
// src/dtb.rs — 用 fdt crate（zero-copy、no-alloc）解析 DTB
fn parse_host_dtb(dtb_addr: usize) -> Option<PlatformInfo> {
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_addr as *const u8).ok()? };

    // UART: arm,pl011 compatible → uart_base
    if let Some(uart_node) = fdt.find_compatible(&["arm,pl011"]) { ... }

    // GIC: arm,gic-v3 → gicd_base, gicr_base
    if let Some(gic_node) = fdt.find_compatible(&["arm,gic-v3"]) { ... }

    // RAM: /memory 节点
    // CPU count: /cpus 节点
    ...
}
```

为什么不硬编码？因为 QEMU 不同版本的地址可能不同，而且我们之后要在真实硬件上跑（pKVM 集成）。DTB 解析是一次性成本，换来的是平台无关性。

`fdt` crate 的设计值得一提：0.1.5 版本，zero-copy，不需要堆分配。在 `no_std` 裸机环境下，你可能还没初始化堆分配器就需要知道 GIC 在哪。这个 crate 直接在 DTB 的原始字节上操作，完美适配。

---

## GICv3 虚拟化：中断是最难的部分

GICv3 是 ARM 平台的中断控制器。它有三个组件，每个的虚拟化策略不同：

| 组件 | 地址 | 作用 | 虚拟化策略 |
|------|------|------|-----------|
| GICD | 0x08000000 | 全局中断使能、路由 | 写穿（write-through）|
| GICR × N | 0x080A0000+ | 每 CPU 的中断配置 | 纯 trap-and-emulate |
| ICC 系统寄存器 | MSR/MRS | 中断确认、EOI | 硬件虚拟接口 |

**三种策略，三种理由。**

### GICD：写穿

GICD 管全局中断——哪些中断被使能、路由到哪个 CPU。Guest 写 GICD，我们需要两件事都做：更新自己的 shadow state（后面查询用），**同时写到真实的物理 GICD**。

```rust
// src/devices/gic/distributor.rs
fn write(&mut self, offset: u64, value: u64, size: u8) -> bool {
    // 只读寄存器不转发
    let forward = !matches!(offset, GICD_TYPER | GICD_IIDR | GICD_PIDR2);

    if forward {
        // 写穿到物理 GICD — EL2 直接访问物理地址，绕过 Stage-2
        unsafe {
            core::ptr::write_volatile(
                (gicd_base() + offset) as *mut u32,
                value as u32,
            );
        }
    }

    // 更新 shadow state
    match offset {
        GICD_CTLR => self.ctlr = value as u32,
        GICD_ISENABLER_BASE..=GICD_ISENABLER_END => {
            let reg = ((offset - GICD_ISENABLER_BASE) / 4) as usize;
            self.enabled[reg] |= value as u32;
        }
        GICD_IROUTER_BASE..=GICD_IROUTER_END => {
            let idx = ((offset - GICD_IROUTER_BASE) / 8) as usize;
            self.irouter[idx] = value;
        }
        _ => {}
    }
    true
}
```

为什么可以写穿？因为在 single-VM 场景下，guest 要控制的物理中断就是真实的物理中断。GICD 是系统全局唯一的，没有"虚拟 GICD"——我们只需要拦截一下确保不越界，然后直接写硬件。

一个关键细节：EL2 访问 GICD 的物理地址时**不经过 Stage-2 翻译**。这是 ARM 架构的设计——EL2 的数据访问使用 Stage-1 翻译（或者绕过，取决于配置），不受 guest 的 Stage-2 约束。

### GICR：纯 trap-and-emulate

GICR 是 per-CPU 的——每个 CPU 有自己的 redistributor 帧（128KB）。Guest 写 GICR 来配置自己 CPU 的中断。

与 GICD 不同，GICR 不能写穿。原因很微妙：在 4 个 vCPU 跑在 1 个 pCPU 上的场景（我们的 single-pCPU 模式），guest 的 vCPU 2 写 GICR[2] 实际上应该更新 vCPU 2 的虚拟状态，而不是物理 CPU 2 的真实 GICR——物理上只有一个 CPU。

所以 GICR 必须纯软件模拟：

```
Guest 写 GICR[2] offset=ISENABLER0, value=(1<<27)  // 使能 vtimer
  ↓
Stage-2 fault（GICR 地址被 unmap 了）
  ↓
VirtualGicr::write(cpu_id=2, offset=0x100, value=(1<<27))
  ↓
shadow_state.gicr[2].isenabler0 |= (1<<27)
```

那 GICR 的地址怎么触发 Stage-2 fault？**把它从 Stage-2 页表里拆出来 unmap。**

GICR 每个 CPU 占 128KB（两个 64KB 帧）。我们先用 2MB block 映射了整个 GIC 区域（包含 GICD + GICR），然后把 GICR 对应的 4KB 页逐个 unmap：

```rust
// 把 2MB block 拆成 512 个 4KB 页，然后 unmap GICR 的那些页
for cpu in 0..num_cpus {
    let base = gicr_rd_base(cpu);
    for page in 0..32u64 {  // 32 × 4KB = 128KB
        mapper.unmap_4kb_page(base + page * 4096)?;
    }
}
```

这就是 Part 2 提到的"先粗后细"——2MB block 覆盖大范围，然后拆到 4KB 精度来控制特定地址。ARM 称之为 **break-before-make**：先 invalidate 旧的 2MB entry，TLB flush，然后写新的 L3 page table。

### List Register：硬件帮你注入中断

GICv3 提供了 4 个 List Register（ICH_LR0-LR3_EL2），这是硬件为虚拟化专门设计的接口。Hypervisor 往 LR 里写一个中断号，硬件在 ERET 回 guest 时自动触发这个中断——不需要修改 guest 的任何状态。

```rust
// 找到空闲的 LR slot，写入待注入的 SGI
for lr in arch.ich_lr.iter_mut() {
    if (*lr >> LR_STATE_SHIFT) & LR_STATE_MASK == 0 {
        // LR 格式: [63:62]=State | [61:48]=Priority | [31:0]=INTID
        *lr = (LR_STATE_PENDING << LR_STATE_SHIFT)
            | LR_GROUP1_BIT
            | ((priority as u64) << LR_PRIORITY_SHIFT)
            | (intid as u64);
        break;
    }
}
```

对 vtimer（INTID 27）有一个特殊处理：设置 `HW=1` bit。这启用了物理-虚拟 EOI 联动——当 guest 写 `ICC_EOIR1_EL1` 结束虚拟中断时，硬件自动结束对应的物理中断。没有这个 bit，物理 timer 永远不会被清除，中断会不停触发。

### ICC_SGI1R_EL1：bit field 的坑

Guest 用 `msr ICC_SGI1R_EL1, x0` 发送 SGI（Software Generated Interrupt，即 IPI）。这条指令被 ICH_HCR_EL2.TALL1=1 trap 到 EL2，hypervisor 需要解码寄存器值，找出目标 CPU 和中断号。

ARM 规范里 ICC_SGI1R_EL1 的 bit field：

```
[55:48] Aff3
[47:44] RS
[40]    IRM (Interrupt Routing Mode)
[39:32] Aff2
[27:24] INTID
[23:16] Aff1
[15:0]  TargetList
```

注意这些字段**不是连续的**，中间有 gap。TargetList 在 [15:0]，INTID 在 [27:24]，Aff1 在 [23:16]。

CLAUDE.md 里有一行大写加粗的警告：

> **ICC_SGI1R_EL1 Bit Fields**: TargetList: bits [15:0] (NOT [23:16]). Aff1: bits [23:16] (NOT [27:24]). INTID: bits [27:24] (NOT [3:0]).

这行警告是写错之后加的。第一版代码把 TargetList 和 Aff1 的位置搞反了——因为直觉上你会觉得"目标列表"应该在更高位，"亲和组"在低位。但 ARM 不是这么设计的。

结果是 vCPU 0 发的 SGI 全部到了错误的目标 CPU。多核场景下 Linux 的内核同步依赖 IPI，IPI 路由错了 → 自旋锁超时 → 内核挂起。

修复是一行代码。定位花了大半天——因为症状是"内核偶尔卡死"，而不是"SGI 发错了"。

---

## Virtio-blk：Guest 怎么读磁盘

Linux 启动需要根文件系统。我们用 virtio-blk 提供一块虚拟磁盘。

Virtio 是一套为虚拟化设计的 I/O 规范。核心思路：guest 和 hypervisor 共享一段内存（virtqueue），guest 往里放请求，通知 hypervisor 处理。比网络协议高效得多——没有网络栈，就是 shared memory + doorbell。

```
Guest 内核                           Hypervisor
    │                                    │
    ├─ 往 virtqueue 放一个读请求          │
    │  (sector=100, len=4096)            │
    │                                    │
    ├─ 写 MMIO QueueNotify ──────────→  │
    │  (Stage-2 Data Abort)              │
    │                                   process_request()
    │                                    │
    │                                    ├─ 解析 descriptor chain
    │                                    ├─ copy_nonoverlapping(
    │                                    │    disk + sector*512,
    │                                    │    guest_buf,
    │                                    │    len)
    │                                    ├─ 写 status byte
    │                                    ├─ 更新 used ring
    │                                    └─ inject_spi(48)
    │                                    │
    │  ←──────── 中断 (SPI 48) ──────────┤
    ├─ 读 used ring，拿到数据             │
```

关键的一步是 `copy_nonoverlapping`——因为我们用的是 identity mapping，guest 的"物理地址"就是真实的物理地址。磁盘镜像被 QEMU 加载到 0x58000000，guest buffer 在 0x48000000 范围内，两者都在 hypervisor 的地址空间里直接可访问。不需要 IOMMU，不需要 DMA 引擎，就是 memcpy。

```rust
// src/devices/virtio/blk.rs
unsafe {
    core::ptr::copy_nonoverlapping(
        (self.disk_base + byte_offset) as *const u8,  // 磁盘镜像
        desc.addr as *mut u8,                          // guest buffer
        len as usize,
    );
}
```

这是 identity mapping 最大的好处之一。如果 GPA ≠ HPA，这里需要一次地址翻译——查 Stage-2 页表，把 guest buffer 的 IPA 转成 hypervisor 能用的 PA。多一层翻译，多一份复杂度。

### Virtqueue 数据结构

Virtio-mmio 的 virtqueue 由三部分组成：

- **Descriptor Table**: guest 填写的 I/O 请求（每个 descriptor 指向一段内存 + 长度 + 标志位）
- **Available Ring**: guest 告诉 hypervisor "我放了新请求"
- **Used Ring**: hypervisor 告诉 guest "我处理完了"

Guest 通过写 MMIO 寄存器 `QUEUE_NOTIFY` 来"按门铃"——这会触发 Stage-2 Data Abort，hypervisor 处理请求。处理完后 hypervisor 注入 SPI 48（virtio-blk 的中断号）通知 guest 去取结果。

整个 virtio-mmio 的 MMIO 寄存器空间只有 0x100 字节。其中最先被读的是偏移量 0 的 magic 值 `0x74726976`（ASCII "virt"）——这就是 Part 2 讲过的 HPFAR_EL2 bug 的受害者。当 MMIO 地址解析错误时，magic 读出来是 0，驱动报 "Wrong magic value"。

---

## SMP：4 个 vCPU 的调度

Linux 内核启动时只有 vCPU 0。当它需要更多 CPU，会通过 PSCI CPU_ON 调用请求 hypervisor 启动 secondary vCPU。

### PSCI CPU_ON

Guest 执行 `smc #0`，x0 = PSCI_CPU_ON，x1 = target_cpu，x2 = entry_point。Hypervisor 收到后，不能立刻启动——因为当前正在 handle_exception 里处理 vCPU 0 的 SMC exit。所以我们把请求放进一个队列：

```rust
// 异常处理里：把 CPU_ON 请求入队
crate::global::current_vm_state().pending_cpu_on.request(
    target_cpu, entry_point, context_id,
);
```

回到 `run_one_iteration()` 主循环后，检查队列，真正创建并启动 secondary vCPU：

```rust
// 创建 vCPU，设置入口地址和 PSCI context_id
let mut vcpu = Vcpu::new(id, entry, 0);
vcpu.context_mut().gp_regs.x0 = ctx_id;  // PSCI 规范要求
vcpu.context_mut().spsr_el2 = SPSR_EL1H_DAIF_MASKED;

// 标记上线
vm_state.vcpu_online_mask.fetch_or(1 << id, Ordering::Release);
```

### 协作 + 抢占调度

有了 4 个 vCPU，需要一个调度器。我们用最简单的模型：round-robin，加一个 10ms 的硬件抢占定时器。

**协作式**：vCPU 执行 WFI（Wait For Interrupt）→ hypervisor trap → 标记为 Blocked → 调度下一个。这是最自然的切换点——vCPU 主动说"我没事干了"。

**抢占式**：如果一个 vCPU 一直在跑 CPU 密集型代码，不执行 WFI 怎么办？CNTHP（Hypervisor Physical Timer）每 10ms 触发一次 IRQ（INTID 26），强制 vCPU 退出。

```rust
// 在 2 个以上 vCPU online 时才启动定时器
let online = vm_state.vcpu_online_mask.load(Ordering::Relaxed);
let multi_vcpu = online != 0 && (online & (online - 1)) != 0;
if multi_vcpu {
    ensure_cnthp_enabled();
    arm_preemption_timer();  // CNTHP_CVAL = now + 10ms
}
```

`ensure_cnthp_enabled()` 是这里面最阴险的一个函数。

### 踩坑：定时器必须每次重新使能

CNTHP 的 INTID 是 26（PPI）。Guest 有能力通过 GICR 写来禁用 PPI 26——我们的 VirtualGicr 只更新 shadow state，但某些 GICR 写会被写穿到物理硬件（在 multi-pCPU 模式下）。

在 single-pCPU 模式下这不是问题（GICR 纯模拟）。但在 multi-pCPU 模式下，guest 的 GICR 写被物理执行了，PPI 26 被真正禁用，抢占定时器不再触发，只要有一个 vCPU 不执行 WFI，其它 vCPU 就永远不会被调度。

修复：每次 enter_guest 之前，都重新使能 INTID 26。

```rust
fn ensure_cnthp_enabled() {
    unsafe {
        // 强制写 GICR ISENABLER0 bit 26 = 1
        core::ptr::write_volatile(
            (sgi_base + GICR_ISENABLER0_OFF) as *mut u32,
            1 << 26,
        );
    }
}
```

这个函数在每次 vCPU 入口被调用。每秒几千次。一条 MMIO 写。代价微不足道，但如果不做，SMP 就是坏的。

---

## 完整启动序列

把所有东西串起来，Linux 从 QEMU 加载到 BusyBox shell 的完整路径：

```
QEMU 加载:
  0x40200000 — hypervisor 二进制
  0x47000000 — DTB (QEMU 生成)
  0x48000000 — Linux 6.12 kernel Image
  0x54000000 — initramfs (BusyBox)
  0x58000000 — virtio-blk 磁盘镜像
        │
        ▼
Hypervisor 启动 (EL2):
  1. 解析 DTB → UART, GIC, RAM, CPU count
  2. 初始化 GICv3 (GICD write-through, GICR unmapped)
  3. 建 Stage-2 页表 (0x40000000-0x58000000, 2MB blocks)
  4. 创建 vCPU 0, PC=kernel entry, x0=DTB addr
  5. enter_guest() → ERET
        │
        ▼
Linux 内核启动 (EL1):
  6. 解析 DTB → 发现 UART, GIC, 4 CPUs, 256MB RAM
  7. 使能 MMU (Stage-1)
  8. 初始化 GICv3 (GICD + GICR 写) ← 全部被 hypervisor 拦截
  9. PSCI CPU_ON → SMC → hypervisor 创建 vCPU 1/2/3
  10. 调度器 round-robin, 4 vCPU time-sliced on 1 pCPU
  11. virtio-mmio probe → 发现 virtio-blk
  12. mount initramfs → /init → BusyBox shell
        │
        ▼
[    1.234567] Welcome to BusyBox!
/ #
```

从 hypervisor 启动到 shell，在 QEMU 上大约 1-2 秒。

---

## AI 表现：GIC 是生成效率最高的模块

GIC 虚拟化有大量重复性的寄存器处理代码。GICD 有 CTLR、TYPER、ISENABLER0-31、ICENABLER0-31、IPRIORITYR0-255、IROUTER32-1019……每个都需要一个 read/write 分支，shadow state 更新逻辑雷同但偏移量不同。

这正是 AI 最擅长的场景。给 Claude 一张 GICv3 寄存器表和一个已经写好的 ISENABLER 处理代码作为模板，它能快速生成所有其它寄存器的处理逻辑。代码质量稳定——每个寄存器的逻辑都是"读偏移 → mask → 更新 shadow"的变体，很难出错。

**生成效率最高的代码**：GICD/GICR 寄存器 shadow state（~500 行）、virtio-mmio 寄存器分发（~200 行）、DTB 解析样板代码。

**AI 帮不上忙的地方**：

1. **调度器并发 bug**。`vcpu_online_mask` 必须在 boot 时包含 vCPU 0，否则抢占定时器永远不会启动（因为 `(online & (online-1)) == 0` 判断为"只有一个 vCPU"）。这个 bug 的症状是"第二个 vCPU 偶尔卡死"——AI 检查了调度逻辑、LR 注入、PSCI 实现，就是不看 online mask 的初始值。

2. **ICC_SGI1R_EL1 bit field**。AI 对 ARM 寄存器的 bit field 布局依赖训练数据，不会去翻最新的规范 PDF。给它一个错误的 bit field 假设，它会基于这个假设写出"正确"的解码代码——逻辑对，数据错。

3. **GICR 4KB unmap 的 break-before-make 时序**。必须先 invalidate 旧 entry → TLB flush → 写新 entry → TLB flush。少一步 flush 就是"偶尔能跑"的 bug。AI 每次都能写出结构正确的代码，但是否遵守了 BBM 协议需要人来核对。

---

## 数字

| 组件 | 代码行数 | 核心挑战 |
|------|---------|---------|
| VirtualGicd | ~400 | 写穿 + shadow state 一致性 |
| VirtualGicr | ~300 | 4KB unmap + per-vCPU 状态 |
| VirtioMmioTransport | ~250 | MMIO 寄存器状态机 |
| VirtioBlk | ~200 | descriptor chain 解析 |
| Scheduler | ~130 | round-robin + block/unblock |
| run_one_iteration | ~100 | 8 步调度循环 |
| DTB 解析 | ~170 | zero-copy, fallback defaults |
| PSCI CPU_ON | ~80 | 队列 + 延迟启动 |
| ICC_SGI1R_EL1 | ~80 | bit field 解码 |

合计约 1700 行新代码（不含测试），在一周内完成。其中 GIC 相关代码占了将近一半。

这周的 commit 历史读起来像一个加速的蒙太奇：2 月 7 日 "Boot Linux 6.12 under hypervisor"，2 月 10 日 "SMP: boot Linux 6.12 with 2 vCPUs"，2 月 11 日 "Phases 1-5: initramfs, GICD, virtio-blk, 4 vCPUs"，2 月 14 日 "GICD full trap-and-emulate with write-through"。

下一篇：两台 VM 互 ping。从 1 台 VM 到 2 台 VM 共享一块 CPU，再到 4 个物理 CPU 各跑各的。还有一个虚拟 L2 交换机。
