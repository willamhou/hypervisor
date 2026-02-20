#!/bin/bash
# Build ARM Trusted Firmware (TF-A) with SPD=spmd for QEMU virt.
# Produces flash.bin (BL1 + FIP) for booting our SPMC at S-EL2.
# Runs inside a Debian container.
#
# Usage (from project root):
#   docker run --rm -v $(pwd)/tfa:/output -v $(pwd):/src \
#       debian:bookworm-slim bash /src/scripts/build-tfa.sh
#
# Prerequisites:
#   /src/tfa/bl32.bin         — BL32 binary (SPMC, loaded at S-EL2)
#   /src/tfa/bl33.bin         — BL33 binary (NS payload, loaded at NS-EL2)
#   /src/tfa/spmc_manifest.dts — SPMC manifest (DTS format)
#
# Output: /output/flash.bin

set -euo pipefail

TFA_VERSION=v2.12.0
TFA_URL="https://github.com/ARM-software/arm-trusted-firmware/archive/refs/tags/${TFA_VERSION}.tar.gz"

NCPU=$(nproc)

echo "=== Building TF-A ${TFA_VERSION} for QEMU virt (SPD=spmd, S-EL2) ==="
echo "=== Using ${NCPU} CPUs ==="

# Verify inputs exist
for f in /src/tfa/bl32.bin /src/tfa/bl33.bin /src/tfa/spmc_manifest.dts; do
    if [ ! -f "$f" ]; then
        echo "ERROR: Missing required file: $f"
        exit 1
    fi
done

# Install build dependencies
apt-get update -qq
apt-get install -y -qq \
    wget \
    build-essential \
    python3 \
    libssl-dev \
    gcc-aarch64-linux-gnu \
    binutils-aarch64-linux-gnu \
    device-tree-compiler \
    2>/dev/null

cd /output

# Download TF-A source if not already present
# Check for Makefile (not just directory) because Docker volume mounts create empty dirs
if [ ! -f "tfa-src/Makefile" ]; then
    echo ">>> Downloading TF-A ${TFA_VERSION}..."
    # Don't rm -rf tfa-src — it may be a Docker volume mount point (device busy)
    wget -q --show-progress "${TFA_URL}" -O /tmp/tfa.tar.gz
    tar xf /tmp/tfa.tar.gz -C tfa-src --strip-components=1
    rm -f /tmp/tfa.tar.gz
fi

cd tfa-src

# Support PRELOADED_BL33_BASE: BL2 skips loading BL33 from FIP,
# uses the specified address as BL33 entry point instead.
# QEMU's -device loader pre-loads the actual binary at that address.
EXTRA_ARGS=""
if [ -n "${TFA_PRELOADED_BL33_BASE:-}" ]; then
    EXTRA_ARGS="PRELOADED_BL33_BASE=${TFA_PRELOADED_BL33_BASE}"
    echo ">>> PRELOADED_BL33_BASE=${TFA_PRELOADED_BL33_BASE}"
    # Force full rebuild when switching between FIP-loaded and preloaded BL33
    make PLAT=qemu realclean 2>/dev/null || true
fi

# Build TF-A with SPMD at EL3 + SPMC at S-EL2
echo ">>> Building TF-A..."
make -j${NCPU} \
    CROSS_COMPILE=aarch64-linux-gnu- \
    PLAT=qemu \
    SPD=spmd \
    SPMD_SPM_AT_SEL2=1 \
    CTX_INCLUDE_EL2_REGS=1 \
    ENABLE_FEAT_SEL2=1 \
    ARM_ARCH_MINOR=5 \
    QEMU_USE_GIC_DRIVER=QEMU_GICV3 \
    SP_LAYOUT_FILE=/src/tfa/sp_layout.json \
    BL32=/src/tfa/bl32.bin \
    BL33=/src/tfa/bl33.bin \
    QEMU_TOS_FW_CONFIG_DTS=/src/tfa/spmc_manifest.dts \
    QEMU_TB_FW_CONFIG_DTS=/src/tfa/tb_fw_config.dts \
    DEBUG=1 \
    ${EXTRA_ARGS} \
    all fip

# Determine build output directory based on DEBUG setting
BUILD_OUT=build/qemu/debug

echo ">>> TF-A build complete!"
ls -lh ${BUILD_OUT}/bl1.bin ${BUILD_OUT}/fip.bin

# Create flash.bin (BL1 at offset 0 + FIP at offset 256KB)
echo ">>> Creating flash.bin..."
dd if=/dev/zero of=/output/flash.bin bs=1M count=64 2>/dev/null
dd if=${BUILD_OUT}/bl1.bin of=/output/flash.bin bs=4096 conv=notrunc 2>/dev/null
dd if=${BUILD_OUT}/fip.bin of=/output/flash.bin seek=64 bs=4096 conv=notrunc 2>/dev/null

echo ">>> flash.bin created at /output/flash.bin"
ls -lh /output/flash.bin
