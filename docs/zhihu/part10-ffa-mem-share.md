# 一块 4 KB 内存,从 pKVM 到 SP,再回来

pKVM 在 Normal 世界发了一条 SMC,函数号 `0x84000073`,FF-A `MEM_SHARE`。它说:这块 4 KB 内存,从 IPA `0x4200_0000` 起,我借给 SP2。

那块内存里此刻有半张 Android 启动用的字体位图。pKVM 想让 SP2 能读到它。它不打算让 SP2 写——读完用完就还回来,所以不是 LEND,不是 DONATE,是 SHARE。

这条 SMC 走过 EL3 的 SPMD,落到 S-EL2 我这边。从这一刻开始,这块 4 KB 内存就要换三次属性、记三笔账、过六个 SMC 才能回到原状。本篇讲这六个 SMC 怎么走完一遍。

代码主线在 `src/ffa/memory.rs`、`src/ffa/stage2_walker.rs`、`src/spmc_handler.rs` 的 `handle_spmc_mem_*` 这一组函数里。

---

## 先把两本账亮开

记账有两本。**第一本在 pKVM(发送方)的 Stage-2 页表里**,标这块内存现在是什么所有权状态。

四个状态,占 PTE 的 SW bits[56:55],两个比特够用:

```rust
// src/ffa/memory.rs
pub enum PageOwnership {
    Owned          = 0b00,  // 还在我手上,没分享给谁
    SharedOwned    = 0b01,  // 我分享出去了,但所有权还在我
    SharedBorrowed = 0b10,  // 我借来用的,别人是真主人
    Donated        = 0b11,  // 我永久转让出去了,不可逆
}
```

为什么放 PTE 里、不放别处?因为内存共享要做的事大多绕着页表走——查所有权、改访问权、解映射——把状态贴在 PTE 上,这些操作都不用查第二张表。SW bits[56:55] 是 ARM Stage-2 PTE 里专门留给软件用的两个比特,跟硬件翻译完全不冲突。

**第二本账在我这边**,记录这一次 SHARE 的元数据——句柄、发送方、接收方、范围:

```rust
// src/spmc_handler.rs
struct SpmcShareRecord {
    handle: u64,                                 // 64 位句柄
    sender_id: u16,                              // 谁发起的
    receiver_id: u16,                            // 给谁
    ranges: [(u64, u32); MAX_SHARE_RANGES],      // (起始 IPA, 页数) × N
    range_count: usize,
    active: bool,
    is_lend: bool,                               // SHARE / LEND / DONATE 三选一
    is_donate: bool,
    retrieved: bool,                             // 接收方拿走没
}
```

这本账放在 SPMC 的全局表里:

```rust
static SPMC_SHARES: SpinLock<[SpmcShareRecord; MAX_SPMC_SHARES]> = ...;
```

两本账要一直对上。哪一本错位,后续就没法收尾。

---

## 第一个 SMC:MEM_SHARE

pKVM 的 SMC 进来,x0 = `0x84000073`,x1 = 总长度,x2 = 第一片长度。描述符放在 pKVM 之前注册过的 TX buffer 里。

描述符不能直接原地读。原因 [Part 4](./part4-war-stories.md) 讲过——跨核 + 跨世界共享 buffer,内存可见性不可靠。这里我先 `dsb sy`,然后 `copy_nonoverlapping` 整块拷到本地栈缓冲,所有解析只读本地副本:

```rust
unsafe { core::arch::asm!("dsb sy", options(nostack, nomem)) }
let mut local_buf = [0u8; 4096];
unsafe {
    core::ptr::copy_nonoverlapping(
        tx_pa as *const u8,
        local_buf.as_mut_ptr(),
        total_length as usize,
    );
}
```

解析按 FF-A v1.1 规范 DEN0077A Table 5.19-5.25 嵌套结构走:

```
FfaMemRegion (48 B 头)
  ├── sender_id
  ├── attributes (cacheability / shareability)
  └── FfaMemAccessDesc (16 B)
        ├── receiver_id
        ├── permissions (RW / RO)
        └── FfaCompositeMemRegion (16 B)
              └── FfaMemRegionAddrRange × N (16 B 每段)
                    ├── start_ipa
                    └── page_count
```

每个结构都是 `#[repr(C, packed)]`。解析用 `core::ptr::read_unaligned` 单字段读,不能直接 deref——packed 字段不对齐,Rust 的 `MaybeUninit::assume_init_read` 在 ARM 上会塞 NEON 对齐检查([Part 7](./part7-bare-metal-rust-pitfalls.md) 那个坑)。

