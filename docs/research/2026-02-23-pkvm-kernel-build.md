# pKVM Kernel Build Deep Dive

**Date**: 2026-02-23
**Purpose**: How to build, configure, and boot a pKVM-enabled Linux kernel for Phase 4.5
**Prerequisite**: [Phase 4.5 Feasibility Research](2026-02-22-phase45-pkvm-feasibility.md)

---

## 1. kvm-arm.mode 选项全景

`kvm-arm.mode` 是 Linux 内核启动参数，控制 KVM/arm64 的运行模式：

| 值 | Host 层级 | Hyp 层级 | 说明 |
|----|----------|---------|------|
| *(default)* | EL2 (VHE) | EL2 | CPU 支持 VHE 则自动使用，`HCR_EL2.E2H=1` |
| `nvhe` | EL1 | EL2 | 强制 nVHE，Host 在 EL1，hyp stub 在 EL2 |
| `protected` | EL1 | EL2 (pKVM) | nVHE + **Host Stage-2 隔离** — 即 pKVM |
| `nested` | EL2 | EL2 | **实验性** — Guest 可跑自己的 hypervisor (ARMv8.4 NV2) |
| `none` | EL1 | — | 完全禁用 KVM |

### VHE Override 机制

`kvm-arm.mode=protected` 在内部等价于 `id_aa64mmfr1.vh=0`：

1. 早期启动时内核解析 cmdline
2. 在 cpufeature override 表中 mask 掉 `ID_AA64MMFR1_EL1.VH` 字段
3. `cpus_have_const_cap(ARM64_HAS_VIRT_HOST_EXTN)` 返回 false
4. KVM 选择 nVHE 代码路径，Host 降到 EL1

代码位置：`arch/arm64/kernel/idreg-override.c` + `arch/arm64/kvm/arm.c`

### hVHE（混合 VHE，2024+）

在较新内核中，`kvm-arm.mode=protected` 映射为 `arm64_sw.hvhe=1`：

- nVHE 分离架构（Host 在 EL1），但 EL2 hypervisor 使用 VHE 的 `EL2+0` 翻译机制
- 好处：支持 VH 硬连线为 1 的 CPU，为 hypervisor userspace 隔离铺路
- 强制纯 nVHE（无 hVHE）：`arm64_sw.hvhe=0 id_aa64mmfr1.vh=0`

---

## 2. pKVM 初始化代码路径

```
BL33 enters kernel at NS-EL2
  → head.S: records __boot_cpu_mode = EL2
  → kvm-arm.mode=protected: masks VH → forces nVHE

kvm_arm_init()  [arch/arm64/kvm/arm.c]        ← subsys_initcall
  → init KVM infrastructure
  → install nVHE hyp code from vmlinux → EL2
  → __kvm_hyp_init: identity-mapped EL2 init

finalize_pkvm()  [arch/arm64/kvm/pkvm.c]       ← device_initcall
  → is_pkvm_initialized() check
  → pkvm_drop_host_privileges():
      - __pkvm_init(): EL2 Stage-1 (hyp address space)
      - Host Stage-2 in VTTBR_EL2
      - Host 降权到 EL1 — 单向不可逆
      - EL2 hyp 对象从 EL1 unmap
      - Cache flush
  → 之后 Host 无法替换 EL2 代码
  → PSCI CPU_ON/SUSPEND 被 pKVM 拦截
  → 新 CPU 在 EL1 启动（pKVM 拦截 PSCI）
```

### 关键源码文件

| 文件 | 作用 |
|------|------|
| `arch/arm64/kvm/arm.c` | KVM 主初始化 `kvm_arm_init()` |
| `arch/arm64/kvm/pkvm.c` | pKVM host 侧: `finalize_pkvm()`, `pkvm_drop_host_privileges()` |
| `arch/arm64/kvm/hyp/nvhe/setup.c` | EL2 侧: `__pkvm_init()`, `__pkvm_init_finalise()` |
| `arch/arm64/kvm/hyp/nvhe/hyp-init.S` | EL2 identity-mapped init |
| `arch/arm64/kvm/hyp/nvhe/pkvm.c` | EL2 侧: VM/vCPU 管理、内存共享、页面所有权 |
| `arch/arm64/kvm/hyp/nvhe/ffa.c` | EL2 FF-A 代理（拦截 Host SMC） |
| `arch/arm64/kvm/sys_regs.c` | ID 寄存器过滤 |

