# 工具链和开发环境

## 概述

项目完全在 Linux x86_64 宿主机上运行，交叉编译到 ARM64。不需要物理硬件——QEMU 模拟一切，包括 GICv3、安全世界（EL3/S-EL2）和多 CPU。

## Rust 工具链

```toml
# rust-toolchain.toml
[toolchain]
channel = "nightly"
components = ["rust-src", "rustfmt", "clippy"]
```

需要 nightly 的原因：
- `#![no_std]` + `#![no_main]` 裸机二进制
- 内联汇编（`core::arch::asm!`）用于 ARM64 指令
- 自定义 target 规范

## 自定义 Target

```json
// aarch64-unknown-none.json
{
  "llvm-target": "aarch64-unknown-none",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "features": "+strict-align,+neon,+fp-armv8"
}
```

关键设置：
- **`panic-strategy: abort`** — 裸机环境没有 unwinding
- **`disable-redzone: true`** — 中断可能破坏 red zone
- **`+neon,+fp-armv8`** — 某些 Rust 代码生成路径需要浮点

## 交叉编译工具

| 工具 | 用途 |
|------|------|
| `aarch64-linux-gnu-gcc` | 编译 `boot.S` 和 `exception.S` |
| `aarch64-linux-gnu-ar` | 将汇编目标文件打包为 `libboot.a` |
| `aarch64-linux-gnu-objcopy` | 将 ELF 转换为 QEMU 使用的 raw binary |
| `rust-lld` | 用自定义链接脚本链接 Rust 代码 |

构建系统（`build.rs`）通过 `aarch64-linux-gnu-gcc` 交叉编译 ARM64 汇编，打包为静态库，然后用 `--whole-archive` 与 Rust 二进制链接。

## QEMU

```bash
# 普通模式（NS-EL2 hypervisor）
qemu-system-aarch64 \
  -machine virt,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic

# 安全世界模式（S-EL2 SPMC + TF-A）
qemu-system-aarch64 \
  -machine virt,secure=on,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic \
  -bios flash.bin
```

QEMU `virt` 机器提供：PL011 UART、GICv3、virtio-mmio 总线、PL031 RTC。加上 `secure=on` 后，支持 EL3 固件以实现 TF-A 引导链。

需要 QEMU 9.2.3（从源码构建），因为 `secure=on` + `virtualization=on` 组合模式需要较新版本。

## Docker 构建

重量级交叉编译任务在 Docker 容器中运行，使用持久化卷缓存：

| 目标 | Docker 卷 | 用途 |
|------|----------|------|
| Linux 内核 | `kernel-build-cache` | upstream 6.12.12 defconfig |
| pKVM 内核 | `pkvm-kernel-build-cache` | AOSP android16-6.12 + `gki_defconfig` |
| TF-A 固件 | `tfa-build-cache` / `tfa-pkvm-build-cache` | ARM Trusted Firmware v2.12 |

这保持宿主机干净，且构建可复现。

## 项目结构

```
hypervisor/
├── src/                    # Rust 源码
│   ├── main.rs             # 入口 + 测试编排
│   ├── vm.rs               # VM 生命周期
│   ├── vcpu.rs             # vCPU 状态机
│   ├── arch/aarch64/       # ARM64 特定代码
│   │   ├── boot.S          # EL2 入口点
│   │   ├── exception.S     # 异常向量
│   │   ├── linker.ld       # 内存布局
│   │   └── ...
│   ├── devices/            # 设备仿真
│   ├── ffa/                # FF-A v1.1 代理
│   └── ...
├── tests/                  # 33 个测试套件
├── tfa/                    # TF-A 配置、SP 二进制、BL33 测试客户端
├── guest/linux/            # 内核构建脚本、DTB、initramfs
├── Cargo.toml              # Features: linux_guest, multi_pcpu, sel2, vfiq, tfa_boot
└── Makefile                # 20+ 个目标
```

## 快速开始

```bash
# 构建 + 运行单元测试（不启动 guest，约 282 个断言）
make run

# 启动 Linux 到 BusyBox shell（4 vCPUs，virtio-blk）
make run-linux

# 启动 pKVM + 我们的 SPMC（需要先 Docker 构建）
make build-pkvm-kernel  # 首次约 15-30 分钟
make build-tfa-pkvm
make run-pkvm
```
