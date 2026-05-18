# 本周宣发内容 (4/27 - 5/3) — Hypervisor

注：原排程是 4/22/24/26 长文，但 part3 是 4/27 才发，整个 series 顺延一周。
长文落点：**Part 5 周三 4/29、Part 6 周五 5/1、Part 7 周日 5/3**。
预告推文都明确指向"周三 / 周五 / 周日"，不再写"今晚"。

每条推文给出**中文版 + 英文版**，分别用于中文 Twitter 和 EN tech Twitter（HN/r/rust 受众）。

## Twitter 推文（一天一条）

### 周一 4/27 — Part 5 预告（状态机的反直觉）

中文：

```
我以前以为：Rust 的 match 穷尽性，会在状态机扩展时自动替我兜底。

做了 10 周裸机 hypervisor 之后发现：状态机这件事它基本帮不上忙；真正救过我一次的，是另一个我没当回事的 enum dispatch。

周三发长文。

#Rust #ARM64
```

English:

```
I thought Rust's match exhaustiveness would be my safety net for state-machine bugs.

10 weeks into a bare-metal hypervisor: not really. The place it actually saved me was a boring enum dispatch I'd forgotten about.

Post goes up Wednesday.

#rustlang #lowlevel
```

### 周二 4/28 — `_ => fallback` 不是罪

中文：

```
状态机 (from, to) 里我用了 _ => false 兜底。

Rust 教程会说这不够 Rust-y。但 5×5 = 25 对，17 对非法——列出来是 N² 噪音，_ 才是文档。

规则：每次写 _ 都要问"加新 variant 时这里该不该报错？"

#Rust
```

English:

```
`_ => false` for illegal state transitions is fine.

In my SP state machine it's 5×5 = 25 pairs, and 17 are illegal. Listing every bad pair is N² noise. The wildcard is the documentation.

Rule: use `_` only when adding a variant should NOT force a compiler error. #rustlang
```

### 周三 4/29 — Part 5 上线（match 穷尽性）

中文：

```
今天发的长文：Rust 的 match 穷尽性在我这个状态机里没想象中好使。

带字段 enum + 每个 variant 独立 handler 的 dispatch 才是真用得上 match 的地方——加 PL031 RTC 那次，编译器一口气报了 6 个 missing 分支。

链接 👇

#Rust #ARM64
```

English:

```
Just shipped: Rust's match exhaustiveness didn't help in my state machine the way I expected.

Where it DID help: the device-dispatch enum. Adding a PL031 RTC made the compiler bark at 6 missing match arms — in spots I'd long forgotten.

[link] #rustlang
```

### 周四 4/30 — TZ Stage-1 NS=1 PTE（独立技术点，铺垫 Part 6）

中文：

```
S-EL2 往 NWd RX buffer 里写数据，pKVM 却一直读到 0。

不是 FF-A，不是 memcpy：在 Secure 世界下，MMU 关着的访问默认 NS=0，落进的是 Secure 物理地址空间。

修法是开 S-EL2 Stage-1 MMU，把 NWd DRAM 那段 PTE 标 NS=1。

#TrustZone #ARM64
```

English:

```
S-EL2 wrote to a NWd RX buffer. pKVM kept reading zeros.

Not FF-A, not memcpy: in Secure world with MMU off, every access defaults to NS=0 — it lands in the Secure physical address space.

Fix: enable S-EL2 Stage-1 MMU, mark NWd DRAM PTEs with NS=1.

#TrustZone #ARM64
```

### 周五 5/1 — Part 6 上线（TrustZone NS bit）

中文：

```
今天发的长文：3 天 debug 才搞清楚 TrustZone 的 NS 位。

它不只是权限位——它在 ARMv8-A 架构上引入了第二个物理地址空间。同一地址数值，NS=0 和 NS=1 落在两段不同的内存上。

链接 👇

#TrustZone #ARM64
```

English:

```
Just shipped: 3 days of debugging the TrustZone NS bit.

It isn't just a permission bit. ARMv8-A gives you two separate architectural physical address spaces. Same numeric address, but NS=0 and NS=1 are different destinations.

[link] #TrustZone
```

### 周六 5/2 — SMC 不是 barrier（铺垫 Part 7 第二坑）

中文：

```
ARMv8-A: SMC 是同步异常 + Context Synchronization Event，**不保证跨 CPU 内存可见性**。

跨 world + 跨 pCPU 共享 buffer 的稳定写法：dsb sy，先拷到本地，再解析。

读到自洽快照，比直接啃可能脏的数据好 debug 一个数量级。

#ARM64 #嵌入式
```

English:

```
ARMv8-A: SMC is a synchronous exception, not a memory barrier.

Cross-world, cross-pCPU shared buffer? `dsb sy`, copy to a local buffer, then parse that copy.

The real win is bounded parsing over a stable byte slice. #ARM64
```

### 周日 5/3 — Part 7 上线 + 周回顾

中文：

```
今天发的长文：bare-metal Rust 三个"硬件有话说"的坑——NEON popcount、SMC 不是 barrier、QEMU `-bios` 不传 DTB 给 BL33。

本周三篇 hypervisor 长文齐了：match 边界、TrustZone NS、bare-metal 三坑。

下周写两台 VM 互 ping。

https://github.com/willamhou/hypervisor

#Rust #ARM64 #虚拟化
```

English:

```
Just shipped: 3 'hardware has opinions' pitfalls: NEON in debug builds, SMC isn't a barrier, and BL33 x0 isn't always your DTB.

This week: match exhaustiveness, TrustZone NS, bare-metal pitfalls.

Next: two VMs ping each other.

https://github.com/willamhou/hypervisor
#rustlang #ARM64
```

---

## 长文计划

| 日期 | 文章 |
|---|---|
| 4/29 周三 | Part 5: Rust match 穷尽性在状态机里没想象中好使 |
| 5/1 周五 | Part 6: 3 天 debug — TrustZone 的 NS 位是一根总线信号 |
| 5/3 周日 | Part 7: 裸机 Rust 三个"硬件有话说"的坑 |
