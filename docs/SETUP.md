# 开发环境安装指南

本文档提供详细的开发环境安装步骤。

## 1. 安装 Rust 工具链

### 1.1 安装 rustup

```bash
# 下载并安装 rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 选择默认安装（按 1）
# 安装完成后，重新加载环境
source $HOME/.cargo/env
```

### 1.2 配置 Rust 工具链

```bash
# 设置为 nightly 版本（项目需要）
rustup default nightly

# 添加 aarch64 裸机目标
rustup target add aarch64-unknown-none

# 安装必要组件
rustup component add rust-src rustfmt clippy
```

### 1.3 验证安装

```bash
# 检查版本
rustc --version
cargo --version

# 应该看到类似输出：
# rustc 1.x.x-nightly (hash date)
# cargo 1.x.x-nightly (hash date)
```

## 2. 安装交叉编译工具链

### Ubuntu/Debian

```bash
sudo apt update
sudo apt install -y \
    gcc-aarch64-linux-gnu \
    binutils-aarch64-linux-gnu \
    build-essential
```

### macOS

```bash
# 使用 Homebrew
brew install aarch64-elf-gcc
```

### 验证

```bash
aarch64-linux-gnu-gcc --version
# 或 (macOS)
aarch64-elf-gcc --version
```

## 3. 安装 QEMU

### Ubuntu/Debian

```bash
sudo apt install -y qemu-system-aarch64
```

### macOS

```bash
brew install qemu
```

### 验证

```bash
qemu-system-aarch64 --version

# 应该看到 QEMU emulator version 7.0+ 或更高
```

## 4. 安装 GDB（调试用，可选）

### Ubuntu/Debian

```bash
sudo apt install -y gdb-multiarch
```

### macOS

```bash
brew install gdb

# macOS 需要额外配置代码签名，参考：
# https://sourceware.org/gdb/wiki/PermissionsDarwin
```

### 验证

```bash
gdb-multiarch --version
# 或 (macOS)
gdb --version
```

## 5. 构建项目

现在环境已经准备好，可以构建项目了：

```bash
cd /home/willamhou/sides/hypervisor

# 方式 1: 使用 Makefile
make build

# 方式 2: 直接使用 cargo
cargo build --target aarch64-unknown-none
```

### 预期输出

```
   Compiling hypervisor v0.1.0 (/home/willamhou/sides/hypervisor)
    Finished dev [unoptimized + debuginfo] target(s) in x.xxs
```

## 6. 运行测试

```bash
make run
```

### 预期输出

```
Starting QEMU...
Press Ctrl+A then X to exit QEMU
========================================
  ARM64 Hypervisor - Milestone 0
========================================

Hello from EL2!

System Information:
  - Exception Level: EL2 (Hypervisor)
  - Architecture: AArch64
  - Target: QEMU virt machine

Project initialized successfully!
========================================
```

**退出 QEMU**: 按 `Ctrl+A`，然后按 `X`

## 7. 调试配置（可选）

### 7.1 启动 GDB 调试

终端 1（启动 QEMU 调试服务器）:
```bash
make debug
```

终端 2（连接 GDB）:
```bash
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor

# 在 GDB 提示符中：
(gdb) target remote :1234
(gdb) break rust_main
(gdb) continue
```

### 7.2 常用 GDB 命令

```gdb
# 查看寄存器
(gdb) info registers

# 查看当前异常级别
(gdb) p/x $CurrentEL

# 单步执行
(gdb) step

# 继续执行
(gdb) continue

# 查看栈回溯
(gdb) backtrace
```

## 8. 开发工具推荐

### VS Code

推荐安装以下扩展：

1. **rust-analyzer**: Rust 语言支持
2. **CodeLLDB**: 调试支持
3. **ARM Assembly**: ARM 汇编语法高亮

配置文件 `.vscode/settings.json`:
```json
{
    "rust-analyzer.cargo.target": "aarch64-unknown-none",
    "rust-analyzer.checkOnSave.allTargets": false
}
```

## 9. 常见问题

### Q1: cargo build 报错 "linker not found"

**解决**: 确保安装了 aarch64 交叉编译工具链（步骤 2）

### Q2: QEMU 启动后无输出

**解决**: 
- 检查是否使用了 `-nographic` 参数
- 确认 UART 基地址正确（0x0900_0000 for QEMU virt）

### Q3: Rust nightly 版本不兼容

**解决**: 项目的 `rust-toolchain.toml` 会自动选择兼容版本，确保运行：
```bash
rustup update
```

### Q4: macOS 上 GDB 权限问题

**解决**: 需要为 GDB 创建代码签名证书，参考官方文档：
https://sourceware.org/gdb/wiki/PermissionsDarwin

## 10. 下一步

环境安装完成后，按照开发计划继续：

1. ✅ 运行 `make run` 验证 "Hello from EL2!"
2. 📝 阅读 [DEVELOPMENT_PLAN.md](../DEVELOPMENT_PLAN.md) 了解后续任务
3. 🚀 开始 Sprint 1.1: vCPU 框架开发

祝开发顺利！🎉