---

## 3. 内核配置 (CONFIG_ 选项)

### 核心必需

```
CONFIG_KVM=y                    # KVM 主开关（arm64 defconfig 默认开启）
CONFIG_VIRTUALIZATION=y         # 虚拟化子系统（CONFIG_KVM 自动选择）
```

pKVM **没有单独的 CONFIG 开关** — 它内建在 nVHE 代码路径中，通过 `kvm-arm.mode=protected` 启动参数激活。

### 调试选项（开发时推荐）

```
CONFIG_NVHE_EL2_DEBUG=y         # 放宽 Host Stage-2 以允许 stacktrace 符号化
CONFIG_PROTECTED_NVHE_STACKTRACE=y  # hyp_panic() 时的 pKVM stacktrace
                                    # (新内核改名为 CONFIG_PKVM_STACKTRACE)
CONFIG_PKVM_DISABLE_STAGE2_ON_PANIC=y  # panic 时放宽 Stage-2 以 unwind
```

### 我们项目还需要的

```
CONFIG_ARM_FFA_TRANSPORT=y      # FF-A 驱动（已在现有 build-kernel.sh 中）
CONFIG_VIRTIO_MMIO=y            # Virtio MMIO（已有）
CONFIG_VIRTIO_BLK=y             # Virtio 块设备（已有）
CONFIG_VIRTIO_NET=y             # Virtio 网络（已有）
CONFIG_SMP=y                    # 多核（已有）
CONFIG_SERIAL_AMBA_PL011=y      # PL011 UART（已有）
```

### 现有 build-kernel.sh 对比

现有脚本基于 `defconfig` + 手动 `scripts/config --enable`。对于 pKVM，**不需要额外 CONFIG 改动** — `defconfig` 已含 `CONFIG_KVM=y`。只需：

1. 在 DTB `chosen/bootargs` 添加 `kvm-arm.mode=protected`
2. （可选）添加 `CONFIG_NVHE_EL2_DEBUG=y` 用于调试

---

## 4. 内核分支选择

### 选项对比

| 分支 | 版本 | pKVM 完整度 | 说明 |
|------|------|-----------|------|
| **upstream mainline** 6.12+ | 6.12.12 (现有) | 基础设施有，部分 out-of-tree | Guest 作为 pKVM 保护 VM 的支持在 6.12 merged |
| **ACK** android15-6.6 | 6.6.x | **完整** | 生产级 pKVM，AVF 完整 |
| **ACK** android16-6.12 | 6.12.x | **完整** | 最新 ACK，完整 pKVM |
| **android-kvm.googlesource.com** | 多版本 | 开发前沿 | Google pKVM 团队的开发仓库 |

### 推荐

**Phase 4.5 用 upstream 6.12.12 即可**。原因：

1. 我们**不需要** pKVM 管理 protected VMs（Phase 4.5 只验证 FF-A 通路）
2. 我们只需要内核在 EL2 启动 → pKVM init → FF-A SMC 被拦截 → SPMD relay → 我们的 SPMC
3. upstream 6.12 的 `CONFIG_KVM=y` + `kvm-arm.mode=protected` 已包含完整的 pKVM host 侧初始化
4. `ffa.c` (FF-A 代理) 在 upstream 中已存在

如果 upstream 有缺失（例如 FF-A proxy 不完整），再考虑 ACK android16-6.12。

---

## 5. Boot Chain: BL33 的根本变化

### 当前 (run-tfa-linux-ffa)

```
BL1 → BL2 → BL31(SPMD) → BL32(Our SPMC) → BL33(Our Hypervisor @ NS-EL2) → Linux @ EL1
                                                    ↑
                                              我们的代码在 NS-EL2
                                              我们管控 Guest Stage-2
```

### Phase 4.5 目标

