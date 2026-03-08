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

# Resolve dependency issues first
make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig

# Override configs AFTER olddefconfig (GKI resets =y to =m for modules)
echo ">>> Forcing built-in configs (overriding GKI module policy)..."
scripts/config --set-val CONFIG_VIRTIO_BLK y
scripts/config --set-val CONFIG_ARM_FFA_TRANSPORT y
scripts/config --set-val CONFIG_VIRTIO_MMIO y
scripts/config --set-val CONFIG_VIRTIO_NET y
scripts/config --set-val CONFIG_EXT4_FS y
scripts/config --set-val CONFIG_BLK_DEV_INITRD y
scripts/config --set-val CONFIG_RD_GZIP y

# Second olddefconfig to resolve any new dependencies from forced =y
make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig

# Verify critical configs are built-in (=y, not =m)
echo ">>> Verifying configs..."
grep -E "CONFIG_KVM=|CONFIG_VIRTIO_BLK=|CONFIG_ARM_FFA_TRANSPORT=" .config
if grep -q "CONFIG_VIRTIO_BLK=m" .config; then
    echo "ERROR: CONFIG_VIRTIO_BLK still =m after override!"
    exit 1
fi

# Build the kernel Image
echo ">>> Building kernel with LLVM=1 (this may take a while)..."
make ARCH=arm64 LLVM=1 CROSS_COMPILE=aarch64-linux-gnu- -j${NCPU} Image

echo ">>> Build complete!"
ls -lh arch/arm64/boot/Image

# Copy to output
cp arch/arm64/boot/Image /output/Image-pkvm
echo ">>> Image copied to /output/Image-pkvm"
