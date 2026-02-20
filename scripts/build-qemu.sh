#!/bin/bash
# Build QEMU 9.2.3 from source for S-EL2 support.
# Runs inside a Debian container.
#
# Usage (from project root):
#   mkdir -p tools
#   docker run --rm -v $(pwd)/tools:/output -v $(pwd)/scripts:/scripts \
#       debian:bookworm-slim bash /scripts/build-qemu.sh
#
# Output: /output/qemu-system-aarch64

set -euo pipefail

QEMU_VERSION=9.2.3
QEMU_DIR=qemu-${QEMU_VERSION}
QEMU_TARBALL=${QEMU_DIR}.tar.xz
QEMU_URL="https://download.qemu.org/${QEMU_TARBALL}"

NCPU=$(nproc)

echo "=== Building QEMU ${QEMU_VERSION} (aarch64-softmmu) ==="
echo "=== Using ${NCPU} CPUs ==="

# Install build dependencies
apt-get update -qq
apt-get install -y -qq \
    wget xz-utils git \
    build-essential \
    python3 python3-venv \
    ninja-build \
    pkg-config \
    libglib2.0-dev \
    libpixman-1-dev \
    libfdt-dev \
    2>/dev/null

cd /build

# Download QEMU source if not already present
if [ ! -d "${QEMU_DIR}" ]; then
    if [ ! -f "${QEMU_TARBALL}" ]; then
        echo ">>> Downloading QEMU ${QEMU_VERSION}..."
        wget -q --show-progress "${QEMU_URL}"
    fi
    echo ">>> Extracting QEMU source..."
    tar xf "${QEMU_TARBALL}"
fi

cd "${QEMU_DIR}"

# Configure — minimal build for aarch64 softmmu only
echo ">>> Configuring QEMU..."
./configure \
    --target-list=aarch64-softmmu \
    --enable-tcg \
    --disable-kvm \
    --disable-docs \
    --disable-guest-agent \
    --disable-tools \
    --disable-user \
    --disable-slirp

# Build
echo ">>> Building QEMU (this may take a while)..."
ninja -C build -j${NCPU}

echo ">>> Build complete!"
build/qemu-system-aarch64 --version

# Copy binary to output
cp build/qemu-system-aarch64 /output/qemu-system-aarch64
echo ">>> QEMU copied to /output/qemu-system-aarch64"
