# 四个寄存器装下整个虚拟中断世界——GICv3 的 List Register

Guest 的 Linux 内核刚刚拿到一个 vtimer 中断。它的 IRQ handler 跑完,在 `EOIR_EL1` 写一次,告诉中断控制器"处理完了,下一个"。

这一笔写理应同时做两件事:**在虚拟世界里,把这条 vtimer 中断标成已处理**(让 guest 不再看见它 pending);**在物理世界里,把刚刚那条物理 vtimer 中断 deactivate**(让硬件可以再次触发它)。

guest 的 EOIR 写只是一条系统寄存器指令,既不进 hypervisor、也不直接戳物理 GIC。它走了一条很巧的路径:**ICH_LR<n>_EL2 里那一条 List Register 的 HW=1 让物理和虚拟 EOI 自动配对**——guest 在虚拟侧的一笔 EOI,硬件自动同步到物理侧 deactivation。

这一篇讲这条路径,以及让它成立的几个 GICv3 虚拟化设施:**ICH_HCR_EL2.En 启用虚拟接口、4 个 ICH_LR_EL2 装载待注入中断、ICC_CTLR_EL1.EOImode=1 把 priority drop 和 deactivation 拆成两步、ICH_HCR_EL2.TALL1=1 拦下 guest 的 ICC_SGI1R 写来做 IPI 仿真**。

代码主线在 `src/arch/aarch64/peripherals/gicv3.rs` 和 `src/vm.rs::inject_pending_sgis()` 这一带。

---

## 两个 GIC,各自记账

物理 GICv3 有自己一套寄存器——`ICC_*_EL1`(CPU interface)、`GICD_*`(distributor)、`GICR_*`(redistributor per CPU)。这些寄存器,hypervisor 在 EL2 可以直接访问,因为 EL2 不受 Stage-2 翻译影响,直接 MMIO。

虚拟 GICv3 也有一套寄存器——`ICV_*_EL1`。从名字看跟物理那套像,但前缀从 `ICC` 变成 `ICV`。硬件在某个开关打开后,**guest 在 EL1 写 `ICC_*` 时,硬件自动重定向到 `ICV_*`**——guest 自己以为在动物理 GIC,实际上动的是虚拟接口。

那个开关是 `ICH_HCR_EL2.En`,在 EL2 里设:

```rust
// src/arch/aarch64/peripherals/gicv3.rs
Self::write_hcr((ICH_HCR_TALL1 | ICH_HCR_EN) as u32);
```

`En=1` 之后,guest 接下来执行的每一条 `mrs`/`msr` 访问 `ICC_*_EL1` 都被透传到对应的 `ICV_*_EL1`——没有 trap,没有进 hypervisor,纯硬件自动做。

这意味着 hypervisor **不用拦截**绝大多数 GIC 系统寄存器访问。Guest 读 `ICC_IAR1_EL1` 想拿当前 pending IRQ 编号——硬件直接返回 `ICV_IAR1_EL1` 的值,我们在 List Register 里写了什么,这里就读得到什么。Guest 写 `ICC_EOIR1_EL1` 处理完一个 IRQ——硬件清掉 `ICV_LR<n>_EL2` 里对应那条,我们下一次 inject 才能重用这条 LR。

整个流程没人在 hypervisor 这边花周期,中断处理快得几乎跟 native 一样。

---

## List Register:四个槽位

虚拟世界要看到的待处理中断,得从 hypervisor 这边塞进去。"塞"的容器就是 **List Register**:`ICH_LR0_EL2` 到 `ICH_LR3_EL2`,一共四个。

四个,不是四十个,也不是任意。这是 ARM 架构最低保证,具体实现可能更多——QEMU virt 给的就是 4 个。所以**任意时刻,hypervisor 注入到 guest 的待处理虚拟中断最多只能有 4 条**。

每条 LR 是一个 64 位的字,里面塞着这条虚拟中断的全部信息:INTID、优先级、状态(pending / active / pending-and-active)、HW bit、(HW=1 时)对应的物理 INTID:

```
ICH_LR<n>_EL2 字段(简化):
  [63:62] State (00=invalid, 01=pending, 10=active, 11=pending-and-active)
  [61]    HW (0=virtual only, 1=hardware-linked)
  [55:48] Priority
  [44:32] Physical INTID (HW=1 时有效)
  [31:0]  Virtual INTID
```

写一条 LR = 给虚拟世界塞了一条 pending 中断。写完之后 ERET 回 guest,硬件会自动把这条 IRQ 信号送到 vCPU 的虚拟 CPU 接口,guest 看见 `ICV_IAR1_EL1` 里有东西,跑 IRQ handler。

LR 是稀缺资源。Inject 的时候要先扫一遍 `ELRSR_EL2`(Empty List Register Status Register)看哪一条是空的,然后写那一条:

```rust
// src/vm.rs::inject_pending_sgis (节选)
let pending = vm_state.pending_sgis[vcpu_id].swap(0, Ordering::AcqRel);
let mut bits = pending;
while bits != 0 {
    let intid = bits.trailing_zeros();
    bits &= !(1 << intid);

    let lr_idx = find_free_lr().unwrap_or_else(|| {
        // 没空 LR — 把这条 INTID 重新放回 pending,下次再注入
        vm_state.pending_sgis[vcpu_id].fetch_or(1 << intid, Ordering::Release);
        return;
    });
    write_lr_sgi(lr_idx, intid);
}
```

如果 pending 的中断数超过 4,多出来的留在 `pending_sgis` 原子位图里,下一次 vCPU exit 进 hypervisor 时再尝试 inject。这种"多了排队"模式是 GIC 虚拟化的常态——4 个 LR 在 SMP 大压力时会饱和,但下次 exit 通常很快(WFI、计时器到期、新 IRQ 都会触发),队列消化得过来。

---

## HW=1:让物理 EOI 和虚拟 EOI 联动

文章开头那个 vtimer 例子是这样跑通的:

1. 物理 vtimer 触发,INTID 27,硬件把这条物理中断进 hypervisor(EL2 trap)
2. Hypervisor 看见是 vtimer,挑一个空的 LR,写入 `state=pending, HW=1, virtual_intid=27, physical_intid=27`
3. ERET 回 guest
4. 硬件把虚拟 INTID 27 送给 guest 的虚拟 CPU 接口
5. Guest 内核 IRQ handler 跑完,在 `ICC_EOIR1_EL1` 写 27
6. 硬件做两件事:**在虚拟侧把这条 LR 标成 invalid**(回收槽位),**在物理侧把 GICD/GICR 里的 INTID 27 deactivate**

第 6 步那个"两件事自动同步"就是 HW=1 的作用。如果 HW=0(纯虚拟中断,没有对应物理 INTID),guest 的 EOI 只清虚拟侧 LR;hypervisor 自己得在合适时机去物理 GIC deactivate。但 vtimer、virtio 这些**真有物理 INTID 对应的中断**,HW=1 让 deactivation 完全免去 trap。

代码里的写法是这样:

```rust
// src/arch/aarch64/peripherals/gicv3.rs:436
pub fn inject_virtual_irq_hw(lr_idx: u8, virtual_intid: u32, physical_intid: u32, priority: u8) {
    let lr_value = (LR_STATE_PENDING as u64) << 62
                 | LR_HW_BIT
                 | ((priority as u64) << 48)
                 | ((physical_intid as u64 & 0x1FFF) << 32)
                 | (virtual_intid as u64 & 0xFFFFFFFF);
    Self::write_lr(lr_idx, lr_value);
}
```

`LR_HW_BIT` 是 bit[61]。`physical_intid` 占 `[44:32]`,可以指向 INTID 0-8191。虚拟 INTID 跟物理 INTID 在 vtimer 这里都是 27——guest 看到的中断号跟硬件看到的一样。Linux 内核在 `request_irq(27, ...)` 注册的 handler 就是为这条服务的。