解析出来:`sender=pKVM(0x0)`、`receiver=SP2(0x8002)`、`ranges=[(0x42000000, 1)]`。

接下来要做的事**只剩一件**:验证发送方真的拥有这块内存。

验证靠 pKVM 这边的 Stage-2 SW bits。我从 `PER_VM_VTTBR[pKVM_id]` 拿到 pKVM 的页表根,构造一个 `Stage2Walker`(`src/ffa/stage2_walker.rs`),按 IPA 走表读出 PTE,从 PTE 取 SW bits:

```rust
let walker = Stage2Walker::from_vttbr(pkvm_vttbr);
let sw_bits = walker.read_sw_bits(0x42000000).ok_or(FFA_INVALID_PARAMETERS)?;
validate_page_for_share(sw_bits)?;     // 要求 Owned
```

`validate_page_for_share` 只在 `Owned` 时返回 `Ok`。其他三种状态都不能再 SHARE——不能把别人借给你的东西转借给第三个人,这是协议级别的约束,不是工程约束。

验证通过之后,改状态:pKVM 这边的 SW bits 从 `Owned` 翻成 `SharedOwned`。这一笔是 atomic 的(`walker.write_sw_bits()`,带 cmpxchg)。S2AP 同时收紧——SHARE 的语义是"我借出去,自己不写",所以把发送方的访问权改成只读:

```rust
walker.write_sw_bits(ipa, PageOwnership::SharedOwned as u8)?;
walker.write_s2ap(ipa, S2AP_RO)?;
```

第二本账也写上:`SpmcShareRecord` 填好,`active=true`,`retrieved=false`,分配一个 64 位句柄返回给 pKVM。

```
x0 = FFA_SUCCESS
x2 = handle & 0xFFFFFFFF
x3 = handle >> 32
```

到这,内存还在原地,pKVM 那边的 PTE 已经变了——能读不能写。一笔账记下,等着 SP2 来取。

---

## 第二个 SMC:SP2 的 RETRIEVE_REQ

SP2 那边还不知道有这块内存。要 pKVM(或者 NWd 的某个组件)把句柄传给 SP2,SP2 自己发一条 `MEM_RETRIEVE_REQ` 来"取":

```
x0 = FFA_MEM_RETRIEVE_REQ (0x84000074)
x1 = handle low 32
x2 = handle high 32
+ 描述符在 SP2 的 TX buffer 里(谁是 receiver、能不能 RW 等等)
```

这条 SMC 是 SP2 主动发的,但 SP2 跑在 S-EL1。它的 SMC 进 S-EL2(我),不进 EL3——这是 Secure World 内部调用。

我在 `handle_spmc_mem_retrieve()` 里做两件事。

第一件:查账。按句柄查 `SPMC_SHARES`,确认 `receiver_id == 0x8002`,确认 `retrieved == false`(同一个 receiver 不能 retrieve 两次)。

第二件,也是关键的一件:**真正把这块内存映射到 SP2 的 Secure Stage-2 里**。在此之前,SP2 的 Stage-2 完全没这个 IPA。从这条 SMC 之后,SP2 才能用普通 load/store 访问它。

`Stage2Walker` 的 `map_page()` 干的就是这件事:

```rust
// src/spmc_handler.rs:2598
walker.map_page(ipa, 0b11, 0b10);  // S2AP_RW, SW=SharedBorrowed
```

`0b11` 是 RW 访问权——SP2 不只能读,如果协议允许,也能写。`0b10` 是 `SharedBorrowed`,标志这块内存是借来的,不是 SP2 自己的。

为什么 SP2 这边可以 RW?因为 FF-A 的访问权是两边对称的——pKVM 端 SHARE 时收紧到 RO(我自己不写),意思是允许 receiver 写(否则就该 LEND 了)。SP2 RETRIEVE 时拿到 RW,语义自洽。

`map_page()` 实现里要分配 Stage-2 的 L2/L3 表(从 SPMC 的 secure heap),把 4 KB 单页插进去。表如果不存在还要先建——`walk_or_create()`。整个过程加 `STAGE2_LOCK`,因为 pKVM 那边的 per-CPU SPMD 可能让两条不同 CPU 的 SMC 同时撞进来。

最后,`SpmcShareRecord.retrieved = true`。SP2 那边收到 RESP,带回描述符——告诉 SP2 这块内存在它 Stage-2 里的 IPA 在哪。

