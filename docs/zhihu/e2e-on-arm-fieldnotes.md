# 在一台真 ARM 服务器上，用发行版自带的 QEMU 跑通了完整 NS→Secure 链路

## 写在前面

之前这个项目的安全世界链路（TF-A → 我们的 S-EL2 SPMC → Secure Partition），一直是在 x86 开发机上靠 QEMU TCG 纯模拟跑的。这次手上有了一台真正的 ARM64 Linux 服务器（aarch64，带 `/dev/kvm`，SVE2，NVIDIA 内核 6.11），我想验证一个朴素的问题：

**搬到真 ARM 机器上，是不是就能用 KVM 加速、甚至跑通 Android AVF 了？**

结论有点反直觉，也正是这篇实战记最想讲清楚的一件事。下面按当天的真实顺序记录，包括几个踩坑和绕过的办法。

---

## 第一个反直觉：真 ARM 机器 + /dev/kvm，也救不了安全世界

很多人（包括当时的我）会下意识觉得：x86 上只能模拟，换到 ARM 原生硬件，KVM 一上，全链路就能加速了。

但安全世界这条链是个例外。我们要跑的是：

```
EL3    TF-A BL31 + SPMD       ← 安全监控器
S-EL2  我们的 SPMC            ← 管理 Secure Partition
S-EL1  SP1 / SP2 / SP3        ← 秘密分区
NS-EL2 pKVM / 测试客户端
NS-EL1 Linux/Android
```

关键点：**QEMU 的 `secure=on`（EL3、安全世界、S-EL2）只能在 TCG 下模拟，KVM 根本无法虚拟化 EL3 或 Secure World**——ARM 上不存在"嵌套安全虚拟化"这种东西。所以无论宿主是 x86 还是 ARM，这条 NS→Secure 全链路都只能 TCG。

那 `/dev/kvm` 在这台机器上还有什么用？只对**纯普通世界**（NS-EL2 的 pKVM 加速、AVF 的 crosvm）有意义，而且还得满足额外条件——后面会说。

所以第一个认知修正就是：**安全世界用 TCG 跑，不是将就，是唯一正确的方式。而 TCG 完全够用。**

---

## 第二个坑：这台机器上压根没有 QEMU，还没有 sudo

想跑 `make run`，结果 `qemu-system-aarch64` 不在 PATH，全系统都搜不到。更尴尬的是：

- 当前用户没有免密 `sudo`（非交互装不了包）
- 不在 `docker` 组（仓库里所有 TF-A/QEMU 构建都靠 Docker）
- 不在 `kvm` 组（`/dev/kvm` 直接 Permission denied）

我先试了一条**完全免 root** 的路子：用 micromamba（单文件静态二进制）从 conda-forge 装 QEMU。下载、解压都顺利，结果在 `linux-aarch64` 频道翻车了——

```
qemu =* * does not exist
```

conda-forge 的 ARM64 频道**只有 `qemu.qmp` 这个 Python 库，没有 QEMU 模拟器本体**。x86 频道有，arm64 没有。这个坑值得记一下：别想当然以为 conda 能在 arm64 上给你 qemu-system。

从源码编译？又缺 `glib` / `pixman` / `meson` / `ninja`，系统里一个都没装。

最后还是最朴素的方案最快：让有 sudo 的人跑一条

```bash
sudo apt install -y qemu-system-arm   # 这个包会附带 qemu-system-aarch64
```

装完得到 QEMU **8.2.2**。记住这个版本号，后面有个惊喜。

---

## 先跑普通世界：make run，34/34

拿到 QEMU 后，先跑最基础的单元测试套件（NS-EL2 + TCG，不碰安全世界）：

```bash
qemu-system-aarch64 -machine virt,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic -kernel target/.../hypervisor
```

干净启动到 EL2，GICv3 初始化、16MB 堆、timer 全部就绪，**34 个测试套件全部运行、零失败**。

一个小提示：最后一个套件 `guest_interrupt` 会进入 guest 执行后**永不返回**（设计如此），所以记得用 `timeout --foreground 90 qemu...` 包一层，否则会一直挂着。同理日志里那句 `[INIT] DTB: parse failed, using defaults` 也是预期的——`-kernel` 模式下 QEMU 传的 DTB 地址是 0，代码回退到 virt 默认值。

---

## 第三个坑：加了 docker 组，但当前会话看不到

要跑安全世界就得用 Docker。`sudo usermod -aG docker $USER` 跑完，`/etc/group` 里也确实有了：

```
docker:x:988:wilamhou
```

但执行 `id` 时，当前 shell 的 live group **还是没有 docker**。原因是：组成员变更只对**新登录的进程**生效，而我的执行 shell 的父进程是在改组之前启动的，重新登录 UI 并不会重启这个底层进程。

不想重启整个会话，有个干净的办法——`sg`（switch group），它在运行时读 `/etc/group`：

```bash
sg docker -c 'docker ps'        # 立即可用，无需重新登录
sg docker -c 'make build-tfa-spmc'
```

`sg docker -c '命令'` 会以 docker 组身份执行整条命令，连带它 fork 出来的 `docker run` 子进程也都有组权限。这招在"刚加完组又不想注销"的场景下很实用。

---

## 构建 + 跑通安全世界全链路

一条命令搞定 TF-A + SPMC + 三个 SP 的构建（Docker 内编译，首次约 10-20 分钟）：

