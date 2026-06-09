# `ICC_SGI1R_EL1` 那笔糊涂账——TargetList 不在 bits[23:16]

我想给 CPU 1 发一条 SGI 编号 4,从 CPU 0 上。手册看了一眼,几个字段往 `ICC_SGI1R_EL1` 里塞:

```rust
// 错误版本 — 拼错了所有位置
let val: u64 = (1 << 23)       // TargetList: bit for CPU 1 ← 我以为 TargetList 在 [23:16]
             | (4 << 0);        // INTID: 4              ← 我以为 INTID 在 [3:0]
asm!("msr icc_sgi1r_el1, {val}", val = in(reg) val, ...);
```

跑下来 SGI 根本没发出去。GIC 没有任何反应,CPU 1 在 WFE 里继续睡。

正确的字段位置:

```rust
let val: u64 = (1u64 << 0)     // TargetList[15:0]: bit for CPU 1 (Aff0 == 1)
             | (4u64 << 24);    // INTID[27:24]: 4
```

`TargetList` 在 **bits[15:0]**——不是 [23:16]。`INTID` 在 **bits[27:24]**——不是 [3:0]。bit 位置全错。这一篇讲为什么这么排,以及怎么记住不忘。

---

## ARM ARM 里的实际布局

`ICC_SGI1R_EL1` 是 64 位寄存器,字段按从高到低:

```
[63:56]  RES0
[55:48]  Aff3        — 目标 CPU 的 Aff3
[47:44]  RES0
[43:40]  RS          — Range Selector(targeting超过 16 颗 CPU 时用)
[39:32]  Aff2        — Aff2
[31:28]  RES0
[27:24]  INTID       — SGI 编号(0-15)
[23:16]  Aff1        — Aff1
[15:0]   TargetList  — Aff0 域内 16 颗 CPU 的位图
```

具体语义:**`TargetList[i] = 1` 表示发送给 Aff0 = i 的那颗 CPU**(在某个 Aff3:Aff2:Aff1 组合内)。一笔写最多可以同时给 16 颗 CPU。

要发给 Aff0 = 1、Aff1 = 0、INTID = 4,就是:

```rust
TargetList = 1u16 << 1;     // bit 1 = Aff0=1
Aff1       = 0;
INTID      = 4;
let val: u64 = ((Aff1 as u64) << 16) | ((INTID as u64) << 24) | (TargetList as u64);
// = 0x0000_0000_0400_0002
```

(0x0000_0000_0400_0002 的来源:INTID 4 在 bits[27:24] = 高 4 位之 0x4,放到字节 3 是 `0x04_00_00_00`;TargetList bit 1 是 `0x00_00_00_02`;Aff1=0 不贡献。)

---

## 为什么字段不按"自然顺序"排

`Aff3 / Aff2 / Aff1 / Aff0` 在 MPIDR 里按从高到低排是 `[63:32] / [23:16] / [15:8] / [7:0]`,直觉上 ICC_SGI1R 应该跟着同样排——但它没有。`ICC_SGI1R_EL1` 把 TargetList(隐含 Aff0)放在最低端,然后 Aff1 / INTID / Aff2 / Aff3 按反向"高低交替"。

这有它的设计理由——`TargetList` + `Aff1` + `Aff2` + `Aff3` 共同定位一个 affinity group,在那个 group 里 TargetList 是 16 位位图(可以一笔多目标);**INTID 跟 affinity 字段交错**是为了让常用的"单 Aff0 group + INTID"场景能用 32 位整数表达——bits[31:0] 就装下了 TargetList + Aff1 + INTID。

代价是字段位置反直觉,**人类阅读 ARM ARM 时大概率读错**。我自己读错过、看别人代码也看到过把 TargetList 写成 bits[23:16] 的(看起来"在中间应该是这个位置")。

---

## 给自己写个 helper

不要直接位运算。每次写 ICC_SGI1R 都用一个有名字字段的 helper:

```rust
fn build_sgi1r(target_aff0_mask: u16, intid: u8, aff1: u8) -> u64 {
    debug_assert!(intid < 16, "SGI INTID must be 0..15");
    (target_aff0_mask as u64)
        | ((aff1 as u64) << 16)
        | ((intid as u64) << 24)
}

fn send_sgi(target_cpu: u8, intid: u8) {
    let val = build_sgi1r(1u16 << target_cpu, intid, 0);
    unsafe {
        asm!("msr icc_sgi1r_el1, {val}",
             "isb",
             val = in(reg) val,
             options(nostack, nomem));
    }
}
```

这套写法的好处:

- **位置错了一个,debug_assert 直接帮你查**(`intid < 16` 这个限制如果你把 INTID 写到 bits[3:0] 等于把它放到了 TargetList 位置,但 SGI INTID 又只能 0-15,所以 mask 之后值也对——这种"看起来对但发到错地方"的 bug 是最难抓的)
- **传 `target_cpu` 是 CPU 编号、不是 mask**——避免把"目标 CPU 1"写成 `1u16` 而不是 `1u16 << 1` 这种 off-by-bit 错误
- **每次回头看代码,字段意思清晰可读**

---

## 一个小推论

任何 GICv3 system register 的位域,**先看 ARM ARM 的位序图,不要看文字描述**。文字描述常常按"逻辑顺序"列字段(Aff3 → Aff2 → Aff1 → Aff0 → INTID),但实际位置可能是反着的。

读位序图时盯着每个字段的 `[high:low]` 范围,**不要根据上下文猜**。`ICC_SGI1R_EL1` 不是孤例——`MPIDR_EL1`、`ICC_BPR1_EL1`、`ICH_LR<n>_EL2` 都有类似"字段位置不按直觉"的情况,具体每个寄存器要单独看。

写代码时给自己留一份位域常量表:

```rust
mod sgi {
    pub const TARGET_LIST_SHIFT: u32 = 0;
    pub const TARGET_LIST_MASK:  u64 = 0xFFFF;
    pub const AFF1_SHIFT:        u32 = 16;
    pub const AFF1_MASK:         u64 = 0xFF << 16;
    pub const INTID_SHIFT:       u32 = 24;
    pub const INTID_MASK:        u64 = 0xF << 24;
    pub const AFF2_SHIFT:        u32 = 32;
    pub const AFF3_SHIFT:        u32 = 48;
}
```

这一份常量表是 ARM ARM 翻成 Rust 的工程沉淀。每读一次手册做对一次,后面就不用再读了。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十九篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
