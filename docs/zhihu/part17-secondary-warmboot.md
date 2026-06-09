# Secondary CPU 在 S-EL2 醒来,接下来要走的六步

[Part 4](./part4-war-stories.md) 讲过我花了几小时调通的那个 SPMD per-CPU 握手——发现 `FFA_MSG_WAIT` 是 secondary 上电的必经动作。这一篇接着讲**握手前面那五步**:secondary CPU 从 `secondary_entry_sel2` 那个标号开始执行,到能调 `FFA_MSG_WAIT` 之间,EL2 上要装配多少东西。

六步,顺序敏感。漏一步 hypervisor 在那颗 CPU 上要么静默挂死,要么不响应中断,要么响应中断时崩。代码在 `src/main.rs::rust_main_sel2_secondary`,从 boot 进 Rust 之后的第一行起。

---

## 第 1 步:装异常向量

```rust
exception::init();
```

`exception::init()` 把 `VBAR_EL2` 设成我们 vector table 的物理地址。**这必须是第一步**——之前任何 trap、page fault、中断都会去硬件默认的位置(通常是 ROM 中的某段无效区域),hypervisor 直接挂。

Primary CPU 启动时也做同样的事,但 secondary 不能共享 primary 那一次设置——`VBAR_EL2` **不是** banked per CPU,每颗 CPU 都得自己 `msr vbar_el2, ...`。

---

## 第 2 步:开 Secure Stage-2

```rust
let hcr: u64;
asm!("mrs {}, hcr_el2", out(reg) hcr, ...);
asm!("msr hcr_el2, {hcr}", "isb",
     hcr = in(reg) hcr | HCR_VM, ...);
```

`HCR_EL2.VM=1` 让 secure Stage-2 翻译生效。SP1/SP2/SP3 在 S-EL1 跑,它们的 IPA 经过 Secure Stage-2 翻译才到真物理地址。

为什么 primary CPU init 阶段已经设过这位,secondary 还要再设?因为 `HCR_EL2` 也**不是** banked——它是 per-CPU 物理寄存器,从 reset 状态出来值为 0。TF-A 的 secondary warm-boot 把 secondary 转交给 SPMC 时不会替你保留 primary 的设置。

`isb` 不能省。`msr hcr_el2` 之后下一条指令的取指阶段可能用旧的 trap 配置去判断,要 `isb` 强制 pipeline 同步才能让新值生效。

---

## 第 3 步:清 trap 位(CPTR / MDCR)

```rust
asm!(
    "mrs x0, cptr_el2",
    "bic x0, x0, {cptr_tz}",      // 不 trap SVE
    "bic x0, x0, {cptr_tfp}",     // 不 trap FP/SIMD
    "bic x0, x0, {cptr_tsm}",     // 不 trap SME
    "bic x0, x0, {cptr_tcpac}",   // 不 trap CPACR
    "msr cptr_el2, x0",
    "msr mdcr_el2, xzr",          // 不 trap debug
    "isb", ...);
```

Primary CPU 启动时也清这些 trap 位,但 secondary CPU 走 TF-A 的 warm-boot 路径上电,**`CPTR_EL2` / `MDCR_EL2` 的状态跟 primary 不一样**——可能保留 reset 值,可能保留 TF-A 临时设置的某个状态。

这一步关键是要在**MMU 开之前做**。后面要 `SCTLR_EL2.M=1` 启用 Stage-1 MMU,启用瞬间硬件会取指、检查权限、跑 `isb`。如果 `CPTR_EL2.TFP=1` 还在,而 Rust debug 模式编译进的 NEON 对齐检查在 `install_sel2_stage1_secondary` 的某条 `read_volatile` 里被 emit 出来,那条 `cnt v0.8b` 就会 trap 进 EL3——而 EL3 的 default handler 不知道怎么处理 S-EL2 来的 SIMD trap,**死循环**。

这个坑 [Part 7](./part7-bare-metal-rust-pitfalls.md) 详细讲过。顺序错一次就是几小时 debug。

---

## 第 4 步:装 S-EL2 Stage-1 MMU

```rust
hypervisor::sel2_mmu::install_sel2_stage1_secondary();
```

S-EL2 跟 NS-EL2 不同——我们要访问 NWd 的 DRAM(给 pKVM 写 FF-A descriptor RESP),这一段地址必须以 `NS=1` 走 Non-secure 物理地址空间。MMU off 时 S-EL2 的访问默认走 Secure 物理地址空间,**两个空间是不同的内存视图**([Part 6](./part6-trustzone-ns-bit.md) 讲过)。

所以 S-EL2 必须开 Stage-1 MMU,页表里 NWd DRAM 那一段打 `NS=1`,Secure DRAM 那一段打 `NS=0`。

Secondary CPU 不用从头建页表——primary 早就建好了一份。`install_sel2_stage1_secondary()` 把 primary 的 TTBR0_EL2 / TCR_EL2 / MAIR_EL2 加载到这颗 CPU,然后 `SCTLR_EL2.M=1` 打开:

