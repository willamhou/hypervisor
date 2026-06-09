# FF-A 描述符的四层套娃,以及包不下一个 SMC 时怎么办

[Part 10](./part10-ffa-mem-share.md) 讲了 MEM_SHARE 走完一遍是怎样的流程——六个 SMC、两本账、四个状态。这一篇换个角度,讲那一笔 MEM_SHARE 真正塞进 TX buffer 的**那段二进制**长什么样、怎么 parse,以及描述符长到放不下一条 SMC 的时候,**MEM_FRAG_TX / MEM_FRAG_RX** 这对调用怎么把它切开传过来。

FF-A 文档(DEN0077A Table 5.19-5.25)把这些结构画在表里,**字段偏移、字节宽度、含义**列得明白,但落到代码上的几个细节——比如 `#[repr(C, packed)]` 怎么避免对齐坑、RXTX mailbox 的 `rx_held_by_proxy` 状态机、分片状态机怎么用句柄串起来——都是文档不会教的。

主线在 `src/ffa/descriptors.rs`、`src/ffa/mailbox.rs` 和 `src/ffa/proxy.rs` 的几个 fragment state 那一段。

---

## 一个真实的描述符,层层剥开

NWd 用 MEM_SHARE 共享一段两个 range 的内存给 SP1。它把这个描述符写在自己的 TX buffer 里。二进制布局(48 + 16 + 16 + 16×2 = 112 字节):

```
Offset 0     [FfaMemRegion]                            48 字节头
       0:    sender_id (u16)        = 0x0000           (NWd)
       2:    attributes (u16)       = ...              (cacheability / shareability)
       4:    flags (u32)            = ...
       28:   receiver_count (u32)   = 1
       32:   receivers_offset (u32) = 48               (第一个 receiver desc 在哪)

Offset 48    [FfaMemAccessDesc]                        16 字节
       0:    receiver_id (u16)      = 0x8001           (SP1)
       2:    permissions (u8)       = ...              (RW)
       4:    composite_offset (u32) = 64               (composite desc 在哪)
       8:    flags (u64)            = 0

Offset 64    [FfaCompositeMemRegion]                   16 字节
       0:    total_page_count (u32) = 3                (两个 range 加起来 3 页)
       4:    address_range_count (u32) = 2             (两个 range)
       8:    reserved (u64)         = 0

Offset 80    [FfaMemRegionAddrRange × 2]               16 × 2 = 32 字节
       0:    address (u64) = 0x42000000, page_count = 1
       16:   address (u64) = 0x42010000, page_count = 2
```

四层套娃。**`FfaMemRegion`** 是顶层 header,记总体信息(发送方、receiver 列表在哪)。**`FfaMemAccessDesc`** 一个 receiver 一份,记某个特定 receiver 的访问权和它对应的 composite descriptor 在哪。**`FfaCompositeMemRegion`** 是内存区域的元信息(总页数 + 子区段数)。**`FfaMemRegionAddrRange`** 是真正的 (addr, page_count) 对,可以多个,描述非连续内存。

为什么不直接拍平?因为同一段内存可以同时共享给多个 receiver,每个 receiver 的权限不同——**`FfaMemAccessDesc` 一对多**,共享同一个 composite 但权限各自记。

---

## `#[repr(C, packed)]` 跟 `read_unaligned` 的搭配

四个结构在 Rust 里都标了 `#[repr(C, packed)]`:

```rust
// src/ffa/descriptors.rs
#[repr(C, packed)]
pub struct FfaMemRegion {
    pub sender_id: u16,
    pub attributes: u16,
    pub flags: u32,
    // ... 一直到 48 字节
}

#[repr(C, packed)]
pub struct FfaCompositeMemRegion {
    pub total_page_count: u32,
    pub address_range_count: u32,
    pub reserved: u64,
}
```

`packed` 让结构体按 1 字节对齐(打消编译器为了对齐自动加 padding),这是规范要求的——FF-A 描述符在不同 endian / 不同 word size 的实现之间要二进制兼容,padding 不能由编译器决定。