```
BL1 → BL2 → BL31(SPMD) → BL32(Our SPMC) → BL33(Linux+pKVM @ NS-EL2)
                                                    ↓
                                              head.S 检测 EL2
                                              kvm_arm_init() → nVHE hyp
                                              pkvm_drop_host_privileges()
                                              Host 降到 EL1
                                              pKVM 在 NS-EL2 代理 FF-A
```

**关键区别**：BL33 从"我们的 hypervisor"变成"Linux 内核本身"。Linux 自带的 pKVM 代码取代我们的 NS-EL2 hypervisor。

### QEMU 内存布局变化

| 组件 | 当前地址 | Phase 4.5 |
|------|---------|-----------|
| TF-A flash.bin | `-bios` | 不变 |
| 我们的 hypervisor | 0x40200000 | **删除** — pKVM 取代 |
| Linux Image (BL33) | 0x48000000 | **0x40200000** (PRELOADED_BL33_BASE) |
| DTB | 0x47000000 | 0x47000000 (不变) |
| Initramfs | 0x54000000 | 0x54000000 (不变) |
| Disk image | 0x58000000 | 0x58000000 (不变) |

Linux Image 加载到 0x40200000 = PRELOADED_BL33_BASE，TF-A 的 BL31 直接跳到那里进入内核。

---

## 6. DTB 修改

### 必须改动

**1. bootargs 添加 `kvm-arm.mode=protected`**：
```dts
chosen {
    bootargs = "earlycon=pl011,0x09000000 console=ttyAMA0 earlyprintk loglevel=8 nokaslr rdinit=/init kvm-arm.mode=protected";
};
```

**2. PSCI method 改为 `smc`**（TF-A 场景下必须）：
```dts
psci {
    method = "smc";  /* 当前是 "hvc" — TF-A 场景下 PSCI 走 SMC 到 EL3 */
    compatible = "arm,psci-1.0", "arm,psci-0.2";
};
```

当前 DTB 用 `method = "hvc"`，因为我们的 hypervisor 在 EL2 拦截 HVC。但 pKVM 场景下，PSCI 需要走 SMC 到 TF-A EL3 处理（pKVM 拦截后转发）。

**3. `arm,ffa` 节点**（已存在）：
```dts
firmware {
    arm_ffa {
        compatible = "arm,ffa";
        method = "smc";
    };
};
```

### 可能需要的改动

**4. memory 起始地址**：当前 memory@48000000。如果 Linux Image 加载到 0x40200000，可能需要调整 memory 节点：

```dts
memory@40200000 {
    reg = <0x00 0x40200000 0x00 0x40000000>;  /* ~1GB from 0x40200000 */
    device_type = "memory";
};
```

或保持 0x48000000 并让 0x40200000-0x48000000 区域仅用于内核代码（不在 memory 节点声明）。需要实验确定。

---

## 7. TF-A 构建修改

### 现有 TF-A build flags (build-tfa-full)

```bash
SPD=spmd
SPMD_SPM_AT_SEL2=1
CTX_INCLUDE_EL2_REGS=1
CTX_INCLUDE_FPREGS=1
ENABLE_SVE_FOR_NS=0
ENABLE_SME_FOR_NS=0
BL32=/output/bl32.bin           # Our SPMC
PRELOADED_BL33_BASE=0x40200000
```

### Phase 4.5 需要改什么？

**几乎不用改**。TF-A 构建已经：
- `SPMD_SPM_AT_SEL2=1` → SPMD 在 EL3，SPMC (我们的) 在 S-EL2
- `CTX_INCLUDE_EL2_REGS=1` → world switch 时保存/恢复 NS-EL2 寄存器（pKVM 需要）
- `PRELOADED_BL33_BASE=0x40200000` → BL33 入口地址

BL33 从我们的 hypervisor 换成 Linux 内核，但 TF-A 不关心 BL33 是什么 — 它只需要入口地址和 exception level (NS-EL2)。

---

## 8. QEMU 配置

### 新增 Makefile target 草案

