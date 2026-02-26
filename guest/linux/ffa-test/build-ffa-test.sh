#!/bin/bash
# Build FF-A test kernel module against the pKVM kernel source
# Runs inside the same Docker container used for build-pkvm-kernel
set -euo pipefail

KERNEL_BRANCH="android16-6.12"
KERNEL_DIR="/build/aosp-kernel-${KERNEL_BRANCH}"
MODULE_SRC="/module"

echo "=== Building FF-A Test Kernel Module ==="

# Install build dependencies (may already be present from kernel build)
apt-get update -qq
apt-get install -y -qq clang lld llvm gcc-aarch64-linux-gnu make bc kmod libelf-dev libssl-dev

# Verify kernel source exists (from previous build-pkvm-kernel)
if [ ! -d "${KERNEL_DIR}" ]; then
    echo "ERROR: Kernel source not found at ${KERNEL_DIR}"
    echo "Run 'make build-pkvm-kernel' first to populate the kernel source."
    exit 1
fi

cd "${KERNEL_DIR}"

# Generate Module.symvers if not present (Image-only build doesn't create it)
if [ ! -f "Module.symvers" ]; then
    echo ">>> Module.symvers not found, preparing kernel for module builds..."
    # Ensure .config exists
    if [ ! -f ".config" ]; then
        make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- gki_defconfig
        scripts/config --enable CONFIG_ARM_FFA_TRANSPORT
        make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig
    fi
    make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- modules_prepare
    # Create empty Module.symvers — external modules don't need kernel symbol versions
    touch Module.symvers
fi

echo ">>> Building module..."
make -C "${KERNEL_DIR}" M="${MODULE_SRC}" ARCH=arm64 LLVM=1 \
    CROSS_COMPILE=aarch64-linux-gnu- \
    KBUILD_MODPOST_WARN=1 \
    modules

echo ">>> Module built:"
ls -lh "${MODULE_SRC}/ffa_test.ko"

# Copy to output
cp "${MODULE_SRC}/ffa_test.ko" /output/ffa_test.ko
echo ">>> Module copied to /output/ffa_test.ko"