代价是:**字段读出来不能直接 `&desc.composite_offset`**——packed 字段地址可能不对齐,在 ARM64 上对一个 misaligned `*const u32` 直接 deref 是 UB,Rust 给你警告或者直接拒绝编译。

正确做法是 `core::ptr::read_unaligned`:

```rust
// src/ffa/descriptors.rs:131-132
let receiver_count = core::ptr::read_unaligned(tx_ptr.add(28) as *const u32);
let receivers_offset = core::ptr::read_unaligned(tx_ptr.add(32) as *const u32);
```

`read_unaligned` 在硬件层用字节级 load 凑出来,**不会 emit 任何依赖对齐的指令**——具体说,不会 emit `ldr w0, [x1]`(这条要求 4 字节对齐),会 emit 一串 `ldrb` 凑成一个 32 位字。性能略差,但对 packed FF-A 描述符这种"只 parse 一次再丢弃"的场景完全可接受。

[Part 7](./part7-bare-metal-rust-pitfalls.md) 那个 NEON 坑里讲过,`read_volatile` 在 Rust debug 模式会塞 NEON 对齐检查。`read_unaligned` 是另一条路径,不走对齐检查——但它跟 `read_volatile` 不能合用,如果你要"既 volatile 又 unaligned"得自己用 `core::arch::asm!` 写一串 `ldrb`。

---

## RXTX Mailbox:三个状态位串起一对 buffer

FF-A 每个 endpoint(VM 或 SP)发起任何"内容超过 8 个 64 位寄存器能装下"的调用,都得先注册一对 buffer——TX(自己写、对方读)和 RX(对方写、自己读)。这是 `FFA_RXTX_MAP`。

我们这边 per-VM 存的 `FfaMailbox` 结构:

```rust
// src/ffa/mailbox.rs
pub struct FfaMailbox {
    pub tx_ipa: u64,                  // guest TX buffer IPA
    pub rx_ipa: u64,                  // guest RX buffer IPA
    pub page_count: u32,              // 通常是 1
    pub mapped: bool,                 // 注册了没
    pub rx_held_by_proxy: bool,       // RX 当前归谁
    pub msg_pending: bool,            // RX 里有没有未读消息
    pub msg_sender_id: u16,           // 那条消息的发送方
}
```

最关键的是 `rx_held_by_proxy`。FF-A 的 RX buffer 有"所有权"概念——**写 RX 的人和读 RX 的人不能同时碰**。初始注册之后 RX 归 proxy 所有(proxy 可以往里写,作为 PARTITION_INFO_GET 之类调用的响应);proxy 写完之后通过 `FFA_SUCCESS` 把所有权交给 VM(VM 可以读);VM 读完之后发 `FFA_RX_RELEASE` 把所有权交还给 proxy。

```
状态机:
    proxy_owned --[proxy writes + returns success]--> vm_owned
    vm_owned --[FFA_RX_RELEASE]--> proxy_owned
```

如果 proxy 在 `vm_owned` 状态下再次想往 RX 写,得返回 `FFA_BUSY` 让调用方稍后再试。这是 FF-A 防止 race 的简单办法——基于状态位的所有权,而不是真的加锁(锁的对象是 buffer,锁的边界跨调用方很难定义)。

`msg_pending` + `msg_sender_id` 这两个字段是给 `FFA_MSG_SEND2`(间接消息)用的——proxy 把发送方塞进 RX,等接收方主动 `FFA_MSG_WAIT` 来取。轮询模型。

---

## 分片:描述符 > 4 KB 怎么办

NWd 这次的 MEM_SHARE 要共享 30 段不连续的内存给 SP1。每段一个 `FfaMemRegionAddrRange` 占 16 字节,加上 48+16+16 字节的三层 header,描述符长度算出来 580 字节——还行,塞得进 4 KB TX buffer。

