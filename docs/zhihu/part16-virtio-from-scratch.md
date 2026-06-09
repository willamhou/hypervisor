# virtio-blk 和 virtio-net 从描述符那一头讲起

Guest 的 Linux 内核往 virtio-blk 的 MMIO 寄存器 `QueueNotify` 写了一个 0。它告诉 hypervisor:"我刚在 virtqueue 0 里放了一个新请求,你处理一下。"

hypervisor 的异常处理 trap 这个 MMIO 写,进入 `VirtioMmioTransport<VirtioBlk>` 的 dispatch。它要做的事:从 guest 内存里读 virtqueue 的 available ring 头部,拿到这个请求对应的描述符链(descriptor chain),按 virtio-blk 的协议解析出请求类型、起始扇区、数据缓冲区、状态写回缓冲区,真正去硬盘镜像文件读/写,然后把结果写回 used ring,inject SPI 48 通知 guest。

这一篇拆开这条路径上的几个具体细节:**MMIO 寄存器布局、virtqueue 的三环结构、descriptor 链怎么走、virtio-blk 请求的三段式格式、virtio-net 那 12 字节 `virtio_net_hdr_v1` 前缀容易踩**。代码在 `src/devices/virtio/{mmio,queue,blk,net}.rs` 这一组。

---

## virtio-mmio:寄存器层

virtio 有 PCI、CCW、MMIO 三种 transport。我们用 MMIO,因为最简单——一段连续 MMIO 地址,寄存器按 offset 排好,guest 直接 load/store 访问。MMIO transport 的寄存器布局在 virtio v1.0 spec 4.2.2:

```
0x000  MagicValue       (RO) = "virt"
0x004  Version          (RO) = 2
0x008  DeviceID         (RO) = 1 (net) / 2 (blk) / ...
0x00C  VendorID         (RO)
0x010  DeviceFeatures   (RO)
...
0x030  QueueSel         (WO) 选择当前操作哪个 virtqueue
0x038  QueueNumMax      (RO)
0x03C  QueueNum         (WO)
0x044  QueueReady       (RW)
0x050  QueueNotify      (WO) ← guest 往这写,通知有新请求
...
0x070  Status           (RW)
0x080  QueueDescLow/High    Descriptor table 物理地址(2x32位 = 64位)
0x090  QueueAvailLow/High   Available ring 地址
0x0A0  QueueUsedLow/High    Used ring 地址
```

hypervisor 这边在 `VirtioMmioTransport::mmio_read/write` 实现这些寄存器的访问。大多数寄存器是返回常量(MagicValue / Version / DeviceID)或者记录 guest 选择(QueueSel / QueueNum / QueueDescLow/High 之类)。**真正引发"开始处理请求"动作的主要是两个**——`Status` 写到 `DRIVER_OK`(guest 完成了初始化握手,后端可以开工)和 `QueueNotify` 写(guest 提交了新请求,后端开始处理)。其他写法主要是更新配置,把映射地址、队列大小记下来留给后续 Notify 用。

---

## Virtqueue 是三个 ring

每个 virtqueue 由三块共享内存组成,**都在 guest 物理内存里**,hypervisor 通过 IPA 访问。三块的角色:

```
Descriptor table:  16B × N entries — 描述单个 buffer 的 (addr, len, flags, next)
Available ring:    guest → hypervisor 提交 (descriptor index 数组)
Used ring:         hypervisor → guest 返回 (idx, len)
```

**Descriptor 链**:一个请求可以由多个 descriptor 串成链,通过 `next` 字段。flags 里的 `VIRTQ_DESC_F_NEXT` 标志这一项还有下一段。这种"分段描述符"是 virtio 的 scatter-gather 语义——guest 可以把请求拆成"控制信息块"+"数据块"+"状态写回块",每块在 guest 内存里物理上不连续,但通过链串起来。

**Available ring**:guest 把要提交的描述符链头部 index 写进 ring,递增 idx。hypervisor 轮询 ring 找新项目处理。简化结构:

```rust
struct VirtqAvail {
    flags: u16,
    idx: u16,           // 写到哪了
    ring: [u16; QUEUE_SIZE],
    // used_event: u16  (可选 feature)
}
```

**Used ring**:hypervisor 处理完一个请求,把 (descriptor head index, written length) 写进 used ring,递增 used.idx。Guest 看到 used.idx 增长,知道有完成的请求。

hypervisor 处理一次 `QueueNotify` 的流程:

