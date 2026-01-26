# ARM64 Hypervisor

一个使用 Rust 编写的教育性 ARM64 Type-1 Hypervisor 实现。

## 特性

- ✅ **vCPU 管理**: 完整的虚拟 CPU 抽象和上下文切换
- ✅ **Stage-2 内存管理**: Guest 物理地址到 Host 物理地址的转换
- ✅ **中断处理**: GIC 支持和 ARM Generic Timer
- ✅ **虚拟中断注入**: HCR_EL2.VI 机制，完整的 Guest 异常处理
- ✅ **设备模拟**: Trap-and-Emulate 架构，支持 UART 和 GICD
- ✅ **Hypercall 接口**: Guest 与 Hypervisor 通信机制
- ✅ **WFI 支持**: Wait-For-Interrupt 指令处理

## 当前状态

**版本**: v0.4.0 (Sprint 1.6 完成)
**进度**: Milestone 1 已完成 + 中断完善
**测试**: 7/7 (100% 通过)
**代码量**: ~4450 行

### 最新更新（2026-01-26）

Sprint 1.6 实现了完整的虚拟中断处理流程：
- Guest 异常向量表（2KB，16个向量）
- IRQ Handler 实现（上下文保存/恢复，EOI）
- WFI 指令支持（检测、跳过、恢复）
- 多次中断注入测试（3 次循环验证）

## 快速开始

### 前置要求

- Rust nightly (支持 no_std 和 ARM64 target)
- QEMU (qemu-system-aarch64)
- ARM64 交叉编译工具链 (aarch64-linux-gnu-*)

```bash
# 安装 Rust target
rustup target add aarch64-unknown-none

# 安装 QEMU (Ubuntu/Debian)
sudo apt install qemu-system-arm

# 安装交叉编译工具链
sudo apt install gcc-aarch64-linux-gnu
```

### 编译

```bash
make
```

### 运行

```bash
make run
```

退出 QEMU: 按 `Ctrl+A` 然后按 `X`

### 调试

```bash
# 在一个终端启动 GDB server
make debug

# 在另一个终端连接 GDB
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor
(gdb) target remote :1234
(gdb) b rust_main
(gdb) c
```

## 项目结构

```
hypervisor/
├── arch/aarch64/          # 汇编启动和异常处理代码
│   ├── boot.S            # 启动代码
│   └── exception.S       # 异常向量表和上下文切换
│
├── src/                   # Rust 源代码
│   ├── arch/aarch64/     # ARM64 架构特定代码
│   │   ├── hypervisor/   # EL2 特定实现
│   │   │   ├── exception.rs  # 异常处理
│   │   │   └── decode.rs     # 指令解码
│   │   ├── mm/           # 内存管理
│   │   │   └── mmu.rs    # Stage-2 页表
│   │   ├── peripherals/  # 外设驱动
│   │   │   ├── gic.rs    # GIC 支持
│   │   │   └── timer.rs  # ARM Generic Timer
│   │   └── regs.rs       # 寄存器定义
│   │
│   ├── devices/          # 设备模拟
│   │   ├── pl011/        # UART (PL011)
│   │   └── gic/          # GIC Distributor
│   │
│   ├── vcpu.rs           # vCPU 抽象
│   ├── vm.rs             # VM 管理
│   ├── global.rs         # 全局状态
│   ├── uart.rs           # UART 驱动
│   ├── lib.rs            # 库入口
│   └── main.rs           # 主程序
│
├── tests/                # 测试代码
│   ├── test_guest.rs     # Guest 执行测试
│   ├── test_timer.rs     # Timer 中断测试
│   └── test_mmio.rs      # MMIO 设备模拟测试
│
├── Cargo.toml            # Rust 项目配置
├── Makefile              # 构建脚本
├── aarch64-qemu.ld       # 链接脚本
├── PROGRESS.md           # 开发进度文档
└── README.md             # 本文件
```

## 技术详情

### 虚拟化模型

- **Type**: Type-1 (裸机 Hypervisor)
- **Privilege Level**: EL2 (Hypervisor mode)
- **Guest Level**: EL1 (Guest kernel mode)
- **Translation**: Stage-2 (IPA → PA)

### 内存管理

- **IPA Space**: 40-bit (1TB)
- **PA Space**: 48-bit (256TB)
- **Page Size**: 4KB granule
- **Mapping**: 2MB block mapping
- **Attributes**: NORMAL (cached), DEVICE (uncached), READONLY

### 中断处理

- **GIC Version**: GICv2
- **IRQ Routing**: HCR_EL2.IMO = 1 (route to EL2)
- **FIQ Routing**: HCR_EL2.FMO = 1 (route to EL2)
- **Timer**: ARM Generic Timer (Virtual Timer, PPI 27)

### 设备模拟

- **方法**: Trap-and-Emulate
- **MMIO 检测**: Data Abort (ESR_EL2.EC = 0x24/0x25)
- **指令解码**: ISS (Instruction Specific Syndrome)
- **支持设备**:
  - PL011 UART (0x09000000)
  - GIC Distributor (0x08000000)

## 开发进度

查看 [PROGRESS.md](PROGRESS.md) 了解详细的开发进度和技术笔记。

### 已完成

- ✅ Sprint 1.1: vCPU Framework
- ✅ Sprint 1.2: Memory Management  
- ✅ Sprint 1.3: Interrupt Handling
- ✅ Sprint 1.4: Device Emulation
- ✅ 目录结构重组 (Phase 1-3)

### 进行中

- 🔄 Phase 4: 文档完善
- 🔄 MMIO 测试调试

### 计划中

- Multi-vCPU support
- Guest interrupt injection
- Dynamic memory allocator
- More device emulation

## 测试

项目包含多个测试，在 `make run` 时自动运行：

1. **Guest Execution Test**: 测试基本的 guest 执行和 hypercall
2. **Timer Interrupt Test**: 测试 ARM Generic Timer 中断检测
3. **MMIO Device Test**: 测试设备模拟框架（调试中）

测试输出示例：

```
========================================
  ARM64 Hypervisor - Sprint 1.4
  Device Emulation Test
========================================

[INIT] Initializing at EL2...
[INIT] Current EL: EL2

[TEST] Starting guest execution test...
[GUEST] G!
[VCPU] Guest requested exit
[TEST] Guest exited successfully
```

## 参考资料

- [ARM Architecture Reference Manual](https://developer.arm.com/documentation/) - ARMv8-A 架构手册
- [Hafnium](https://github.com/TF-Hafnium/hafnium) - TensorFlow 的参考 Hypervisor
- [KVM/ARM](https://www.kernel.org/doc/html/latest/virt/kvm/arm/index.html) - Linux KVM ARM 实现
- [Rust Embedded Book](https://docs.rust-embedded.org/book/) - Embedded Rust 编程

## 贡献

这是一个教育性项目，欢迎：

- Bug 报告
- 功能建议
- 代码改进
- 文档完善

## 许可证

[待定]

## 致谢

- Rust 社区的 embedded-rs 生态
- QEMU 项目
- ARM 文档团队
- Hafnium 项目的架构灵感

---

**作者**: [你的名字]  
**创建时间**: 2026-01  
**最后更新**: 2026-01-26