```bash
sg docker -c 'make build-tfa-spmc'
```

顺便提一个容易爆的雷：S-EL2 下 `CPTR_EL3.TFP` 会陷阱掉所有 FP/SIMD，而 Rust 的 debug 构建会给 `read_volatile` 的对齐检查发 NEON 指令（`cnt v0.8b`），一旦执行就**静默挂死**。好在仓库的 `[profile.dev]` 已经把 `opt-level` 设成 1，sel2 默认构建就不发 NEON，这个坑提前被堵上了。

产物齐了（`flash-spmc.bin` 64MB、`hypervisor_spmc.bin`、SP1/2/3），跑：

```bash
qemu-system-aarch64 -machine virt,secure=on,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic -bios tfa/flash-spmc.bin -nic none
```

输出一气呵成，整条四级特权链全部起来了：

```
NOTICE:  BL31: v2.12.0(debug)          ← EL3 TF-A + SPMD
[SPMC] Running at EL2                   ← S-EL2 我们的 SPMC
[SPMC] spmc_id=0x8000 version=1.1
[SPMC] S-EL2 Stage-1 MMU enabled (NS DRAM mapped)
[SP] Hello from S-EL1!                  ← SP1
[SP2] Hello from S-EL1!                 ← SP2
[SP3] Hello from S-EL1 (sp_relay)       ← SP3
...
  Test 1: FFA_VERSION .............. PASS
  ...
  Test 20: SP-to-SP MEM_RECLAIM .... PASS
  All tests complete.
```

**BL33 测试 20/20 全过**——FF-A 发现、DIRECT_REQ（含多 SP）、抢占 + FFA_RUN、安全 vIRQ 注入、MEM_SHARE/LEND 完整生命周期、SP↔SP 转发 + 循环检测 + 内存共享/回收，全链路打通。

---

## 最大的惊喜：发行版自带的 QEMU 8.2.2 就够了

仓库 Makefile 里有一句注释：

> Local QEMU 9.2+ for S-EL2 targets (secure=on requires newer QEMU)

意思是安全世界目标需要自己从源码编译 QEMU 9.2.3（一个 20-40 分钟的大活）。但这次实测下来——**Ubuntu 自带的 QEMU 8.2.2 跑 S-EL2 完全没问题，那条"需要 9.2+"的注释过于保守了。** 直接省掉了编译自定义 QEMU 这一步。

（机制上，`QEMU_SEL2` 变量是"有 `tools/qemu-system-aarch64` 就用它，否则回退系统 qemu"。我们不构建自定义版本，它就自动用系统的 8.2.2，正好。）

---

## 那 Android AVF 到底能不能用 QEMU 跑通？

这是当天另一个核心问题。答案要拆成两层：

| 层 | 是什么 | QEMU 能跑吗 |
|---|---|---|
| **pKVM**（EL2 hypervisor） | `kvm-arm.mode=protected` 的保护型 KVM | ✅ 能。`make run-pkvm` 已能在 TCG 下把 AOSP 内核以 pKVM 模式启动到 BusyBox，FF-A 正常 |
| **crosvm / pVM**（EL0 建受保护 VM） | VMM 通过 `/dev/kvm` 创建 pVM | ❌ 当前不能，卡在 `failed to create IRQ chip` |

为什么 crosvm 这层卡住：crosvm 要在 guest **内部**再调 `/dev/kvm` 建 pVM，这需要 guest 内核的 KVM 真正可用；而 QEMU TCG 无法创建 `KVM_DEV_TYPE_ARM_VGIC_V3`（vGICv3）这个 KVM 设备。要让它工作，要么 QEMU 用 `-accel kvm` + **嵌套虚拟化**（给 guest 一个真 EL2），但本机 `kvm.nested` 没开；要么**原生在宿主跑 crosvm**，但 `/dev/kvm` 权限被挡。这跟我们的 hypervisor 无关，是 Android pKVM+crosvm 自身在纯 TCG 下的边界。

所以一句话总结：**Android 的 pKVM hypervisor 层，QEMU 里已经跑通；完整 AVF 的 pVM 创建层，QEMU TCG 下跑不通**，得开嵌套虚拟化或原生 KVM。

---

## 几条可复用的经验

1. **安全世界（EL3/S-EL2）永远是 TCG**，KVM 帮不上忙，但 TCG 完全够用——别为此纠结要不要上 KVM。
2. **conda-forge 的 arm64 频道没有 qemu-system 本体**，别在这上面浪费时间。
3. **`sg docker -c '...'`** 能在刚加完 docker 组、不想注销的情况下立刻用上组权限。
4. **发行版自带 QEMU（8.2.2）足以跑 S-EL2 `secure=on`**，不必非编译 9.2.3。
5. **S-EL2 的 Rust 构建必须 `opt-level >= 1`**，否则 debug 模式的 NEON 对齐检查会静默挂死。
6. 跑 `run` / `run-spmc` 记得用 `timeout` 包一层，因为最后会停在空转/阻塞状态。

真 ARM 硬件给项目带来的真正增量，不在安全世界（那本来就该 TCG），而在于**未来能不能解锁 AVF 的 crosvm 那层**——前提是把嵌套虚拟化或原生 `/dev/kvm` 这两道门之一打开。那是下一篇的事了。
