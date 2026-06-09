# TF-A 启动链上几个"规范没写但必须知道"的坑

QEMU 起来,TF-A BL1 → BL2 → BL31 都过了,日志里看到 BL31 准备转交给 BL32(我的 SPMC,挂在 S-EL2 那一层),然后**SPMC 入口直接挂死**——串口没新输出,没有 panic,没有 exception 日志,什么都没有。

我跑 `fiptool info build/qemu/debug/fip.bin` 想看 FIP 内容。每一项都对得上,SP UUID 列出来,size 不为零。再跑一次 `objdump -d` 看 SPMC 入口附近,反汇编出来是一串 `0x4750_4B53`、`0x0000_0001`、`0x0000_1000`——根本不是 ARM64 指令,而是 ASCII "SPKG" + 一段头部数据。

这就是症状。BL31 把我加载到 `0x0e100000`,我以为我的代码就在那个地址,实际上代码在那个地址**加 0x4000** 之后。中间 16 KB 是 SPKG 头加 manifest。

这一篇记四个让我反复栽跟头的具体细节——**FIP 里 SP 镜像怎么找、SPKG 头那 0x4000 偏移、UUID 字节序换没换、CTX_INCLUDE_FPREGS 跟谁互斥**。文档把"BL1 → BL2 → BL31 → BL32 → BL33"这条链画得清清楚楚,但两个 BL 之间约定的细节都藏在源码里。

---

## FIP 是个扁平容器,里面找东西靠 UUID

TF-A 把所有 BL 镜像打成一个文件叫 FIP(Firmware Image Package)。BL1 / BL2 / BL31 / BL32 / BL33 / 各种 config DTB / SP 镜像,全部塞一个 `flash.bin` 里。物理介质上看就是 4 MB 一段、4 MB 一段的二进制连续排着。

要让 BL1 / BL2 知道每个东西在哪,FIP 头部有一个 `fip_toc`(目录)。每一项记录 `uuid + offset + size`,顺着 UUID 找对应内容。所以 FIP 不是按"名字"找,是**按 UUID 找**。

`fiptool info fip.bin` 把目录列出来给你看,大概长这样:

```
Trusted Boot Firmware BL2: offset=0xB0, size=0xBDD0, ...
EL3 Runtime Firmware BL31: offset=0xBE80, size=0xFF5D, ...
Secure Payload BL32: offset=0x1BDDD, size=0x475D9, ...
Non-Trusted Firmware BL33: offset=0x633B6, size=0x1064, ...
TB_FW_CONFIG: offset=0x6441A, size=0x1C7, ...
TOS_FW_CONFIG: offset=0x645E1, size=0x189, ...
78563412-7856-3412-7856-341278563412: offset=0x6476A, size=0x5111, ...   # SP1
DDCCBBAA-DDCC-BBAA-DDCC-BBAADDCCBBAA: offset=0x6987B, size=0x5088, ...   # SP2
00112233-0011-2233-0011-223300112233: offset=0x6E903, size=0x41EC, ...   # SP3
```

前几条是 TF-A 已知的"标准"组件,有人类可读的名字。SP 镜像不在 TF-A 的"标准"清单里,FIP 直接把 UUID 当 ID。BL2 启动 SP 的时候要靠这串 UUID 找。

---

## UUID 字节序换了一次

这就是第一个坑。`sp_manifest.dts` 里写 UUID 是这样:

```
sp_hello {
    uuid = <0x12345678 0x12345678 0x12345678 0x12345678>;
};
```

四个 32 位字。直觉上以为 `fiptool` 看到的应该是 `12345678-1234-5678-1234-567812345678`。但你前面那个表看到的是 `78563412-7856-3412-...`——每个 32 位字都被**字节反序**了。

为什么?TF-A 的 SP 打包工具 `sp_mk_generator.py` 在生成 SPKG 头时,把 manifest 里的 UUID 按 little-endian 序列化。`0x12345678` 在 LE 序列化下是 `78 56 34 12`,人类读出来就是 `78563412`。

