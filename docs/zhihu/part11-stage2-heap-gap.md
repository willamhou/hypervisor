# 把堆放在 guest 看得到的物理范围里——然后把它藏起来

我的 hypervisor 把堆放在 `0x4100_0000`。16 MB。这块物理地址在 guest 声称的 RAM 范围里——guest 看到的是从 `0x4000_0000` 开始的一大段 RAM。

但 guest 永远摸不到 `0x4100_0000`。

不是因为 guest 自我克制,不是因为我用 TrustZone 的 NS bit 隔了它,也不是因为代码里有 if-check 拦截。是因为 hypervisor 的 Stage-2 页表里**那一段根本没映射**——guest 用 IPA `0x4100_0000` 发起的任何访问都触发 Stage-2 fault,被 hypervisor 接管,拒绝。

这一篇讲为什么要这样做、怎么做、以及做到一半发现 GICR 需要 4 KB 精度的时候,2 MB 块的 IdentityMapper 怎么演化成支持 2 MB / 4 KB 混合的 DynamicIdentityMapper。

主线代码在 `src/arch/aarch64/mm/mmu.rs` 里那两个 mapper 类型,以及 `split_2mb_block()` 那一段。

---

## 为什么 guest 不会撞到堆

Guest Linux 启动靠 DTB 来知道 RAM 在哪。我们给 guest 的 DTB 里 `/memory` 节点写的是这样:

```
memory@48000000 {
    device_type = "memory";
    reg = <0x0 0x48000000 0x0 0x40000000>;  // 0x4800_0000 起,1 GB
};
```

Linux 启动时 parse 这一条,把它的物理内存分配器(`memblock`)初始化成"可用 RAM 是 `0x4800_0000` 到 `0x8800_0000`"。从这一刻开始,Linux 的 `alloc_pages()` 不会返回 `0x4100_0000` 附近的地址——它根本不知道那里有 RAM 可用。

那段地址确实是 RAM——QEMU 的 `-m 2G` 让 `0x4000_0000` 起的 2 GB 都有真实 DRAM 在背后。Linux 不去访问只是因为 DTB 没告诉它。

但**安全不能靠 guest 自觉**。Linux 内核里有 bug、有越界、有指针计算错位。如果某条野指针刚好踩到 `0x4100_0001`,我们的 hypervisor 页表就被踩了——`l0_table`、`l1_table`、`l2_tables` 全在堆里,改一个比特,下一次进 guest 的 Stage-2 翻译就疯。

所以 Stage-2 页表里那一段必须不映射。DTB 把 Linux 引到 `0x4800_0000`;真有人伸手到 `0x4100_0000`,Stage-2 fault 当场拦下。一前一后,两层防御。

```
guest 看到的 RAM:    [    0x40000000  ......  0x88000000   ]
                                ^
                              堆在这里 (0x41000000, 16MB)
                              但 Stage-2 不映射 → fault
guest 实际用的 RAM:  [             0x48000000  ......  0x88000000 ]
                                ^
                              Linux 从这里开始 alloc
```

heap 既不在 guest 用的范围 (Linux 的 memblock 不知道它),也不在 hypervisor 视角隔离区域 (它本来就是 hypervisor 自己的)。设计上这是一段"在 RAM 物理范围但不可达"的 deliberate gap。

---

## IdentityMapper 第一版:静态 2 MB 块

最早的 Stage-2 mapper 是这样的——hypervisor 启动时,把 `0x4800_0000` 到 `0x8800_0000` 这 1 GB 标成 RAM 可访问,每个映射条目是一个 2 MB 块。512 条 PTE 装在一张 4 KB 的 L2 表里,加上 L0 / L1 两张表指过去,一共三张页表,完全 static 分配——不需要堆。

```rust
pub struct IdentityMapper {
    l0_table: u64,
    l1_table: u64,
    l2_table: u64,  // 单张 L2,512 个 2MB 块 = 1GB
}
```

2 MB 块的好处一目了然:L2 一格就是 2 MB,TLB 轻,走表只到 L2 那一层就停,启动时一张表填完就完事。

代价也摆在同一格里:4 KB 的 GICR 不能单独剔出去。一个 2 MB 块要么整块映射、要么整块不映射,中间挖不出一个 4 KB 的洞——而 GICR 的 trap 恰好需要这种洞。

第一版 hypervisor 还没有 GICR 虚拟化,这个限制不重要。等到要加 trap-and-emulate GICR 的时候,这一版就走不下去了。

---

## 演进:GICR 是 4 KB,需要 4 KB 粒度的 Stage-2 控制

GICv3 的 Redistributor (GICR) 是每个 CPU 一个 0x2_0000 (128 KB) 的 MMIO 区域,但里面的 SGI / PPI 子页只占 4 KB。我们要 trap GICR 是因为 guest 直接写它会改物理中断状态,跟我们 SPI 路由表对不上。

