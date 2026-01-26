#!/bin/bash
# 环境检查脚本

set -e

echo "=========================================="
echo "  ARM64 Hypervisor 环境检查"
echo "=========================================="
echo ""

# 颜色定义
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

check_command() {
    local cmd=$1
    local name=$2
    local install_hint=$3
    
    if command -v "$cmd" &> /dev/null; then
        echo -e "${GREEN}✓${NC} $name: $(command -v $cmd)"
        return 0
    else
        echo -e "${RED}✗${NC} $name: 未安装"
        if [ -n "$install_hint" ]; then
            echo -e "  ${YELLOW}安装提示: $install_hint${NC}"
        fi
        return 1
    fi
}

check_rust_target() {
    if rustup target list --installed 2>/dev/null | grep -q "aarch64-unknown-none"; then
        echo -e "${GREEN}✓${NC} aarch64-unknown-none target: 已安装"
        return 0
    else
        echo -e "${RED}✗${NC} aarch64-unknown-none target: 未安装"
        echo -e "  ${YELLOW}运行: rustup target add aarch64-unknown-none${NC}"
        return 1
    fi
}

check_rust_component() {
    local component=$1
    if rustup component list --installed 2>/dev/null | grep -q "^$component"; then
        echo -e "${GREEN}✓${NC} Rust component '$component': 已安装"
        return 0
    else
        echo -e "${RED}✗${NC} Rust component '$component': 未安装"
        echo -e "  ${YELLOW}运行: rustup component add $component${NC}"
        return 1
    fi
}

all_ok=true

echo "1. 检查 Rust 工具链"
echo "-------------------"
if check_command "rustc" "rustc" "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; then
    rustc --version
else
    all_ok=false
fi

if check_command "cargo" "cargo" "（随 rustc 一起安装）"; then
    cargo --version
else
    all_ok=false
fi

if command -v rustc &> /dev/null; then
    check_rust_target || all_ok=false
    check_rust_component "rust-src" || all_ok=false
    check_rust_component "rustfmt" || all_ok=false
    check_rust_component "clippy" || all_ok=false
fi

echo ""
echo "2. 检查交叉编译工具"
echo "-------------------"
if check_command "aarch64-linux-gnu-gcc" "aarch64-linux-gnu-gcc" "Ubuntu/Debian: sudo apt install gcc-aarch64-linux-gnu"; then
    aarch64-linux-gnu-gcc --version | head -1
elif check_command "aarch64-elf-gcc" "aarch64-elf-gcc" "macOS: brew install aarch64-elf-gcc"; then
    aarch64-elf-gcc --version | head -1
else
    all_ok=false
fi

echo ""
echo "3. 检查 QEMU"
echo "-------------------"
if check_command "qemu-system-aarch64" "QEMU (aarch64)" "Ubuntu/Debian: sudo apt install qemu-system-aarch64 | macOS: brew install qemu"; then
    qemu-system-aarch64 --version | head -1
else
    all_ok=false
fi

echo ""
echo "4. 检查 GDB（可选）"
echo "-------------------"
if check_command "gdb-multiarch" "gdb-multiarch" "Ubuntu/Debian: sudo apt install gdb-multiarch"; then
    gdb-multiarch --version | head -1
elif check_command "gdb" "gdb" "macOS: brew install gdb"; then
    gdb --version | head -1
else
    echo -e "${YELLOW}⚠${NC} GDB 未安装（调试时需要，但不是必需的）"
fi

echo ""
echo "5. 检查构建工具"
echo "-------------------"
check_command "make" "make" "Ubuntu/Debian: sudo apt install build-essential" || all_ok=false

echo ""
echo "=========================================="
if $all_ok; then
    echo -e "${GREEN}✓ 所有必需工具已安装！${NC}"
    echo ""
    echo "下一步："
    echo "  1. make build  # 构建项目"
    echo "  2. make run    # 运行 QEMU"
    echo ""
else
    echo -e "${RED}✗ 部分工具未安装${NC}"
    echo ""
    echo "请根据上面的提示安装缺失的工具"
    echo "详细安装指南: docs/SETUP.md"
    exit 1
fi
echo "=========================================="