---

## EOImode=1:为什么要把 priority drop 和 deactivation 拆开

`ICC_CTLR_EL1.EOImode=1` 是 Linux 内核运行 GICv3 时**无条件**设的状态——不光 HW=1 链接中断要它,所有虚拟化中断处理流程都依赖这套语义。我们 hypervisor 这边在初始化阶段就把它设好。

`EOImode=0`(默认):guest 写 `EOIR_EL1` 时,硬件同时做 **priority drop**(把这条 IRQ 的优先级从 running 状态降下来,允许更低优先级的 IRQ 抢占)和 **deactivation**(把这条 IRQ 从 active 状态变成 inactive)。一气呵成,简单。

`EOImode=1`:guest 写 `EOIR_EL1` 只做 priority drop。Deactivation 要另外写 `DIR_EL1` 来触发。

为什么要拆?因为 **HW=1 的 LR 在 deactivation 时,硬件要把动作同步到物理 GIC**——这是关键。如果 priority drop 和 deactivation 绑在一起,硬件没办法判断哪个 EOI 是给虚拟中断(只清虚拟侧)、哪个是给硬件链接中断(同步到物理)。拆开之后:**priority drop 仍是 guest 的 EOIR 触发,纯虚拟侧动作;deactivation 是后续的 DIR 触发,这时硬件根据 LR 里的 HW 位决定是否同步到物理**。

Linux 内核知道这件事。它的 GIC 驱动会按 EOImode 设置走对应路径:

- EOImode=0: 只写 EOIR
- EOImode=1: 先写 EOIR,再写 DIR

Hypervisor 这边只要在启动时设好 EOImode=1,后面 Linux 自己会跟上:

```rust
// src/arch/aarch64/peripherals/gicv3.rs:655
let mut ctlr = Self::read_ctlr();
ctlr |= ICC_CTLR_EOIMODE;
Self::write_ctlr(ctlr);
crate::log_info!("[GIC] ICC_CTLR_EL1.EOImode=1 (split priority drop/deactivation)\n");
```

`ICC_CTLR_EL1` 是被透传到 `ICV_CTLR_EL1` 还是真物理的,看 `ICH_HCR_EL2.En` 状态。这里在 GIC init 阶段(hypervisor 自己设),走的是物理 CTLR。但因为虚拟接口启用后两套 CTLR 都遵循这一 EOImode 语义,guest 的 EOI 路径自动是 split 模式。

---

## TALL1=1:拦 ICC_SGI1R 来做 IPI

绝大多数 ICC_* 寄存器访问被硬件自动重定向到 ICV_*,但 `ICC_SGI1R_EL1` 是个例外。

ICC_SGI1R 是 IPI(Inter-Processor Interrupt)入口。Guest CPU 0 想给 CPU 1 发一条 SGI,写 ICC_SGI1R,字段里包 TargetList(哪些 CPU 要收)和 INTID(哪个 SGI 编号)。

为什么不能纯虚拟化?因为 IPI 的语义涉及**跨 vCPU 调度**——CPU 0 写完 SGI,得让 CPU 1 真的醒过来处理。在多 pCPU 模式里,这意味着要让另一颗物理 CPU 进入 IRQ handler 路径(物理 SGI 0 唤醒,见 [Part 9](./part9-multi-pcpu.md))。在单 pCPU 模式里,这意味着要在 vCPU 调度器里把目标 vCPU 排上下一轮。

两件事都需要 hypervisor 介入。所以我们把 ICC_SGI1R 的写**强制 trap**:

```rust
// ICH_HCR_TALL1 = bit 11
Self::write_hcr((ICH_HCR_TALL1 | ICH_HCR_EN) as u32);
```

TALL1=1 让 guest 的所有 `ICC_SGI1R_EL1` 写产生一个 EC=0x18(unknown trap) 的异常,带着 guest 写入的值。hypervisor 在 trap handler 里解码这个值,提取 TargetList 和 INTID,然后:

1. 在 `pending_sgis[target_vcpu]` 原子位图里置位
2. (多 pCPU)向目标物理 CPU 发物理 SGI 0 唤醒
3. (单 pCPU)目标 vCPU 在下一次被调度时,inject 路径会从 pending_sgis 把这条 SGI 写到 LR

ICC_SGI1R 的位域见 [Part 9 的相关段落](./part9-multi-pcpu.md)——`TargetList[15:0]`、`Aff1[23:16]`、`INTID[27:24]`。位置容易写反,我自己第一次写错过。

---

## ELRSR:LR 用完了怎么知道

LR 是稀缺资源,inject 之前要找空闲槽位。`ELRSR_EL2`(Empty List Register Status Register)是个 32 位寄存器,bit i 等于 1 表示 LR i 此刻空闲(已 EOI 完成,或从未写过):

```rust
fn find_free_lr() -> Option<u8> {
    let elrsr = read_elrsr();
    for i in 0..4 {
        if elrsr & (1 << i) != 0 {
            return Some(i);
        }
    }
    None
}
```

Guest 处理完一个 IRQ、写 EOIR 之后,对应的 LR 状态从 active 翻成 invalid,ELRSR 对应位自动变 1。下一次 vCPU exit 时,我们扫 ELRSR,装新的 pending IRQ 进去。

如果 ELRSR 全是 0(四个 LR 都还在 pending / active 状态),说明 guest 还没处理完之前的中断,我们这一轮就 inject 不了任何新的。那些 pending 暂存在 `pending_sgis` / `pending_spis` 位图里。下次 exit 通常很快,队列消化得过来。

观察一下这套机制的两个特点。**第一,inject 是同步的——hypervisor 主动把 LR 写好再 ERET,guest 醒来就看见**。不像有些虚拟化方案要中断异步信号。**第二,EOI 是异步的——guest 自己写 EOIR 触发,hypervisor 完全不参与**。这种"一边主动写、一边硬件代劳"的不对称,是 GICv3 虚拟化效率的关键。

---

## 一次完整的 vtimer 注入

把上面这些串起来。一条 vtimer 中断从触发到 guest 处理完,完整流程:

```
1. 物理 vtimer 到期 (CNTV_CTL.ISTATUS=1) → 物理 INTID 27 触发
2. 物理 GIC 把这条中断送到当前 pCPU 的 ICC_HPPIR1_EL1
3. HCR_EL2.IMO=1 → IRQ 在 EL1 不能处理,trap 到 EL2
4. Hypervisor 进 IRQ handler, mrs ICC_IAR1_EL1 → 27, ack physical
5. find_free_lr() → LR_2
6. write_lr(2, state=pending, HW=1, virtual=27, physical=27, prio=...)
7. ERET 回 guest
8. 虚拟 GIC 把 ICV_HPPIR1_EL1 = 27 送给 guest
9. Guest IRQ handler: mrs ICC_IAR1_EL1 → 27 (硬件自动重定向到 ICV_IAR1_EL1)
10. 跑 timer handler
11. msr ICC_EOIR1_EL1, 27 → 硬件清 LR_2 (state=invalid, ELRSR[2]=1)
                              priority drop (虚拟侧)
12. msr ICC_DIR_EL1, 27    → 硬件 deactivation:
                              虚拟侧无作用(LR_2 已 invalid)
                              物理侧 (因 HW=1) deactivate GIC INTID 27
13. 下次 vtimer 到期可以重新触发
```

Hypervisor 介入只在第 4-6 步——读物理 IAR、写 LR、ERET。三条指令。Guest 看不到第 4 步存在,以为自己直接在处理物理硬件。EOI 路径里 hypervisor 完全不出现,纯硬件代劳。

下一篇我想讲 HPFAR_EL2:guest MMU 开起来之后,`FAR_EL2` 里装的不再是 IPA 而是 guest 的虚拟地址——做 MMIO emulation 时取错寄存器会调一整天。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十二篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