```rust
pub fn install_sel2_stage1_secondary() {
    let ttbr0 = PRIMARY_TTBR0.load(Ordering::Acquire);
    let mair = PRIMARY_MAIR.load(Ordering::Acquire);
    let tcr = PRIMARY_TCR.load(Ordering::Acquire);
    unsafe {
        asm!(
            "msr ttbr0_el2, {ttbr0}",
            "msr mair_el2, {mair}",
            "msr tcr_el2, {tcr}",
            "isb",
            "mrs x0, sctlr_el2",
            "orr x0, x0, {sctlr_bits}",   // SCTLR_EL2 |= M | C | I
            "msr sctlr_el2, x0",
            "isb",
            ...
        );
    }
}
```

复用 primary 的页表是这套架构的关键。**S-EL2 的 Stage-1 映射对所有 CPU 是一致的**——secondary 不需要分配自己的页表,只要把 TTBR0 指向同一个表。这跟 EL1 那种"每个进程一个 TTBR0"完全不同。

---

## 第 5 步:使能本 CPU 的 GIC PPI 26 + 29(Secure Group 1)

```rust
let gicr_sgi_base = hypervisor::dtb::gicr_sgi_base(core_id);
let ppi_mask: u32 = (1 << 26) | (1 << 29);

// GICR_IGROUPR0: clear bits → 不是 NS Group 1
let igroupr0 = (gicr_sgi_base + 0x0080) as *mut u32;
write_volatile(igroupr0, read_volatile(igroupr0) & !ppi_mask);

// GICR_IGRPMODR0: set bits → 是 Secure Group 1
let igrpmodr0 = (gicr_sgi_base + 0x0D00) as *mut u32;
write_volatile(igrpmodr0, read_volatile(igrpmodr0) | ppi_mask);

// GICR_ISENABLER0: enable
let isenabler0 = (gicr_sgi_base + 0x0100) as *mut u32;
write_volatile(isenabler0, ppi_mask);
```

每颗 CPU 一个 GICR(GIC Redistributor),互相独立。Primary CPU 在 init 时使能了它自己那个 GICR 上的 PPI 26+29,但 secondary 这边的 GICR 还是 reset 状态——PPI 不使能,中断永远不送过来。

这一步走三个寄存器:`IGROUPR0` + `IGRPMODR0` 一起决定中断的 group(Group 0 / NS Group 1 / Secure Group 1),`ISENABLER0` 使能。三个一起做,中断才能进 S-EL2。

不做会怎么样?[Part 9](./part9-multi-pcpu.md) 里讲过的 CNTHP poll 定时器(PPI 26)在这颗 CPU 上永远不触发。如果 SP 在这颗 CPU 上 idle 等中断,SPMC 就接管不了——它依赖 CNTHP 定时唤醒,但定时器中断没使能,就一直在 WFI 状态死着。pKVM 那边发的 FF-A SMC 都拿不到响应。

---

## 第 6 步:FFA_MSG_WAIT,跟 SPMD 握手

```rust
let first_request = forward_smc8(FFA_MSG_WAIT, 0, 0, 0, 0, 0, 0, 0);
```

[Part 4](./part4-war-stories.md) 讲过这一笔。前面五步都做对了,但少了这一笔,SPMD 不知道我们 S-EL2 就绪——它会阻塞,pKVM 那边的 PSCI CPU_ON 永远不返回,Linux 看到 secondary CPU 没上来。

`FFA_MSG_WAIT` 返回的不是 success,**是阻塞**——这一笔 SMC 在 SPMD 那边一直挂着,直到 NWd(pKVM)第一次往这颗 CPU 发 FF-A 请求。那时候 SPMD 把 NWd 的 SMC 内容塞进 `FFA_MSG_WAIT` 的返回值(x0-x7),secondary 这边才真正"返回"。

返回之后下一步是进 `run_event_loop(first_request)`——同 primary,处理 FF-A 调用、跑 SP、转发结果。

---

## 收尾

六步全部走完,这颗 secondary CPU 才真正"活着"——能接 FF-A 请求、能让 SP 跑、能响应中断。

顺序硬性:清 trap 位必须在开 MMU 之前;开 MMU 必须在握手之前(SPMD 来的返回值要读,读得有 MMU);PPI 使能可以稍微往后挪,但绝不能放到事件循环里(进事件循环之后中断该响应了)。

调 secondary CPU 时做不出来的现象常常是**第三步漏了某一位**(`CPTR_EL2.TFP` 没清)或者**第五步顺序乱了**(IGROUPR / IGRPMODR / ISENABLER 写反了)。这些都是看起来"功能性的小细节",但漏一行整个 CPU 就废。

下一篇我想讲 `HCR_EL2.TSC` 那个非对称 trap——它能拦 guest 的 SMC,但 hypervisor 自己发的 SMC 直通 EL3,这种"我能拦你你拦不了我"的语义为什么是对的。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十七篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