工具这么做有它的理由(C 结构体里 UUID 是 16 字节数组,按 LE 写直接 memcpy 就对),但**这一步在 sp_manifest.dts 文档里没写**。你照着 manifest 写 UUID,然后想验证 fip 里有没有,直接 grep 原始 UUID 是 grep 不到的——它在 fip 里是字节反序之后的形态。

紧接着一个推论:**`tb_fw_config.dts` 里写的 UUID 必须用反序之后的形态**:

```
sp1 {
    uuid = "78563412-7856-3412-7856-341278563412";   # 反序后的!
    load-address = <0x0e300000>;
};
```

BL2 启动时按 `tb_fw_config.dts` 里的 UUID 去 FIP 找 SP——这边 UUID 跟 FIP 里的 UUID 必须**字节级**对得上。我第一次写的时候把 `12345678` 直接抄进 `tb_fw_config`,BL2 找不到 SP1,日志显示 "SP UUID not found",这种错误信息让你完全猜不到症结在字节序。

`sp_manifest.dts` 的 UUID 是写**原始**形态、`tb_fw_config.dts` 是写**反序**形态。这件事我每次写新 SP 都要去翻一次 commit 提醒自己。

---

## SPKG 头那 0x4000 字节偏移

第二个坑。BL2 把 SP 镜像从 FIP 加载到 `tb_fw_config.dts` 里写的 `load-address`(比如 `0x0e300000`)之后,SPMC 拿到的不是"SP 入口在 `0x0e300000`",而是"SP 入口在 `0x0e300000 + 0x4000`"。

那 0x4000 字节是 **SPKG 头**(SP Package header),24 字节 LE 结构 + 一堆补齐:

```
Offset 0:  magic[4]    = "SPKG"
Offset 4:  version      (u32 LE)
Offset 8:  pm_offset    = 0x1000   (manifest 在包内的 offset)
Offset 12: pm_size      (manifest 大小)
Offset 16: img_offset   = 0x4000   (镜像在包内的 offset)
Offset 20: img_size     (镜像大小)
```

`sp_mk_generator.py` 生成的 SPKG 把 manifest 放在 `[0x1000, 0x1000 + pm_size)`、镜像放在 `[0x4000, 0x4000 + img_size)`。中间 padding 是为对齐。

所以 SPMC 启动 SP 的时候要做:

```rust
// src/main.rs:
const SPKG_IMG_OFFSET: u64 = 0x4000;
let sp_entry = sp_load_addr + SPKG_IMG_OFFSET;
```

24 字节头看起来很简单,但**TF-A 文档里不提**——它假设你直接用 sp_mk_generator 生成包,不会自己手动设入口地址。等你像我这样自己写 SPMC,把 SP 加载到一个地址然后 ERET 过去,你得**自己**知道镜像不在那个地址的开头,而在偏移 0x4000 处。

第一次没注意时,直接 ERET 到 `0x0e300000`,CPU 取指到 SPKG 头的 "SPKG" 四个字节——`0x4750_4B53`,在 ARM64 里随便解码成一条无效指令,Undefined Instruction exception,挂死。日志里你只看到 EC=0x00 (unknown),根本看不出"哦我跳到 header 上了"。

---

## CTX_INCLUDE_FPREGS 不能跟 ENABLE_SVE/SME_FOR_NS 一起

第三个坑,这个是我跨了一整天的。

[Part 7](./part7-bare-metal-rust-pitfalls.md) 讲过 `CPTR_EL3.TFP=1` 会把 S-EL2 的 FP/SIMD 指令 trap 到 EL3,Rust debug 模式的 NEON 指令会让 SPMC 静默挂死。修法是 TF-A 编译时加 `CTX_INCLUDE_FPREGS=1`,这个 flag 让 TF-A 保存/恢复 FP 寄存器,顺带清掉 `CPTR_EL3.TFP`。

我加进去之后 TF-A 构建直接报错:

```
ENABLE_SVE_FOR_NS is mutually exclusive with CTX_INCLUDE_FPREGS
```

