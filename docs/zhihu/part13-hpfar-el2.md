# FAR_EL2 给我的不是 IPA——Stage-2 fault 时该读哪个寄存器

Guest Linux 启动到一半。某条指令访问了一个 device,触发 Stage-2 fault,进我这边的异常 handler。我照着早期写好的代码读 `FAR_EL2`,准备 dispatch 给对应的 MMIO 仿真设备。

`FAR_EL2 = 0xffff_8000_0900_0000`。

UART 在 `0x0900_0000`,这个地址末尾对得上,但前面那一长串 `0xffff_8000` 哪里来的?那不是 IPA,那是 Linux 内核地址空间的某个虚拟地址。我的 device manager 按 IPA 路由,这个值过去当然找不到任何设备。MMIO 仿真直接走 fallback,guest 看见的就是"读到 0"或者"写入丢失",然后挂死在某个奇怪的位置。

修这个 bug 花了我一整天。回头看,根因一行话:**Stage-2 enabled 时,`FAR_EL2` 装的是 guest 的虚拟地址,不是 IPA。**

---

## 翻 ARM ARM 之前我猜了什么

我先怀疑 device manager 路由错了。改了几版地址范围匹配,加了 log 打印 dispatch 路径。`FAR_EL2 = 0xffff_8000_0900_0000` 在我修改后的 dispatch 里依然不命中,因为它依然不像一个合法 device 地址。

然后怀疑 device 注册时初始化顺序有问题,UART 是不是太晚才进 manager 数组。给 `register_device()` 加 log,确认 UART 在第一次访问之前就已注册。

再后来怀疑 ESR_EL2 decoded 错了,是不是把 PERM fault 当成 TRANSLATION fault。读 `ESR_EL2.EC` 字段,EC=0x24,确实是 Data Abort from lower EL,没解错。

兜了一圈之后才去翻 ARM ARM——这本来该是第一件事。

---

## ARM ARM D13.2.55 那一段

> For exceptions taken from EL1 or EL0 due to a Stage-2 fault, the value of the virtual address is reported in FAR_EL2. Software must use HPFAR_EL2 to determine the IPA at which the fault occurred.

一句话。**Stage-2 fault 时,FAR_EL2 报的是 virtual address**。

为什么这样设计?因为 Stage-2 fault 发生在 Stage-1 翻译**之后**——guest 的 MMU 已经把虚拟地址翻译成 IPA,然后 IPA 在 Stage-2 翻译时撞到 fault。硬件在 fault 那一刻能"忠实"报告的是 guest 软件最初使用的地址,也就是 VA(虚拟地址,在 guest 看来是它的物理地址)。IPA 在那时候也有,但 ARM 选择把它放进一个**专门**的寄存器:`HPFAR_EL2`(Hypervisor IPA Fault Address Register)。

设计上这是有理由的——FAR_EL2 在 EL2 异常里通用,**任何**异常都可能写它,语义需要保持一致。Stage-2 fault 把 IPA 塞 FAR_EL2 会让"FAR_EL2 在 Stage-1 fault 里是 VA,在 Stage-2 fault 里是 IPA"这种条件性混淆开来。HPFAR_EL2 单独的好处是清楚:**它只在 Stage-2 fault 时有意义,其他时候无视它**。

---

## 正确的 IPA 提取

HPFAR_EL2 不是直接的 IPA,而是 **IPA 的页帧号**。在 48 位 IPA 配置下(QEMU virt 默认),位域是:

```
HPFAR_EL2:
  [39:4] FIPA[47:12] — Faulting Intermediate Physical Address (page number)
  [3:0]  RES0
```

(开了 FEAT_LPA2 的 52 位 IPA 实现里,这一段扩成 `[43:4] = IPA[51:12]`。位宽对得上,只是覆盖范围更大。)

注意 `[39:4]` 这个范围:**HPFAR_EL2 里存的是 IPA 右移 8 位的页号**。要还原出页基址,要左移 8 位回去:

```rust
let ipa_page = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8;
```

Mask `0x0000_0FFF_FFFF_FFF0` 在 48 位 IPA 配置下实际只有 `[39:4]` 这 36 位会有意义的值,`[43:40]` 是 RES0(硬件保证为 0)。左移 8 位之后得到 `IPA[47:12] << 12`,就是 4 KB 页对齐的基址。如果以后开 FEAT_LPA2,这套 mask 也能直接覆盖 52 位 IPA 不用改。

页内偏移得另外取——`FAR_EL2[11:0]` 是页 offset(VA 的低 12 位等于 IPA 的低 12 位,因为 Stage-1 翻译以 4 KB 为粒度):

```rust
let page_offset = far_el2 & 0xFFF;
let ipa = ipa_page | page_offset;
```

代码里这一段在 `src/arch/aarch64/hypervisor/exception.rs:455-459`:

```rust
// HPFAR_EL2[43:4] = IPA[47:12] (page number)
// FAR_EL2[11:0] = page offset within the 4KB page
let ipa_page = (hpfar & 0x0000_0FFF_FFFF_FFF0) << 8;
let page_offset = context.sys_regs.far_el2 & 0xFFF;
let ipa = ipa_page | page_offset;
```

这一行注释比代码本身重要。后面任何人维护这段代码,都得知道"为什么 FAR_EL2 在这里只取低 12 位"——不然就会犯跟我一样的错。

---

## 为什么前期没踩坑

回头看,这个 bug 在 boot 的早期阶段**应该**早就触发的——hypervisor 启动后做的第一件事就有 MMIO 仿真(UART 打 log)。为什么前几周一直没事?

因为前几周 guest 的 MMU 还是关着的。

Stage-1 翻译在 guest MMU off 时是 passthrough——VA == IPA。这种状态下 `FAR_EL2` 装的"虚拟地址"恰好等于"IPA",我那段错误的代码用 FAR_EL2 当 IPA,跑得跟对的没区别。

Linux 内核启动到 `arch/arm64/kernel/head.S` 后期会开 MMU(`SCTLR_EL1.M=1`),从此 VA != IPA。我的代码就在那一刻开始拿到"看起来不对的 FAR_EL2",但仍然按 IPA 拿去 dispatch——找不到设备,丢访问,挂死。

这种"前期无症状、跑到某个里程碑触发"的 bug,在裸机系统里特别多。Boot 流程里**任何一个状态开关**(MMU on、cache on、IRQ enable、virtualization enable)都可能改变某个寄存器的语义,而你之前的代码恰好绕开了那条语义。等到状态开关翻过去,bug 才显形。

---

## 一个推论

如果你写 hypervisor 时遇到"MMIO 仿真在 guest MMU off 时正常、开了之后挂"这种症状,**优先怀疑 FAR_EL2 取错**。不是其他几十件可能错的事。

更广义的检查:任何在异常里读地址的代码,看看那个寄存器的语义是不是依赖 fault 类型。`ESR_EL2.EC` 决定该读哪个寄存器,**永远先看 EC**。Hypervisor 关心的 guest 异常是"from lower EL"那一组:

- Instruction Abort from lower EL (EC=0x20) — `FAR_EL2` 是 fault PC
- Data Abort from lower EL (EC=0x24) — `FAR_EL2` 是 guest VA,IPA 在 `HPFAR_EL2`
- Watchpoint from lower EL (EC=0x34) — `FAR_EL2` 是 trigger 地址

对应的 0x21 / 0x25 / 0x35 是 same EL 触发(hypervisor 自己代码崩了),要单独走 fault diag 路径——别跟 guest 异常用同一段处理。

写注释的时候把这个判断条件写下来,下一次任何人(包括你自己)动这段代码,都不用再花一天去 ARM ARM 里挖一遍。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十三篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
