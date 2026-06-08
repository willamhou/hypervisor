# 4 个 vCPU 跑在 4 颗物理 CPU 上 —— 真 PSCI、TPIDR_EL2 和那把不能拿的 DEVICES 锁

> 上一篇是两台 Linux VM 在一颗物理 CPU 上时间片切,互 ping 走 200 行 vSwitch。这一篇换条线:一台 Linux VM,4 个 vCPU,**每个 vCPU 独占一颗物理 CPU**,1:1 affinity,没有调度器。代码上是 `multi_pcpu` 这条 feature 分支,`make run-linux-smp` 启动。

---

## 写在前面

`multi_vm` 是"两台 VM 抢一颗 pCPU 的时间片",`multi_pcpu` 是"一台 VM 的 4 个 vCPU 各占一颗 pCPU,谁也不让谁"。两条 feature 是互斥的——`multi_vm` 假设 vCPU 远多于 pCPU,所以得调度;`multi_pcpu` 假设 vCPU 数 == pCPU 数,所以根本不要调度器。运行模型差太多,放一个二进制里就要满地 `if cfg`,所以代码层就分了。

听起来 1:1 比时间片简单——没调度器、没切换。实际上不简单,但**复杂在你想不到的地方**。这篇讲**七个非平凡的问题**,从"secondary pCPU 怎么活过来"开始,到"为什么 `inject_spi()` 不能拿那把它本该拿的锁",再到"跨 pCPU 唤醒该用什么硬件原语"。

整个 multi_pcpu 这条分支加起来 ~500 行新代码,但每行都有故事。

---

## 问题 1:QEMU virt 的 secondary pCPU 一开始是 powered off 的

`make run-linux-smp` 里 QEMU 启动参数:

```
-smp 4
```

直觉上以为这就有 4 颗 CPU 一起跑,4 份 `_start` 同时进入 hypervisor。**不是的。**

QEMU virt 模拟的硬件复刻了大多数 ARMv8 SoC 的实际行为:**boot CPU(MPIDR.Aff0=0)上电,其余 secondary CPU 都处于 powered-off 状态**,根本不执行 `_start`,等谁来唤醒。

`arch/aarch64/boot.S` 这里就处理这件事:

```asm
_start:
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // x19 = cpu_id
    cbnz    x19, halt              // cpu_id != 0 → halt
    
primary_boot:
    // ... 仅 primary 走这条 ...
```

也就是:**即使有别的 CPU 跑到这,我们也立刻让它 halt**——因为合法情况下不会有别的 CPU 在这一刻跑到这。secondary 现在是物理上断电的。

那 secondary 怎么活过来?**hypervisor 自己得发一条 PSCI CPU_ON 给 QEMU 的 EL3 firmware**,告诉它"请把 CPU N 上电,并从地址 X 开始执行"。

---

## 问题 2:从 EL2 直接发 SMC 给 EL3 唤醒物理核

`src/guest_loader.rs::wake_secondary_pcpus()`(节选):

```rust
// 给每颗 secondary 发一次 PSCI CPU_ON
for cpu_id in 1..num_cpus {
    let target_mpidr = cpu_id as u64;       // Aff0 = cpu_id
    let entry_addr = secondary_entry as u64; // boot.S 里的入口符号

    let ret: u64;
    // SAFETY: SMCCC v1.2,x4-x17 caller-saved
    unsafe {
        core::arch::asm!(
            "smc #0",
            inout("x0") PSCI_CPU_ON_64 => ret,   // 0xC400_0003
            inout("x1") target_mpidr => _,
            inout("x2") entry_addr => _,
            inout("x3") 0u64 => _,
            lateout("x4") _, lateout("x5") _,
            // ... x4-x17 全部 clobber
            options(nostack, nomem),
        );
    }
    // ret == PSCI_SUCCESS 就成功
}
```

几个值得点出的细节:

- **`smc #0` 从 EL2 直接进 EL3**——`HCR_EL2.TSC` 只拦 EL1 的 SMC,EL2 自己发出去的 SMC 是直通的。
- **PSCI CPU_ON 的 function ID 是 `0xC400_0003`**(SMC64 调用约定,bit[31] 表示 fast call,bit[30] 表示 SMC64,bit[29:24]=0x84 是 PSCI service)。
- **`target_mpidr` 是按 MPIDR 格式打的**,QEMU virt 用 Aff0 区分核,直接 `cpu_id` 就行;真硬件可能要带 Aff1/Aff2/Aff3。
- **SMCCC 调用约定 x4-x17 caller-saved**,Rust inline asm 必须把它们都列成 clobber,否则编译器以为这些寄存器还能复用,后续读到的值是垃圾。这是个第一次写 SMC 容易踩的坑。