```
1. last_avail_idx 跟 avail.idx 比 — 有几个新提交
2. 循环每个新项目:
   a. head = avail.ring[last_avail_idx % QUEUE_SIZE]
   b. 走描述符链:descs[head], descs[head].next, ...
   c. 把整条链丢给 backend (process_request)
   d. backend 处理完,返回写回 used: written
   e. used.ring[used.idx % QUEUE_SIZE] = (head, written)
   f. used.idx += 1, last_avail_idx += 1
3. inject SPI 通知 guest
```

整条循环里 hypervisor 不分配内存——所有 buffer 都在 guest 内存里,我们只 read/write,不 copy。性能这一段决定了 virtio 的基本上限。

---

## virtio-blk 请求:三段式

virtio-blk 的请求按 spec 5.2.6 是三段:**header (16B)**、**data (变长)**、**status (1B)**:

```
header (RO from device):
  type:   u32 (0=in, 1=out, 4=flush, ...)
  reserved: u32
  sector: u64

data (RO from device for write, WO for read):
  size 由 descriptor.len 决定

status (WO from device):
  0=ok, 1=ioerr, 2=unsupported
```

Guest 用三个 descriptor 串成一条链:头部、数据缓冲、状态缓冲。flags 区分:

- 头部:flags = NEXT(链向下一个)
- 数据:flags = NEXT | WRITE(WRITE 表示 device 写入这个 buffer,即 guest 读)
- 状态:flags = WRITE(无 NEXT,最后一段)

`process_request` 解析这三段:

```rust
// src/devices/virtio/blk.rs (简化)
fn process_request(&mut self, queue: &mut Virtqueue, head: u16,
                   descs: &[VirtqDesc], count: usize) {
    if count < 2 { return; }  // 至少头 + 状态

    // 第 0 段是 header
    let header_addr = descs[0].addr;
    let req_type = read_u32(header_addr);
    let sector = read_u64(header_addr + 8);

    // 中间是 data,最后是 status
    let status_idx = count - 1;
    let data_descs = &descs[1..status_idx];

    let result = match req_type {
        0 => self.do_read(sector, data_descs),
        1 => self.do_write(sector, data_descs),
        _ => Err(VIRTIO_BLK_S_UNSUPP),
    };

    let status_byte = match result { Ok(_) => 0, Err(e) => e };
    write_u8(descs[status_idx].addr, status_byte);
}
```

读写真实数据时,`data_descs` 可能多段——guest 的物理内存如果不连续(典型情况),会拆成多个 4 KB descriptor。`do_read` 走整个 data_descs 顺序写,`do_write` 顺序读。

`addr` 是 guest physical address (IPA)。我们这边 Stage-2 是 identity mapping([Part 11](./part11-stage2-heap-gap.md) 讲过),所以 `addr` 直接当物理地址用,`unsafe { read_volatile / copy_nonoverlapping }` 就行。

---

## virtio-net 的 12 字节前缀

virtio-net 跟 virtio-blk 的结构类似——两个 virtqueue(RX queue 0,TX queue 1),descriptor 链,scatter-gather。但有个特别的细节:**每个数据包前面有一个 header 前缀**——协商了 `VIRTIO_NET_F_MRG_RXBUF` feature(我们这边默认协商)就是 `virtio_net_hdr_v1`,**12 字节**;没协商就是更老的 `virtio_net_hdr`,**10 字节**(差那 2 字节就是 `num_buffers` 字段)。我们这边一律按 12 字节走。

```
virtio_net_hdr_v1 (12 B):
  flags:        u8
  gso_type:     u8
  hdr_len:      u16
  gso_size:     u16
  csum_start:   u16
  csum_offset:  u16
  num_buffers:  u16   ← VIRTIO_NET_F_MRG_RXBUF 协商后这字段存在
```

guest 发包时(TX queue),descriptor 链第一段是这 12 字节 header,后面是真正的 Ethernet 帧。hypervisor 处理 TX:

```rust
// src/devices/virtio/net.rs (简化)
fn process_tx(&mut self, descs: &[VirtqDesc]) {
    // 跳过前 12 字节,取真正的 frame
    let header_addr = descs[0].addr;
    let frame_start = header_addr + 12;
    let frame_len = descs[0].len - 12;

    // 直接交给 vswitch
    let frame = read_buf(frame_start, frame_len);
    crate::vswitch::vswitch_forward(self.port_id, &frame);
}
```