SP2 现在能读这块字体位图了。pKVM 端的物理内存没动一根毫毛。

---

## 中间这段,SP2 在用

SP2 在这段时间里做它要做的事——读字体,处理,可能也写一些状态回去。它的 `load`/`store` 都走自己的 Secure Stage-2,翻译到同一块物理地址,跟 pKVM 看到的是同一块内存,但属性、所有权、访问权都是各自记的。

注意 pKVM 这时候**也还能读**。它的 SW bits 是 `SharedOwned`、S2AP 是 RO——能读不能写。SHARE 的"共享"是真共享,不是"借出去就没了"。如果它在这时候要写,会被 Stage-2 拦下,触发 fault。

LEND 不一样。LEND 的语义是"借走之后我自己也不能碰",所以 LEND 走的路径里,发送方的 S2AP 收紧到 `0b00`(None):

```rust
// LEND path
walker.write_sw_bits(ipa, PageOwnership::SharedOwned as u8)?;
walker.write_s2ap(ipa, S2AP_NONE)?;   // ← LEND 的关键差别
```

`SharedOwned` 状态相同(都是"我分享出去但所有权还在"),区别只在 S2AP 那一位。

DONATE 又不一样。DONATE 是不可逆转让,发送方直接 unmap——`walker.unmap_page(ipa)`——这块内存就在发送方的 Stage-2 里**消失了**。SW bits 写成 `Donated`,但其实没什么用,因为 PTE 已经无效。之所以保留这个状态值,是兼容性:接收方那边要标 `Donated`,沿用同一套编码。

DONATE 之后 `is_donate=true` 写进 `SpmcShareRecord`。后面的 RELINQUISH 和 RECLAIM 看到这个 flag 都返回 `FFA_DENIED`——DONATE 是单程票。

---

## 第三个 SMC:SP2 的 RELINQUISH

SP2 用完了。发 `MEM_RELINQUISH`:

```
x0 = FFA_MEM_RELINQUISH (0x84000076)
+ 句柄在 SP2 的 TX buffer 里(描述符格式比 RETRIEVE_REQ 简单)
```

`handle_spmc_mem_relinquish()` 干的事是 `map_page()` 的逆操作:**把这块内存从 SP2 的 Secure Stage-2 里 unmap**。

```rust
walker.unmap_page(ipa)?;  // 把对应 L3 PTE 清零
```

清完之后 `SpmcShareRecord.retrieved = false`。但 `active` 还是 `true`——账还没销,只是 receiver 那边放手了。pKVM 现在可以回收,也可以让别的 SP 再 RETRIEVE。

`unmap_page()` 还做一件保险的事:**清完 PTE 后跑一次 TLBI**(`tlbi vae2is`),把这条映射从所有 Inner Shareable 域的 CPU TLB 里赶出去。不清的话,SP2 在某个 pCPU 上的 TLB 里还有这一项,即使 PTE 已经清零,它还是能用旧映射访问——直到下一次 context 切换。

---

## 第四个 SMC:pKVM 的 RECLAIM

pKVM 想把内存彻底要回来。发 `MEM_RECLAIM`:

```
x0 = FFA_MEM_RECLAIM (0x84000077)
x1 = handle low 32
x2 = handle high 32
```

`handle_spmc_mem_reclaim()` 做三件事。

第一,查账。`SPMC_SHARES` 里按句柄找,确认 `sender_id` 是当前调用者(pKVM),确认 `retrieved == false`——SP2 还没放手时不能 reclaim,返回 `FFA_DENIED`。

第二,改 pKVM 端的 Stage-2 SW bits 和 S2AP:

```rust
walker.write_sw_bits(ipa, PageOwnership::Owned as u8)?;
walker.write_s2ap(ipa, S2AP_RW)?;
```

`Owned` 回来了,RW 也回来了。pKVM 这边的页表完全恢复到 SHARE 之前的状态。

第三,销账:`SpmcShareRecord.active = false`。槽位释放,等下一次 SHARE 来用。

---

## 这六个调用之间不能错位

SHARE 之后没 RETRIEVE,RELINQUISH 走不通(根本没什么可 RELINQUISH 的)。RETRIEVE 之后没 RELINQUISH,RECLAIM 走不通(SP 还在用,RECLAIM 出去 SP 会读到无效映射)。DONATE 之后没有 RELINQUISH 也没有 RECLAIM——单程票。