发完这条 SMC,QEMU EL3 firmware(其实是 QEMU 自己模拟的 PSCI handler)做两件事:把 CPU N 上电,然后让它从 `entry_addr`(我们的 `secondary_entry` 标号)开始执行。

---

## 问题 3:secondary 醒来后,EL2 一切都是空的

`secondary_entry` 在 `boot.S` 里:

```asm
.global secondary_entry
secondary_entry:
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // cpu_id

    // 每个 pCPU 一块独立的栈:pcpu_stacks + cpu_id * 128KB
    adrp    x1, pcpu_stacks
    add     x1, x1, :lo12:pcpu_stacks
    lsl     x0, x19, #17           // cpu_id * 128KB
    add     sp, x1, x0             // 栈顶
    // 跳进 Rust
    mov     x0, x19
    bl      rust_main_secondary
```

注意 **每 pCPU 独立 128KB 栈**——多 pCPU 一起跑,共用一块栈就直接互相踩死了。

`rust_main_secondary` 里做的事看起来都简单,但**少做一件就挂**:

```rust
#[cfg(feature = "multi_pcpu")]
pub extern "C" fn rust_main_secondary(cpu_id: usize) -> ! {
    // 1. 装异常向量(否则下一秒任何 trap 都不可恢复)
    exception::init();

    // 2. 装 Stage-2(VTTBR/VTCR 从 primary 共享过来)
    let vttbr = SHARED_VTTBR.load(Ordering::Acquire);
    let vtcr = SHARED_VTCR.load(Ordering::Acquire);
    unsafe { asm!("msr vtcr_el2, {0}", "msr vttbr_el2, {1}", "isb", ...) }

    // 3. HCR_EL2:开 Stage-2,清 TWI(WFI 透传)
    let mut hcr: u64;
    unsafe { asm!("mrs {}, hcr_el2", out(reg) hcr, ...) }
    hcr |= HCR_VM;
    hcr &= !HCR_TWI;
    unsafe { asm!("msr hcr_el2, {}", "isb", in(reg) hcr, ...) }

    // 4. 清 CPTR_EL2 的 trap 位(否则任何 FP/SIMD/SVE/SME 都陷到 EL3)
    // 5. 初始化本 pCPU 的 GIC(系统寄存器接口 + 虚拟接口)
    gicv3::init();

    // 6. 装本 pCPU 的 percpu context 槽(后面再讲)
    unsafe { (*percpu::this_cpu()).vcpu_id = cpu_id; }

    // 7. 进 idle loop:WFE 等 guest 的 PSCI CPU_ON
    loop {
        unsafe { asm!("wfe", ...) };
        if let Some((entry, ctx)) = PENDING_CPU_ON_PER_VCPU[cpu_id].take() {
            secondary_enter_guest(cpu_id, entry, ctx);
        }
    }
}
```

第 3 步的 **`HCR_EL2 &= !HCR_TWI`** 是这次架构层最重要的一个开关:**清 TWI 等于让 WFI 透传**。guest 的 `wfi` 不再陷入 EL2,直接走到物理 CPU 的 WFI——真休眠,等真硬件中断把它唤醒。`multi_vm` 模式下 TWI 是设着的,guest 一 WFI 我们就抢回控制做调度;`multi_pcpu` 1:1 affinity 没什么好调度的,放开能省下 trap 开销。

第 4 步那个 `CPTR_EL2` 的清 trap 位——如果是 secondary 直接跑普通 Linux guest 是无所谓的,但**这套代码同二进制要兼容 SPMC 模式**,SPMC 在 S-EL2 跑 secondary 时不清 CPTR 会被 Rust debug 模式编译进的 NEON 指令打挂([Part 7](./part7-bare-metal-rust-pitfalls.md) 那个坑)。所以这一步保险起见放在这。

第 7 步的 `wfe` + `sev`/PENDING 那对——guest 发 PSCI CPU_ON 后我们这边怎么唤醒在 idle 的 secondary?见下一节。

---

## 问题 4:1:1 没调度器,context 指针放哪?TPIDR_EL2

`multi_vm` 单 pCPU 那条线里,异常处理找当前 vCPU 的 context 只要读全局 `current_vcpu_context`——只有一颗 CPU 在跑,不会冲突。

`multi_pcpu` 这就不行了:4 颗 pCPU 同时在跑各自的 vCPU,异常向量是**同一份代码**(所有 pCPU 共享 `VBAR_EL2`),但每颗 pCPU 走进来时要找**自己的** vCPU context。一个全局变量绝对救不了你。