```makefile
# pKVM DTB (adds kvm-arm.mode=protected, method=smc)
PKVM_DTB ?= guest/linux/guest-pkvm.dtb

# Boot: TF-A → SPMC (S-EL2) → pKVM kernel (NS-EL2) → Linux (NS-EL1)
run-pkvm:
    @test -f $(TFA_FLASH_FULL) || (echo "ERROR: Run 'make build-tfa-full' first." && exit 1)
    @echo "Starting pKVM + SPMC boot chain..."
    $(QEMU_SEL2) -machine virt,secure=on,virtualization=on,gic-version=3 \
        -cpu max,pauth-impdef=on -smp 4 -m 2G -nographic \
        -bios $(TFA_FLASH_FULL) \
        -device loader,file=$(LINUX_IMAGE),addr=0x40200000,force-raw=on \
        -device loader,file=$(PKVM_DTB),addr=0x47000000,force-raw=on \
        -device loader,file=$(LINUX_INITRAMFS),addr=0x54000000,force-raw=on \
        -device loader,file=$(LINUX_DISK),addr=0x58000000,force-raw=on \
        -nic none
```

与 `run-tfa-linux-ffa` 的区别：
- 不再加载 `$(BINARY_BIN)` (我们的 hypervisor)
- Linux Image 加载到 0x40200000（BL33 入口）
- 使用 `guest-pkvm.dtb`（含 `kvm-arm.mode=protected`、`method=smc`）
- 添加 `pauth-impdef=on` 加速 TCG PAC 仿真（2x 性能提升）

### QEMU TCG 兼容性

| 特性 | TCG 支持 | 备注 |
|------|---------|------|
| EL2 exception handling | ✅ | 已在我们的 hypervisor 中验证 |
| Stage-2 MMU (VTTBR_EL2) | ✅ | 已验证 |
| nVHE context switch | ✅ | EL2↔EL1 切换 |
| id_aa64mmfr1 override | ✅ | 内核侧 mask，不依赖 QEMU |
| PSCI SMC forwarding | ✅ | TF-A 已处理 |
| FF-A SMC via SPMD | ✅ | 已在 run-tfa-linux-ffa 中验证 |
| DMA/SMMU isolation | ⚠️ | QEMU SMMU 有限，pKVM DMA 保护不可全面测试 |
| 性能 | ❌ | TCG 10-50x 慢，pKVM Stage-2 增加额外开销 |

---

## 9. pKVM 内核构建脚本

基于现有 `guest/linux/build-kernel.sh` 的增量修改。**不需要新脚本** — 只需在现有脚本基础上添加 pKVM 调试选项（可选）：

```bash
# pKVM debug options (optional, for development)
scripts/config --enable CONFIG_NVHE_EL2_DEBUG
scripts/config --enable CONFIG_PROTECTED_NVHE_STACKTRACE
```

`CONFIG_KVM=y` 已在 arm64 `defconfig` 中默认开启，无需额外添加。

### 验证 pKVM 初始化成功

在 dmesg 中查找：
```
kvm [1]: Protected nVHE mode initialized successfully
```

或者（较新内核）：
```
kvm [1]: pKVM mode initialized successfully
```

如果看到以下输出则说明 VHE 没有被正确 override：
```
kvm [1]: VHE mode initialized successfully
```

---

## 10. 验证路径（Step-by-Step）

### Step 1: 确认 Linux 直接通过 TF-A 启动

```bash
make run-tfa-linux   # 已有 — 验证 TF-A → 我们的 hypervisor → Linux
```

### Step 2: Linux 内核直接作为 BL33

新建 `guest-pkvm.dtb`，修改 QEMU loader 参数，让 Linux 直接在 0x40200000 加载：

```bash
# 1. 创建 guest-pkvm.dts（bootargs + psci method=smc）
# 2. dtc 编译
# 3. QEMU: -device loader,file=Image,addr=0x40200000
# 4. 目标: Linux 在 NS-EL2 启动，正常 boot 到 shell
```

**不加 `kvm-arm.mode=protected`**，先验证 VHE 模式能正常 boot。

### Step 3: 启用 pKVM

在 DTB bootargs 添加 `kvm-arm.mode=protected`，验证 dmesg 输出：

```
kvm [1]: Protected nVHE mode initialized successfully
```

