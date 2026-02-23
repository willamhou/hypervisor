#!/bin/bash
# Build AOSP android16-6.12 kernel for pKVM (kvm-arm.mode=protected)
# Runs inside a Debian container with Clang cross-compilation tools
set -euo pipefail

KERNEL_REPO="https://android.googlesource.com/kernel/common"
KERNEL_BRANCH="android16-6.12"
KERNEL_DIR="aosp-kernel-${KERNEL_BRANCH}"

NCPU=$(nproc)
export ARCH=arm64
export CROSS_COMPILE=aarch64-linux-gnu-

echo "=== Building AOSP ${KERNEL_BRANCH} kernel for pKVM ==="
echo "=== Using ${NCPU} CPUs ==="

# Install Clang/LLVM (AOSP mandates Clang builds)
echo ">>> Installing Clang/LLVM and build dependencies..."
apt-get update -qq
apt-get install -y -qq \
    clang lld llvm \
    gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu \
    make bc flex bison libelf-dev libssl-dev libdw-dev \
    git cpio kmod

cd /build

# Shallow-clone AOSP kernel if not already present
if [ ! -d "${KERNEL_DIR}" ]; then
    echo ">>> Shallow-cloning ${KERNEL_BRANCH} (this may take a few minutes)..."
    git clone --depth 1 --single-branch --branch "${KERNEL_BRANCH}" \
        "${KERNEL_REPO}" "${KERNEL_DIR}"
fi

cd "${KERNEL_DIR}"
echo ">>> Kernel commit: $(git rev-parse HEAD)"

# Start with gki_defconfig (has KVM, VIRTIO_MMIO, VIRTIO_NET, SMP)
echo ">>> Generating gki_defconfig..."
make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- gki_defconfig

# Override configs: force built-in (gki has some as =m)
echo ">>> Enabling required configs..."
scripts/config --enable CONFIG_VIRTIO_BLK
scripts/config --enable CONFIG_ARM_FFA_TRANSPORT
scripts/config --enable CONFIG_VIRTIO_MMIO
scripts/config --enable CONFIG_VIRTIO_NET
scripts/config --enable CONFIG_EXT4_FS
scripts/config --enable CONFIG_BLK_DEV_INITRD
scripts/config --enable CONFIG_RD_GZIP

# Resolve dependency issues
make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig

# Verify critical configs
echo ">>> Verifying configs..."
grep -E "CONFIG_KVM=|CONFIG_VIRTIO_BLK=|CONFIG_ARM_FFA_TRANSPORT=" .config

# Build the kernel Image
echo ">>> Building kernel with LLVM=1 (this may take a while)..."
make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- -j${NCPU} Image

echo ">>> Build complete!"
ls -lh arch/arm64/boot/Image

# Copy to output
cp arch/arm64/boot/Image /output/Image-pkvm
echo ">>> Image copied to /output/Image-pkvm"