但如果是 100 段、300 段呢?或者多个 receiver,每个 receiver 一份 `FfaMemAccessDesc` + 一份 composite?到 4 KB 装不下的时候,FF-A 给的解法是 **fragmentation**:

- 发送方第一片塞进 TX 里发 `MEM_SHARE`,带上"总长度 total_length"和"这一片长度 fragment_length"
- proxy 收到,如果 `total_length > fragment_length`,意识到这是第一片,返回 `FFA_MEM_FRAG_RX` 和一个临时 handle,**告诉发送方继续**
- 发送方下次发 `MEM_FRAG_TX`,带 handle 和下一段
- proxy 累加到本地 buffer,直到 `received == total_length`
- 这时候才**真正**做 share 解析、Stage-2 映射、记账

代码里维护这个状态机的是 `FragmentState`(发送侧)和 `FragRxState`(接收侧):

```rust
// src/ffa/proxy.rs
struct FragmentState {
    active: bool,
    handle: u64,
    total_length: u32,
    received: u32,
    accum_buf: [u8; 4096],
    is_lend: bool,
    is_donate: bool,
    sender_id: u16,
}
```

per-VM 一个状态。原因是同时刻一个 VM 至多有一笔 MEM_SHARE 在分片中(由 handle 串联),而不同 VM 互不影响。

接收侧的 `FragRxState` 是镜像问题:`MEM_RETRIEVE_REQ` 的 RESP 描述符比 RX buffer 大时,receiver 第一次拿到的是头部,然后通过 `MEM_FRAG_RX` 一次次取剩余:

```rust
struct FragRxState {
    active: bool,
    handle: u64,
    total_length: u32,
    delivered: u32,
    sender_id: u16,
}
```

发送和接收的分片各自有 5 个字段、各自有状态机,而且**两边都得做边界校验**——`fragment_length > total_length`、`received > total_length`、`delivered > total_length`、`handle` 对不上,全部返回错误。这种"FF-A 规范定义错误码、实现负责具体校验"的代码很多,占整个 FF-A 模块约一半行数。

---

## 接收侧分片:两个状态机在 RX 那一页上撞到一起

receiver 这边发 `MEM_RETRIEVE_REQ`,proxy 写了头一片到 RX,把 RX 移交给 VM 读;VM 读完发 `FFA_RX_RELEASE`,RX 还给 proxy;然后 VM 发 `MEM_FRAG_RX` 要下一片,proxy 再写。这中间**两套状态机**在动:`FragRxState` 跟 handle 走,记"这一笔 RETRIEVE 已经送出去多少字节";`FfaMailbox.rx_held_by_proxy` 跟 RX page 走,记"现在 RX 归谁"。

如果两套状态对不齐——比如 VM 还没 release RX,proxy 想写下一片——proxy 这边的 `MEM_FRAG_RX` 处理就要返回 `FFA_BUSY`,逼 VM 先做 RX_RELEASE。FF-A 把"协议层的分片状态"跟"传输层的 buffer 所有权"分开维护,正是为了让这种 race 有明确的恢复路径——出错就 BUSY,VM 自己来选什么时候解。

---

## 收尾

FF-A 协议表面是"几十个 SMC 函数 ID"。真正复杂的是这些 SMC 之间共享的状态——RXTX 所有权、分片重组、句柄追踪、描述符 packed 解析。**规范定义状态,实现把状态分配到具体字段,每加一笔 FF-A 调用都要看清这些状态怎么动**。

代码里 `descriptors.rs` + `mailbox.rs` + `proxy.rs` 加起来约 2000 行,大部分是状态校验和边界 check。如果只看"功能本身",可能 600 行就够。多出来那 1400 行,是 FF-A 把"两个互不信任的 endpoint 通过共享 buffer 交换数据"这件事做对的代价。

下一篇我想讲 virtio-blk 和 virtio-net 从零搭起来——virtqueue 描述符链怎么走、`virtio_net_hdr_v1` 那 12 字节前缀容易踩、RX 异步注入怎么用 SPSC ring 解开 producer/consumer 边界。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十五篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