### Step 4: 验证 FF-A 通路

```bash
# dmesg 中查找 FF-A 驱动 probe
arm_ffa: FF-A driver registered
```

pKVM 的 `ffa.c` 代理应该拦截 FF-A SMC → 转发给 SPMD → 到我们的 SPMC。

### Step 5: DIRECT_REQ 端到端

从 Linux userspace 或内核模块发 FFA_MSG_SEND_DIRECT_REQ → pKVM 代理 → SPMD → SPMC → SP1。

---

## 11. 已知风险

| 风险 | 概率 | 影响 | 缓解 |
|------|------|------|------|
| VHE override 在 QEMU TCG 下静默失败 | 低 | 严重 | 用 `-cpu max,vh=off` 作为备选 |
| 内核 Image 在 0x40200000 与 QEMU DTB 冲突 | 中 | 中 | QEMU `-bios` 模式下 DTB 可能在 0x40000000；Linux Image 通常 > 40MB，可能覆盖此区域。需测试或用其他地址 |
| pKVM `finalize_pkvm()` 在 TCG 下失败 | 低 | 严重 | 检查 `__pkvm_init` 返回值，启用 `CONFIG_NVHE_EL2_DEBUG` |
| TCG 性能太慢无法完成 boot | 中 | 中 | 减少 SMP 数（`-smp 1`），减少 RAM（`-m 512M`），使用 `pauth-impdef=on` |
| upstream 6.12 的 ffa.c 缺少关键功能 | 低-中 | 中 | 对比 ACK android16-6.12 的 ffa.c，必要时 cherry-pick |
| PSCI method=smc 导致 secondary CPU 启动失败 | 中 | 中 | pKVM 拦截 PSCI → 转发 SMC 到 TF-A；需确认 SPMD 不干扰 PSCI 路径 |

---

## 12. 与现有构建的差异总结

| 项目 | 当前 (run-tfa-linux-ffa) | Phase 4.5 (run-pkvm) |
|------|-------------------------|----------------------|
| BL33 | 我们的 hypervisor (Rust) | Linux kernel (Image) |
| NS-EL2 代码 | 我们的 exception handler | pKVM nVHE hyp |
| Guest Stage-2 | 我们的 DynamicIdentityMapper | pKVM 的 host Stage-2 |
| FF-A 代理 | 我们的 `src/ffa/proxy.rs` | pKVM 的 `ffa.c` |
| DTB bootargs | `nokaslr rdinit=/init` | + `kvm-arm.mode=protected` |
| DTB psci | `method = "hvc"` | `method = "smc"` |
| 内核加载地址 | 0x48000000 | 0x40200000 (BL33 entry) |
| TF-A 构建 | 不变 | 不变 |

---

## References

- [pKVM Preamble patch series (v2, 47 patches)](https://lore.kernel.org/all/20240416095638.3620345-1-tabba@google.com/T/)
- [kvm-arm.mode as alias for id_aa64mmfr1.vh=0](https://www.mail-archive.com/linux-kernel@vger.kernel.org/msg2470014.html)
- [hVHE: Use VHE in nVHE hypervisor](https://lore.kernel.org/linux-arm-kernel/ZIdu0e857qPXPyZA@arm.com/T/)
- [hVHE in pKVM by default (2024)](https://lists.infradead.org/pipermail/linux-arm-kernel/2024-May/925372.html)
- [AVF architecture](https://source.android.com/docs/core/virtualization/architecture)
- [pKVM base support at EL2 (LWN)](https://lwn.net/Articles/895790/)
- [pkvm-aarch64 QEMU project](https://github.com/vrosendahl/pkvm-aarch64)
- [Will Deacon: Running ARM64 under QEMU](https://www.kernel.org/pub/linux/kernel/people/will/docs/qemu/qemu-arm64-howto.html)
- [TF-A Firmware Design](https://trustedfirmware-a.readthedocs.io/en/stable/design/firmware-design.html)
- [TF-A QEMU virt platform](https://trustedfirmware-a.readthedocs.io/en/stable/plat/qemu.html)
- [Android Common Kernels](https://source.android.com/docs/core/architecture/kernel/android-common)