trap 的实现方式是:**把 GICR 那 4 KB 页从 Stage-2 里 unmap**。Guest 访问就触发 Data Abort,hypervisor 接管,在 `VirtualGicr` 的 shadow 状态上模拟一遍读写,然后透传给物理 GICR(如果需要),最后 ERET 回去。

要 unmap 4 KB,Stage-2 在那一块必须是 4 KB 粒度。但 2 MB 块覆盖了它——整个 2 MB 块 unmap 会把周围合法的 GIC 区域也带走。

唯一的办法是把那个 2 MB 块**拆成 512 个 4 KB 页**。拆完之后,512 个 4 KB PTE 里有 511 个保留原映射、1 个标 invalid。访问到 invalid 那一页才 fault,其他 511 页继续直通硬件。

但 IdentityMapper 没办法做这件事——它的 L3 表根本没分配过。要拆 2 MB 块,你得能动态从 heap 拿一张新的 4 KB 页做 L3 表,把 512 个 4 KB PTE 写进去,然后改 L2 entry 让它指向新的 L3 表。

于是有了 DynamicIdentityMapper。

---

## DynamicIdentityMapper:heap-backed,2 MB / 4 KB 都行

```rust
pub struct DynamicIdentityMapper {
    l0_table: u64,
    l1_table: u64,
    l2_tables: [u64; 4],  // 最多 4 张 L2,覆盖 4 GB
    l2_count: usize,
}
```

跟 IdentityMapper 的区别有两个。第一,`l2_tables` 是 `[u64; 4]`——四个槽位,每个槽位装一个 L2 表的物理地址,可以按需分配多张(每张覆盖 1 GB),启动时分配一张就够。第二,所有页表都从 heap 拿(`crate::mm::heap::alloc_page()`),而不是 static `.bss`。

`map_region()` 仍然按 2 MB 块走,跟老版本接口一样。新增的是 `map_4kb_page()` 和 `unmap_4kb_page()`,这两个会触发拆块。

```rust
pub fn map_4kb_page(&mut self, ipa: u64, attr: MemoryAttribute) -> Result<(), &'static str> {
    let l1_idx = ((ipa >> 30) & PT_INDEX_MASK) as usize;
    let l2_table = self.get_or_create_l2(l1_idx)?;
    let l2_idx = ((ipa >> 21) & PT_INDEX_MASK) as usize;
    let l3_idx = ((ipa >> 12) & PT_INDEX_MASK) as usize;

    let l2_entry = Self::read_pte(Self::entry_ptr(l2_table, l2_idx));

    let l3_table = if l2_entry & PTE_VALID != 0 && l2_entry & PTE_TABLE == 0 {
        // L2 entry 是 2 MB 块 — 拆成 L3 表
        self.split_2mb_block(l2_table, l2_idx, l2_entry)?
    } else if l2_entry & (PTE_VALID | PTE_TABLE) == (PTE_VALID | PTE_TABLE) {
        // L2 entry 已经指向 L3 表,直接用
        l2_entry & PTE_ADDR_MASK
    } else {
        // L2 entry 无效 — 新建空 L3 表
        let l3 = crate::mm::heap::alloc_page().ok_or("Failed to allocate L3 table")?;
        unsafe { core::ptr::write_bytes(l3 as *mut u8, 0, PAGE_SIZE) }
        Self::write_pte(Self::entry_ptr(l2_table, l2_idx), l3 | PTE_VALID | PTE_TABLE);
        l3
    };
    // ... 写 L3 PTE
}
```

三个分支按 L2 entry 当前状态走。第三个最简单——L2 那里本来就没映射,直接装一张空 L3 表上去。第二个也简单——L3 表已经存在,直接用。

麻烦的是第一个分支:`split_2mb_block`。

---

## split_2mb_block:拆 2 MB 块的细节

把一个 2 MB 块拆成 512 个 4 KB 页要走 5 步,**不能省任何一步**:

1. 从 heap 拿一张新的 4 KB 页做 L3 表
2. 把这张 L3 表填上 512 个 PTE,每个指向 2 MB 块原本覆盖的对应 4 KB 物理地址,继承原来的属性
3. **把 L2 entry 写成 0**(invalid)
4. **TLBI**——把旧的 2 MB block 映射从 TLB 里赶出去
5. 把 L2 entry 改成"指向 L3 表"(从 block descriptor 改成 table descriptor),再 TLBI 一次

第 3、4 步是 ARMv8-A D5.10.1 明文要求的——任何把一个 valid PTE 替换成另一个 valid PTE 的操作,中间必须经过一次 invalid 状态加 TLBI,否则可能触发 TLB conflict abort。原因是 TLB 可能同时缓存"旧块 PTE"和"新表 PTE",硬件不知道哪个是真的,直接报错中止。