TF-A 默认 `ENABLE_SVE_FOR_NS=1`(允许 Normal world 用 SVE)。SVE 寄存器跟 FP 寄存器在硬件层重叠,TF-A 内部两种保存路径互斥。要么 TF-A 自己管 SVE(`ENABLE_SVE_FOR_NS=1` + 不开 CTX_INCLUDE_FPREGS,SVE 保存路径同时处理 FP),要么 hypervisor 自己管 FP(`ENABLE_SVE_FOR_NS=0` + `CTX_INCLUDE_FPREGS=1`)。

SME 同理。完整组合是:

```makefile
CTX_INCLUDE_FPREGS=1
ENABLE_SVE_FOR_NS=0
ENABLE_SME_FOR_NS=0
```

三个一起设才编得过。错过任何一个 TF-A 都会在构建阶段卡。文档**有**写,但散在 `docs/getting_started/build-options.rst` 三个不同地方,不连续读容易漏。我第一次开了 `CTX_INCLUDE_FPREGS` 没 disable SVE,构建报错;关 SVE 没关 SME,构建又报错;来回试了三轮才把三个 flag 凑齐。

写下来:**S-EL2 跑 Rust(或者任何会 emit NEON 的语言)的 hypervisor,这套三个 flag 是固定组合,记下来一次别再忘**。

---

## fiptool info 是你最好的朋友

调 TF-A 时,任何关于"BL2 找不到 X / 启动到 Y 就挂"的怀疑,**先跑一次 `fiptool info fip.bin`**。它把 FIP 里所有组件的位置、大小、UUID 列出来。

```bash
$ tools/fiptool/fiptool info build/qemu/debug/fip.bin
Trusted Boot Firmware BL2: offset=0xB0, size=0xBDD0, cmdline="--tb-fw"
EL3 Runtime Firmware BL31: offset=0xBE80, size=0xFF5D, cmdline="--soc-fw"
Secure Payload BL32: offset=0x1BDDD, size=0x475D9, cmdline="--tos-fw"
Non-Trusted Firmware BL33: offset=0x633B6, size=0x1064, cmdline="--nt-fw"
TB_FW_CONFIG: offset=0x6441A, size=0x1C7, cmdline="--tb-fw-config"
TOS_FW_CONFIG: offset=0x645E1, size=0x189, cmdline="--tos-fw-config"
78563412-7856-3412-7856-341278563412: offset=0x6476A, size=0x5111, cmdline="--blob"
DDCCBBAA-DDCC-BBAA-DDCC-BBAADDCCBBAA: offset=0x6987B, size=0x5088, cmdline="--blob"
00112233-0011-2233-0011-223300112233: offset=0x6E903, size=0x41EC, cmdline="--blob"
```

这一段输出能验证:

- FIP 包了所有该有的组件(BL2/BL31/BL32/BL33/configs/SPs)
- SP UUID 形态(看是反序还是原始,跟 `tb_fw_config.dts` 对比)
- 每个组件的实际 size(SP 镜像 size > 0x4000 + 你预期的 img_size,因为含 SPKG 头 + manifest + padding)
- 没有意外的 size=0 项(说明某个 build step 没产出对应文件)

`fiptool info` 是 FIP 的 X 光片。出问题的时候它 99% 能告诉你"哪一块不对"。

---

## 收尾

TF-A 这套启动链上的隐性约定,大半藏在源码、build 系统、Python 工具脚本里。文档把概念讲清楚,**约定**让你自己挖。

我自己的经验:**碰到症状不明的 boot 阶段错误,先做这三件事**——`fiptool info` 看 FIP 内容、grep 二进制看 SPKG 头 magic 在不在 `+0x4000`、对比 `tb_fw_config.dts` 跟 FIP 里的 UUID 字节序。三件事不到五分钟,80% 的 boot 阶段错误都能定位。

下一篇我想讲 FF-A v1.1 的协议层细节——RXTX mailbox 怎么注册、composite memory region descriptor 是怎样的嵌套结构、fragmentation 怎么把超长描述符切成多次 SMC 传过来。FF-A 这一层,代码本身比规范文档更像规范。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十四篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