每条路径都在 `SpmcShareRecord` 上做一次状态检查:`active`、`retrieved`、`is_donate`。出错了就 `FFA_DENIED`,不修改任何状态。这一点很重要——FF-A 没有"撤销"操作,任何中间状态如果让错误的 SMC 改了一半,后续就没法恢复。

```rust
// RELINQUISH 入口的检查链
if !record.active                  { return DENIED; }   // 已 reclaim
if !record.retrieved               { return DENIED; }   // 还没 retrieve
if record.is_donate                { return DENIED; }   // 不可逆
if record.receiver_id != caller    { return DENIED; }   // 不是你借的
```

每条 SMC 都自带这一段。看起来重复,但写出来不能省——FF-A 的合规靠这一层兜底。

---

## 跨 NWd 的 fragmentation

实战里描述符常常不止一段。一个 MEM_SHARE 可能包 16 个 range,描述符长度超过单条 SMC 能携带的 4 KB TX buffer。FF-A 给了一对调用处理这件事:**`MEM_FRAG_TX`** 是发送方继续传剩余分片,**`MEM_FRAG_RX`** 是接收方继续取剩余分片。

发送方 fragmentation 这边的代码:

```rust
// 第一片进来,total_length > fragment_length
if total_length != fragment_length && fragment_length > 0 {
    // 启动重组
    frag.total_length = total_length;
    frag.received = fragment_length;
    frag.handle = spmc_alloc_handle();
    frag.active = true;
    // 返回 FFA_MEM_FRAG_RX,告诉发送方继续发
    return SmcResult8 {
        x0: FFA_MEM_FRAG_RX,
        x1: handle & 0xFFFF_FFFF,
        x2: handle >> 32,
        x3: fragment_length,
        ...
    };
}
```

之后发送方按句柄继续 `MEM_FRAG_TX`,我累加进 `NWD_FRAG.accum_buf`,直到 `received == total_length`。这时候才真正做完整的 share 解析、Stage-2 映射、记账。

中途任何一片格式有问题——比如 `received > total_length`、`fragment_length > total_length`、句柄不对——重组直接作废,`NWD_FRAG.active = false`,返回错误。绝不能让半截描述符进到真实路径里,因为它会让后面的 IPA 校验过不去,但已经被错误地分配了句柄。

---

## 关于 PTE SW bits 的一个小观察

四个状态用两个比特,刚好够。但仔细想想其实有一个问题——**借来的内存不能再借出去**这条规则,是协议级的,代码里通过 `validate_page_for_share` 强制("非 Owned 不许 SHARE")。那 `SharedBorrowed` 这个状态在发送方端永远不会出现——它只在接收方端有意义。

这就是为什么 receiver 在 RETRIEVE 时映射的 PTE 直接打成 `SharedBorrowed`(`0b10`)——它从 SP2 的视角看,这块内存确实是借来的。SP2 自己如果想再 SHARE 给 SP3,代码会读它的 PTE 拿到 `SharedBorrowed`,`validate_page_for_share` 返回 `FFA_DENIED`,转嫁不成立。

ARM 留这两个 SW bits 给软件用,有人用来做 PTE 缓存标记、有人用来做引用计数。这套用来做所有权状态机,刚好够,而且天然 per-page 颗粒度——没有第二张表要维护一致性,加上 atomic 写,多 pCPU 下也不会撕裂。

---

## 收尾

`MEM_SHARE` 走完一圈,六个 SMC、两本账、四个状态。听起来繁琐,实际上是 FF-A 把"两个互不信任的 hypervisor 共享一块内存"这件事做对的最低成本——任何一步省掉,要么内存泄漏(句柄不销),要么权限混乱(S2AP 不收紧),要么有人能读到本不该读的内容(SW bits 不查)。

代码量上,`memory.rs` + `stage2_walker.rs` + `spmc_handler.rs` 里的六个 handler,加起来大约 1500 行。看起来不少,但每一行都对着 FF-A 规范的某一个语义,删一行就有一个语义不成立。这种"接口规约驱动"的代码密度,跟普通业务逻辑的那种"一行写不完两件事"很不一样——这边的每一行都是合规义务。

下一篇,我打算讲 GICv3 的虚拟化:四个 List Register、HW=1 怎么把物理 vtimer EOI 自动透传给虚拟世界、EOImode 把优先级降权和去激活拆成两步。FF-A 是协议,GICv3 是硬件——下一篇是硬件这条线。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