源码里走得也是这个顺序(`src/arch/aarch64/mm/mmu.rs::split_2mb_block`):

```rust
// Fill L3 with 512 page entries first
unsafe {
    let l3_ptr = l3 as *mut u64;
    for i in 0..512u64 {
        let pa = block_pa + i * PAGE_SIZE_4KB;
        let page = pa | block_attr_bits | PTE_TABLE | PTE_VALID;
        *l3_ptr.add(i as usize) = page;
    }
}

// Break-before-make: invalidate old L2 block entry first
Self::write_pte(Self::entry_ptr(l2_table, l2_idx), 0);
Self::tlbi_all();

// Then write new L2 table descriptor pointing to L3
let l2_desc = l3 | PTE_VALID | PTE_TABLE;
Self::write_pte(Self::entry_ptr(l2_table, l2_idx), l2_desc);
Self::tlbi_all();
```

`map_4kb_page` 后续往 L3 PTE 里写新页的时候,也走同一套 break-before-make:

```rust
let l3_ptr = Self::entry_ptr(l3_table, l3_idx);
let old_l3 = Self::read_pte(l3_ptr);
if old_l3 & PTE_VALID != 0 {
    Self::write_pte(l3_ptr, 0);   // break: 先写 invalid
    Self::tlbi_ipa(ipa);          // flush
}

let page_entry = self.make_page_entry(ipa & !PAGE_MASK_4KB, attr);
Self::write_pte(l3_ptr, page_entry);   // make: 再写新值
Self::tlbi_ipa(ipa);
```

两次 `tlbi_ipa`——一次在 break 之后,一次在 make 之后。第一次是合规要求,第二次是确保 guest 之后访问能看到新 PTE。

跳过第一次会怎样?多数时候没事——TLB miss 直接 walk 表,看到新值。但某些 microarchitecture 在两个 valid PTE 同时被缓存时会报 TLB Conflict Abort,在某些 CPU 模型上随机触发,很难复现。页表看上去换好了,机器却会在某次启动里停住。等查到现场,破坏现场早已经离开。D5.10.1 不是 nice-to-have,是必须做的。

---

## 那为什么 heap 还在 guest 的 RAM 物理范围里?

绕回开头的问题。既然要把 heap 藏起来,为什么不干脆放到 guest 的 RAM 范围之外?比如放到 `0x3000_0000`?

两个原因。

第一,我们走的是 identity mapping——guest 看到的 IPA 直接等于真实 PA。如果 hypervisor 自己的代码段、数据段、堆都在 `0x4000_0000` 起的低端 RAM,而 guest RAM 从 `0x4800_0000` 起,这一段的地址布局很自然:linker script 把 hypervisor 装在 `0x4020_0000`,堆紧接着放 `0x4100_0000`,然后留个 7 MB 空当到 guest kernel 装载点 `0x4800_0000`。物理上是连续的一大块 RAM,只是中间有个 deliberate 的"洞"在 Stage-2 上不映射。

第二,也是更技术性的——hypervisor 启动时还没建好 Stage-2 页表,自己代码访问堆走的是物理地址(此时 MMU off / Stage-1 identity)。Stage-2 是后来给 guest 用的。所以堆的物理地址必须是 hypervisor 自己物理上能访问的——QEMU `-m 2G` 给的就是 `0x4000_0000` 起的 2 GB。

把这两点合起来:**堆在低端 RAM(0x41000000),hypervisor 自己能用;Stage-2 上不映射,guest 不能用**。

---

## DynamicIdentityMapper 的局限

四张 L2 表,覆盖 4 GB,够 QEMU virt 玩。换到大内存的物理板子上要扩。

但更隐性的限制是**L3 表的回收**。`split_2mb_block` 一旦把 2 MB 块拆成 L3 表,即使后来那些 4 KB 页全部 unmap 了,L3 表本身不会回收回 heap——因为没办法判断"是不是所有 512 个 entry 都已经 invalid 了"而不付出每次 unmap 都遍历一次的代价。

在 hypervisor 这种"启动期建好就不动"的工作模式下,这个限制无所谓——`BumpAllocator` 配上 free list 处理得了,且 4 KB 的 L3 表数量本来就少(只有需要 4 KB 粒度的地方才拆)。但如果有人把这套 mapper 拿去做"频繁映射/解映射 4 KB 页"的工作(比如用户态 mmap 模拟),L3 表泄漏会成为问题。

下一篇我打算讲 GICv3 虚拟化,正好是 4 KB 拆块这一招的用户。GICR 的那 4 KB MMIO 区域不映射,guest 访问就陷下来,我们在 `VirtualGicr` 的 shadow 上响应,该透传的写透给物理 GIC,该挡的挡。整条路径下一篇见。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十一篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
