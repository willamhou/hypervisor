#!/bin/bash
# Build a minimal arm64 Linux kernel with virtio-mmio + virtio-blk support
# Runs inside a Debian container with cross-compilation tools
set -euo pipefail

KERNEL_VERSION=6.12.12
KERNEL_DIR=linux-${KERNEL_VERSION}
KERNEL_TARBALL=${KERNEL_DIR}.tar.xz
KERNEL_URL="https://cdn.kernel.org/pub/linux/kernel/v6.x/${KERNEL_TARBALL}"

NCPU=$(nproc)
export ARCH=arm64
export CROSS_COMPILE=aarch64-linux-gnu-

echo "=== Building Linux ${KERNEL_VERSION} for arm64 ==="
echo "=== Using ${NCPU} CPUs ==="

# Download kernel source if not already present
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

# Start with defconfig (already has virtio-mmio and virtio-blk for arm64)
echo ">>> Generating defconfig..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- defconfig

# Verify and force-enable the configs we need
echo ">>> Enabling virtio configs..."
scripts/config --enable CONFIG_VIRTIO
scripts/config --enable CONFIG_VIRTIO_MMIO
scripts/config --enable CONFIG_VIRTIO_BLK
scripts/config --enable CONFIG_VIRTIO_CONSOLE
scripts/config --enable CONFIG_VIRTIO_NET
scripts/config --enable CONFIG_BLK_DEV
scripts/config --enable CONFIG_EXT4_FS
scripts/config --enable CONFIG_TMPFS
scripts/config --enable CONFIG_DEVTMPFS
scripts/config --enable CONFIG_DEVTMPFS_MOUNT
scripts/config --enable CONFIG_BLK_DEV_INITRD
scripts/config --enable CONFIG_RD_GZIP

# Also enable some useful debug/serial options
scripts/config --enable CONFIG_SERIAL_AMBA_PL011
scripts/config --enable CONFIG_SERIAL_AMBA_PL011_CONSOLE
scripts/config --enable CONFIG_PRINTK
scripts/config --enable CONFIG_SMP

# FF-A transport driver (discovers Secure Partitions via SMC)
scripts/config --enable CONFIG_ARM_FFA_TRANSPORT

# Resolve any dependency issues
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- olddefconfig

# Verify our configs are set
echo ">>> Verifying virtio configs..."
grep -E "CONFIG_VIRTIO_MMIO|CONFIG_VIRTIO_BLK|CONFIG_VIRTIO=" .config

# Build the kernel Image
echo ">>> Building kernel (this may take a while)..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- -j${NCPU} Image

echo ">>> Build complete!"
ls -lh arch/arm64/boot/Image

# Copy to output
cp arch/arm64/boot/Image /output/Image
echo ">>> Image copied to /output/Image"