ARM 给了你一个礼物:**`TPIDR_EL2`,硬件 banked per pCPU**。每颗物理 CPU 都有自己独立的 `TPIDR_EL2`,读写不互相影响。把 vCPU context 指针塞这里:

```rust
// enter_guest 之前(每颗 pCPU 各自做一次)
unsafe { asm!("msr tpidr_el2, {}", in(reg) ctx_ptr) }
```

`arch/aarch64/exception.S` 异常处理向量里:

```asm
    // 从 TPIDR_EL2 拿当前 pCPU 的 vCPU context 指针
    mrs     x0, tpidr_el2
    // 接下来按 x0 指针保存 guest 状态...
```

一句 `mrs x0, tpidr_el2` 就把"全局 vCPU 指针"问题彻底解决——不用锁、不用原子操作、不用查 MPIDR 索引数组。**让硬件帮你做 per-CPU**,这是 EL2 编程里最优雅的一类技巧。

副产品:`fault_diag_print`(`exception.S` 末尾的诊断 fault handler)用 **`TPIDR_EL2 == 0` 作为"这次异常没有 vCPU 上下文"**的信号——比如 hypervisor 自己代码 fault 时。逻辑很自然:进 guest 前才会写 TPIDR_EL2,boot 阶段它就是 0。

---

## 问题 5:guest GICR 写只动 shadow,得自己使能物理 GICR

GICv3 的中断需要在两个地方被 enable:**软件 shadow**(我们的 `VirtualGicr`)和**物理 GICR**。`multi_vm` 单 pCPU 路径里这个问题不显眼——只有一颗物理 CPU,boot 时一次初始化就够。`multi_pcpu` 这就麻烦:

- 4 颗 pCPU 各有各的 GICR(地址不一样:`gicr_base + cpu * 0x20000`)
- 每颗都得使能自己的 SGI 0-15 + PPI 27(virtual timer)
- guest 写自己的 `GICR_ISENABLER0` 时**只更新软件 shadow**——hypervisor 把 GICR 区域做了 trap-and-emulate,guest 看到的是 `VirtualGicr` 的状态,物理 GICR 根本没被触及

所以 `ensure_vtimer_enabled(cpu_id)` 在**每次进 guest 前**都要跑一次,直接写物理 GICR:

```rust
pub fn ensure_vtimer_enabled(cpu_id: usize) {
    let rd_base = dtb::gicr_rd_base(cpu_id);
    let sgi_base = rd_base + 0x10000;
    let isenabler0 = (sgi_base + GICR_ISENABLER0_OFF) as *mut u32;
    // SGI 0-15 + PPI 27 (vtimer)
    let mask = 0xFFFF | (1 << 27);
    unsafe { core::ptr::write_volatile(isenabler0, mask) };
}
```

为什么每次都要写?**guest 自己可能再去 disable 它**——比如某些 Linux 启动路径会把 vtimer 关掉重开。我们的 `VirtualGicr` 接受这次 disable(改 shadow),但物理 GICR 已经被关了,下一次 vtimer 就再也不会触发,guest 时钟死掉。所以每次进 guest 前重新确认物理使能,简单粗暴但能用。

EL2 直接写物理 GICR 是**绕过 Stage-2 翻译**的——准确说,EL2 自己的访问不经过 Stage-2(它仍然走自己的 EL2 Stage-1 + 内存属性,但 Stage-2 这层对 EL2 透明),所以可以直达物理 GIC,没有中间层,几条指令就完成。

---

## 问题 6:`inject_spi()` 不能拿那把锁

设备完成处理(比如 virtio-blk 写完一个 request)要把 SPI 注入 guest——通用入口是 `global::inject_spi(intid)`。它做两件事:

1. 查 SPI 该路由到哪个 vCPU(读 `GICD_IROUTER`)
2. 把这一位标记到目标 vCPU 的 pending 队列里

`multi_vm` 单 pCPU 下,`route_spi` 走的是 `DEVICES.route_spi()`——读 `VirtualGicd` 的 shadow 状态。**但 `multi_pcpu` 下 `DEVICES` 是 SpinLock 保护的**(多 pCPU 真的可能同时访问)。

死锁场景出现了:

```
某 pCPU 上 virtio-blk 处理完一个 request
  → DEVICES.lock() 拿到锁,调 process_request()
    → process_request() 内部调 signal_interrupt()
      → signal_interrupt() 调 inject_spi(48)
        → inject_spi() 想 DEVICES.lock() 读 IROUTER
          ⛔ 死锁:我们已经持有 DEVICES 锁
```

