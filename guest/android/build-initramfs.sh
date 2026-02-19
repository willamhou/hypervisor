#!/bin/bash
# Build Android Phase 2 initramfs (shell /init + BusyBox)
#
# Usage (via Docker):
#   docker run --rm \
#     -v $PWD/guest/android:/android \
#     -v $PWD/guest/linux:/linux \
#     debian:bookworm-slim \
#     bash /android/build-initramfs.sh
#
# Output: /android/initramfs.cpio.gz

set -euo pipefail

echo "=== Building Android Phase 2 initramfs ==="

# Install tools
apt-get update -qq
apt-get install -y -qq cpio gzip > /dev/null 2>&1

WORKDIR=/tmp/initramfs-build
rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"

# ── 1. Install shell /init ────────────────────────────────────────
echo "[1/3] Installing /init shell script..."
cp /android/init.sh "$WORKDIR/init"
chmod 755 "$WORKDIR/init"

# ── 2. Create directory structure ────────────────────────────────
echo "[2/3] Creating directory structure..."
cd "$WORKDIR"
mkdir -p bin sbin etc proc sys dev dev/pts tmp mnt usr/bin usr/sbin

# ── 3. Copy BusyBox from existing Linux initramfs ────────────────
echo "[3/3] Extracting BusyBox from Linux initramfs..."
LINUX_RAMFS=/linux/initramfs.cpio.gz
if [ ! -f "$LINUX_RAMFS" ]; then
    echo "ERROR: $LINUX_RAMFS not found"
    exit 1
fi

# Extract busybox binary from existing initramfs
EXTRACT_DIR=/tmp/linux-extract
rm -rf "$EXTRACT_DIR"
mkdir -p "$EXTRACT_DIR"
cd "$EXTRACT_DIR"
zcat "$LINUX_RAMFS" | cpio -idm 2>/dev/null
cd "$WORKDIR"

# Copy busybox binary
cp "$EXTRACT_DIR/bin/busybox" bin/busybox

# Create symlinks (same set as existing initramfs)
cd bin
for cmd in sh ash cat ls echo grep mount mkdir cp mv rm ln \
           chmod chown ps kill sleep date dmesg df du free \
           head tail sed awk sort uniq wc cut printf tee \
           touch stat vi top od hexdump clear reset hostname; do
    ln -sf busybox "$cmd"
done
cd ..

# sbin symlinks
cd sbin
ln -sf ../bin/busybox ifconfig
ln -sf ../bin/busybox reboot
ln -sf ../bin/busybox halt
cd ..

# Copy init.rc
cp /android/init.rc "$WORKDIR/init.rc"

# Create cpio archive
echo "[*] Packing initramfs..."
cd "$WORKDIR"
find . | cpio -o -H newc 2>/dev/null | gzip > /android/initramfs.cpio.gz

echo ""
echo "=== Android initramfs built ==="
echo "  Output: /android/initramfs.cpio.gz"
echo "  Size: $(du -h /android/initramfs.cpio.gz | cut -f1)"
echo "  Contents:"
echo "    /init          - Android-style init script (shell)"
echo "    /init.rc       - Minimal config"
echo "    /bin/busybox   - BusyBox (from existing Linux initramfs)"
echo "    /bin/*         - Shell commands (symlinks to busybox)"
