#!/bin/bash
# 自动安装开发依赖

set -e

echo "=========================================="
echo "  ARM64 Hypervisor 依赖安装"
echo "=========================================="
echo ""

# 检测操作系统
if [ -f /etc/os-release ]; then
    . /etc/os-release
    OS=$ID
else
    echo "无法检测操作系统"
    exit 1
fi

echo "检测到操作系统: $OS"
echo ""

# 1. 安装 Rust
echo "1. 安装 Rust 工具链"
echo "-------------------"
if ! command -v rustc &> /dev/null; then
    echo "正在下载并安装 Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "Rust 安装完成"
else
    echo "Rust 已安装: $(rustc --version)"
fi

echo "配置 Rust 工具链..."
rustup default nightly
rustup target add aarch64-unknown-none
rustup component add rust-src rustfmt clippy

echo ""

# 2. 安装交叉编译工具和 QEMU
echo "2. 安装交叉编译工具和 QEMU"
echo "-------------------"

case "$OS" in
    ubuntu|debian)
        echo "使用 apt 安装..."
        sudo apt update
        sudo apt install -y \
            gcc-aarch64-linux-gnu \
            binutils-aarch64-linux-gnu \
            qemu-system-aarch64 \
            gdb-multiarch \
            build-essential
        ;;
    
    fedora|rhel|centos)
        echo "使用 dnf/yum 安装..."
        sudo dnf install -y \
            gcc-aarch64-linux-gnu \
            binutils-aarch64-linux-gnu \
            qemu-system-aarch64 \
            gdb-multiarch \
            make
        ;;
    
    arch|manjaro)
        echo "使用 pacman 安装..."
        sudo pacman -S --noconfirm \
            aarch64-linux-gnu-gcc \
            aarch64-linux-gnu-binutils \
            qemu-arch-extra \
            gdb-multiarch \
            make
        ;;
    
    darwin)
        echo "使用 Homebrew 安装..."
        if ! command -v brew &> /dev/null; then
            echo "错误: 请先安装 Homebrew (https://brew.sh)"
            exit 1
        fi
        brew install \
            aarch64-elf-gcc \
            qemu \
            gdb
        ;;
    
    *)
        echo "不支持的操作系统: $OS"
        echo "请手动安装以下工具:"
        echo "  - aarch64 交叉编译工具链"
        echo "  - QEMU (aarch64)"
        echo "  - GDB (支持 aarch64)"
        exit 1
        ;;
esac

echo ""
echo "3. 验证安装"
echo "-------------------"
./scripts/check_env.sh

echo ""
echo "=========================================="
echo "安装完成！"
echo ""
echo "下一步:"
echo "  cd /home/willamhou/sides/hypervisor"
echo "  make build"
echo "  make run"
echo "=========================================="
