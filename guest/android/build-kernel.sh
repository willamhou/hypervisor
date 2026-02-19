#!/bin/bash
# Build upstream Linux 6.6 LTS kernel with Android config for QEMU virt + our hypervisor
# Usage: docker run --rm -v /tmp/android-kernel-build:/build -v $PWD/guest/android:/output debian:bookworm-slim bash /output/build-kernel.sh
set -euo pipefail

KERNEL_VERSION=6.6.126
KERNEL_DIR=linux-${KERNEL_VERSION}
KERNEL_TARBALL=${KERNEL_DIR}.tar.xz
KERNEL_URL="https://cdn.kernel.org/pub/linux/kernel/v6.x/${KERNEL_TARBALL}"

NCPU=$(nproc)
export ARCH=arm64
export CROSS_COMPILE=aarch64-linux-gnu-

echo "=== Building Linux ${KERNEL_VERSION} with Android config for arm64 ==="
echo "=== Using ${NCPU} CPUs ==="

# Install build dependencies
apt-get update -qq
apt-get install -y -qq gcc-aarch64-linux-gnu make bc flex bison libssl-dev libelf-dev wget python3 kmod cpio rsync lz4 xz-utils

# Download kernel source if not present
cd /build
if [ ! -d "${KERNEL_DIR}" ]; then
    if [ ! -f "${KERNEL_TARBALL}" ]; then
        echo ">>> Downloading kernel source..."
        wget -q --show-progress "${KERNEL_URL}"
    fi
    echo ">>> Extracting kernel source..."
    tar xf "${KERNEL_TARBALL}"
fi

cd "${KERNEL_DIR}"

# Start with arm64 defconfig
echo ">>> Generating defconfig..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- defconfig

# Apply Android config fragment
echo ">>> Applying Android config fragment..."
while IFS= read -r line; do
    # Skip comments and empty lines
    [[ "$line" =~ ^#.*$ ]] && continue
    [[ -z "$line" ]] && continue
    # Parse CONFIG_FOO=y or CONFIG_FOO=n
    if [[ "$line" =~ ^(CONFIG_[A-Za-z0-9_]+)=(.+)$ ]]; then
        key="${BASH_REMATCH[1]}"
        val="${BASH_REMATCH[2]}"
        if [ "$val" = "n" ]; then
            scripts/config --disable "$key"
        else
            scripts/config --enable "$key"
        fi
    fi
done < /output/android-virt.config

# Resolve dependency conflicts
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig

# Verify critical configs
echo ">>> Verifying configs..."
grep -E "CONFIG_VIRTIO_MMIO|CONFIG_VIRTIO_BLK|CONFIG_ANDROID_BINDER" .config

# Build the kernel Image
echo ">>> Building kernel (this takes 10-30 minutes)..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j${NCPU} Image

echo ">>> Build complete!"
ls -lh arch/arm64/boot/Image

# Copy to output
cp arch/arm64/boot/Image /output/Image
echo ">>> Image copied to /output/Image"
echo ">>> Kernel version:"
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- kernelrelease