Rust 的 `SpinLock` 不是 reentrant 的——同一线程二次 acquire 自旋等自己,永远等不到。

修法不是"换 reentrant 锁"(锁本身没病,数据流有病),而是**绕开锁:直接读物理 GICD_IROUTER**。EL2 一样不受 Stage-2 影响:

```rust
pub fn inject_spi(intid: u32) {
    if !(32..=63).contains(&intid) { return; }
    let bit = intid - 32;
    let vm_id = CURRENT_VM_ID.load(Ordering::Relaxed);
    let vs = &VM_STATE[vm_id];

    #[cfg(feature = "multi_pcpu")]
    let target = {
        let irouter_addr = dtb::platform_info().gicd_base + 0x6100
                         + (intid as u64 - 32) * 8;
        let irouter = unsafe {
            core::ptr::read_volatile(irouter_addr as *const u64)
        };
        (irouter & 0xFF) as usize  // Aff0 = vCPU ID
    };
    #[cfg(not(feature = "multi_pcpu"))]
    let target = DEVICES[vm_id].route_spi(intid);

    if target < MAX_VCPUS {
        vs.pending_spis[target].fetch_or(1 << bit, Ordering::Release);
        // ... 跨 pCPU 唤醒,见下一节
    }
}
```

讲得绕一点:`DEVICES` 锁保护的是 `VirtualGicd` 的 shadow 状态——它是 `IROUTER` 写入的**镜像**。但物理 `GICD_IROUTER` 是 guest 写 shadow 时由 hypervisor 同步写过去的(GICD 是 trap + write-through)。所以**物理 IROUTER 一定和 shadow 一致**,我们直接读物理就行。换个角度想,**只读物理 + 信任 write-through 一致性**,完美绕开锁,正确性也站得住。

---

## 问题 7:跨 pCPU 投递:物理 SGI 0 把对方从 WFI 拉出来

SPI 标记到目标 vCPU 的 pending 队列后,如果目标 vCPU 此刻**正在另一颗 pCPU 上 idle**(WFI 等中断),光标记 pending 它也没察觉,得有人去戳它。

我们用**物理 SGI 0**:

```rust
#[cfg(feature = "multi_pcpu")]
{
    let current = percpu::current_cpu_id();
    if target != current {
        // SGI 0 → 目标 pCPU,把它从 WFI 唤醒
        let val: u64 = 1u64 << target; // TargetList,INTID=0
        unsafe {
            asm!("msr icc_sgi1r_el1, {val}", "isb",
                 val = in(reg) val, options(nostack, nomem));
        }
    }
}
```

这里要扣两个 ARM 细节:

- **`ICC_SGI1R_EL1` 的 TargetList 在 bits[15:0]**——bit i 代表 "目标 Aff0 == i",一次可以多个 pCPU 一起戳。bits[15:0] 这个位置很多人写成 [23:16] 出错(我也踩过)。
- **SGI 0 是普通中断**,会被 guest 的中断分发处理。我们这里只把它当"信号" 用——目的不是注入,目的就是把目标 pCPU 从 WFI 中拉醒,让它重新进入 hypervisor 检查 pending SPI。

EL2 直接发 `msr icc_sgi1r_el1`——硬件就送一条 SGI 给目标 pCPU,所有都绕过 Stage-2,绕过 DEVICES 锁。**用硬件原语解决一切**。

---

## 实测

`make run-linux-smp`,Linux guest 看到自己有 4 颗 CPU,全部 online,vtimer 正常,virtio-blk 走 SPI 48 正常 inject。BusyBox shell 里:

```
# cat /proc/cpuinfo | grep processor
processor       : 0
processor       : 1
processor       : 2
processor       : 3

# nproc
4

# dd if=/dev/vda of=/dev/null bs=1M count=64
64+0 records in
64+0 records out
67108864 bytes (64MB) copied, ...
```

虚拟设备走得通,4 颗 vCPU 真的在 4 颗 pCPU 上跑,WFI/IRQ 路径都成立。

## 数字