12 字节里我们一字都不看——guest 发包时设这些字段是给"硬件 offload"用的(校验和卸载、TSO、GSO),我们的 vSwitch 是纯软件,什么都不卸载,直接转发。

RX 反过来——把帧塞回 guest 时**必须**先写 12 字节 header,再写帧:

```rust
fn inject_rx(&mut self, frame: &[u8], descs: &[VirtqDesc]) {
    let head_addr = descs[0].addr;

    // 写 12 字节 header,全部 0 除了 num_buffers=1
    unsafe {
        core::ptr::write_bytes(head_addr as *mut u8, 0, 12);
        core::ptr::write_unaligned((head_addr + 10) as *mut u16, 1);
    }

    // 写帧
    let frame_dst = head_addr + 12;
    unsafe {
        core::ptr::copy_nonoverlapping(
            frame.as_ptr(),
            frame_dst as *mut u8,
            frame.len(),
        );
    }
}
```

第一次写 virtio-net 时我没注意到这 12 字节前缀的事——直接把帧从 vSwitch 投递到 guest 的 RX queue,guest 的网络栈跑出来全是错的(它以为前 12 字节是 IP header,后面是 IP payload,但偏移整体错了 12 字节)。tcpdump 在 guest 里抓出来的包看起来"对不上结构",查了半天才意识到 virtio 协议要求这个 header。

---

## RX 路径的异步问题

virtio-net 的 TX 是同步的——guest 写 QueueNotify,trap 进 hypervisor,hypervisor 立刻处理,return,一气呵成。

RX 不能这样。RX 帧的"发起点"是另一台 VM 通过 vSwitch forward 过来的(见 [Part 8](./part8-multi-vm-vswitch.md))——发生在 hypervisor 的某次异常上下文里(那一台 VM 的 virtio-net TX 处理过程中)。这时候我们想把帧塞给本 VM,但**本 VM 此刻可能并没在跑**——它在另一台 pCPU 上,或者根本没被调度到。

vSwitch forward 不能直接调本 VM 的 `inject_rx`——会撞 `DEVICES` 锁(我们已经在持有 source VM 的 `DEVICES` 锁)。所以 RX 走一个 per-port SPSC ring buffer:

```rust
// src/vswitch.rs
pub static PORT_RX: [NetRxRing; MAX_PORTS] = [...];

// vSwitch forward 路径 (在 EL2 异常上下文里):
PORT_RX[dst_port].store(frame);

// VM 调度循环里 (主循环上下文):
fn drain_net_rx(vm_id: usize) {
    while let Some(frame) = PORT_RX[vm_id].take() {
        DEVICES[vm_id].inject_net_rx(&frame);
    }
}
```

SPSC ring 让 producer(vSwitch forward, 异常上下文)和 consumer(drain, 主循环)解耦——producer 不需要等 consumer,consumer 也不需要等 producer。两边用原子 head/tail 索引,acquire/release ordering 保证内存可见性。

这种"trap 时只入队,真正处理放到主循环"的 pattern,在裸机里反复出现。**异常上下文要尽量短**——它持有的锁可能在主循环里也要拿。两者解耦的代价是一个 ring buffer,收益是 hypervisor 整体的可调度性。

---

## 收尾

virtio 协议层不长——MMIO 寄存器 30 个、virtqueue 三个 ring、descriptor 16 字节、协议 header 几个字段。难的不是协议,是**协议跟 hypervisor 自己执行模型的耦合**:RX 不能同步、descriptor 不能跨页假设、IPA 翻译要看 Stage-2 配置。这些约束让 virtio 后端代码比看起来多 30%。

代码量上,`virtio/mmio.rs` + `virtqueue.rs` + `blk.rs` + `net.rs` 加起来约 1100 行,其中 mmio.rs 占一半(寄存器分发 + descriptor chain walk)。剩下的是设备特定逻辑。

下一篇我想讲 Secondary CPU 在 S-EL2 的完整 warm-boot 流程——[Part 4](./part4-war-stories.md) 是"发现 SPMD 状态机",这一篇是"装一颗 secondary 完整要走哪几步、为什么顺序敏感"。

---

代码:<https://github.com/willamhou/hypervisor>

博客:<https://willamhou.github.io/hypervisor/>

*这是 ARM64 Hypervisor 开发系列的第十六篇。之前的文章索引在 [Part 0a](./part0a-why.md) 的末尾。*