| 模块 | 行数 | 关键点 |
|---|---|---|
| `wake_secondary_pcpus()` + boot.S secondary_entry | ~50 | 真 PSCI CPU_ON SMC + per-pCPU 栈分配 |
| `rust_main_secondary()` | ~80 | secondary EL2 状态从零搭建 |
| `secondary_enter_guest()` | ~60 | vCPU 装配 + 进 guest |
| TPIDR_EL2 改造(exception.S 等) | ~30 | 把全局 ctx 指针换成 per-pCPU banked |
| `DEVICES` SpinLock + 直读物理 GICD_IROUTER | ~40 | 解死锁 |
| 跨 pCPU 唤醒:`icc_sgi1r_el1` 发 SGI 0 | ~20 | 把 idle pCPU 从 WFI 拉醒 |
| `ensure_vtimer_enabled` 每次进 guest 前调 | ~10 | 物理 GICR 重新使能 |
| `PENDING_CPU_ON_PER_VCPU` + WFE/SEV 配对 | ~40 | guest PSCI CPU_ON 跨 pCPU 投递 |

合计约 **500 行新代码**。大多数代码本身不长,但每一段背后都对应一个 ARM 架构细节:**TPIDR_EL2 是硬件 banked、SMC 从 EL2 直通 EL3、EL2 绕过 Stage-2 读物理 GIC、SpinLock 不可重入、SGI TargetList 在 [15:0]**。这些拼起来才是 multi_pcpu。

---

## 与 `multi_vm` 互斥:为什么是 feature flag 而不是运行时开关

`multi_vm` 和 `multi_pcpu` 在 Cargo 里写成互斥 feature:

```toml
[features]
multi_vm = ["linux_guest"]
multi_pcpu = ["linux_guest"]
# 用户不应同时启用
```

互斥的原因不是"两个不能共存",而是**代码路径分歧太大**:

- `DEVICES`: `multi_vm` 是 `UnsafeCell`(单 pCPU 保证串行),`multi_pcpu` 是 `SpinLock`
- `inject_spi`: 前者读 shadow,后者直读物理
- WFI: 前者 trap,后者 passthrough
- ctx 指针: 前者全局,后者 TPIDR_EL2
- 调度: 前者有调度器,后者完全没有

放一个二进制里就会满地 `if cfg!` 或者动态分发,运行时开关比"每次开 feature 重编译"要复杂得多,而且 hypervisor 哪条线产生 bug 都会让另一条同时不稳。**这种分歧用编译期 feature 分开,是 no_std 项目里最干净的做法**。

代价是 CI 要把两条都跑,但比起调试运行时分支错位带来的痛,这点 CI 开销值。

---

## 收尾

`multi_pcpu` 之后,hypervisor 已经能在 4 颗 ARM 物理核上做真 SMP:guest 看到 4 颗 vCPU,WFI 真休眠,中断真投递,virtio 真跑。

要继续往上走有几个方向:

- **VM 内的 multi-pCPU + multi-VM 共存**——两台 VM 都跑 4 vCPU 真硬件 1:1,需要 8 颗物理核,代码层面是把 `DEVICES` 的 SpinLock + per-VM 切换 + per-pCPU TPIDR_EL2 三件事融起来(还没做)
- **更细粒度的 vCPU 迁核**——目前是死 1:1,如果想 vCPU 跨 pCPU 迁移,就要补 vCPU context 在 pCPU 间的同步,以及 GIC 路由表的动态更新
- **vGIC v4**(direct injection LPI)——目前 SPI/PPI 都靠 LR 注入,LPI 没用上;真 SMP 跑大型 guest 时 LPI 直接投递能再省一截开销

下一篇我想展开"**跨世界内存共享**":FF-A 的 `MEM_SHARE/LEND/DONATE/RETRIEVE/RELINQUISH/RECLAIM` 整套生命周期、PTE software bits 怎么追踪所有权、为什么 `S2AP` 要根据共享类型动态收紧——是另一条独立的 deep dive 主线。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第九篇。之前的文章:*

- *Part 0a: [为什么写一个 Hypervisor](./part0a-why.md)*
- *Part 0b: [AI 辅助系统编程](./part0b-ai-workflow.md)*
- *Part 1: [从零到 "Hello from EL2!"](./part1-first-boot.md)*
- *Part 2: [陷入-模拟-恢复](./part2-trap-emulate-resume.md)*
- *Part 3: [让 Linux 启动](./part3-linux-boot.md)*
- *Part 4: [裸机四大坑·收尾](./part4-war-stories.md)*
- *Part 5: [Rust enum 状态机的真相](./part5-enum-state-machine.md)*
- *Part 6: [TrustZone 的 NS 位](./part6-trustzone-ns-bit.md)*
- *Part 7: [bare-metal Rust 三个坑](./part7-bare-metal-rust-pitfalls.md)*
- *Part 8: [两台 VM 互 ping(200 行 vSwitch)](./part8-multi-vm-vswitch.md)*
