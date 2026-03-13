#!/bin/bash
# Build pKVM initramfs with crosvm + embedded nested guest kernel/initramfs
# Produces: initramfs-crosvm.cpio.gz (BusyBox + crosvm + libs + nested kernel)
# The nested guest kernel is stored at /nested/Image inside the initramfs
set -euo pipefail

SRCDIR="$(cd "$(dirname "$0")" && pwd)"
CROSVM_BIN="${SRCDIR}/crosvm"
CROSVM_LIBS="${SRCDIR}/crosvm-libs"
BASE_INITRAMFS="${SRCDIR}/initramfs.cpio.gz"
PKVM_IMAGE="${SRCDIR}/Image-pkvm"
OUTPUT_INITRAMFS="${SRCDIR}/initramfs-crosvm.cpio.gz"
KVM_TEST="${SRCDIR}/kvm_test"

# Validate inputs
for f in "$CROSVM_BIN" "$BASE_INITRAMFS" "$PKVM_IMAGE"; do
    [ -f "$f" ] || { echo "ERROR: $f not found"; exit 1; }
done
[ -d "$CROSVM_LIBS" ] || { echo "ERROR: $CROSVM_LIBS not found"; exit 1; }

echo "=== Building crosvm initramfs ==="

# --- Build nested guest initramfs ---
echo ">>> Building nested guest initramfs..."
NESTED_DIR=$(mktemp -d)
mkdir -p "$NESTED_DIR"/{bin,proc,sys,dev}

EXTRACT=$(mktemp -d)
cd "$EXTRACT"
zcat "$BASE_INITRAMFS" | cpio -idm 2>/dev/null
cp "$EXTRACT/bin/busybox" "$NESTED_DIR/bin/busybox"

cd "$NESTED_DIR/bin"
for cmd in sh ls cat echo mount mkdir grep dmesg uname nproc sleep reboot poweroff; do
    ln -sf busybox "$cmd"
done

cat > "$NESTED_DIR/init" << 'NESTEDINIT'
#!/bin/sh
mount -t proc proc /proc 2>/dev/null
echo ""
echo "=========================================="
echo "  NESTED VM RUNNING INSIDE CROSVM!"
echo "  This is L2 (VM inside a VM)"
echo "=========================================="
echo ""
echo "Kernel: $(uname -r)"
echo "CPUs:   $(nproc 2>/dev/null || echo unknown)"
grep MemTotal /proc/meminfo 2>/dev/null
echo ""
echo "L2 nested virtualization: SUCCESS"
echo ""
echo "Shutting down nested VM..."
sleep 1
echo o > /proc/sysrq-trigger 2>/dev/null
exec /bin/sh
NESTEDINIT
chmod +x "$NESTED_DIR/init"

NESTED_INITRAMFS=$(mktemp)
cd "$NESTED_DIR"
find . | cpio -o -H newc 2>/dev/null | gzip > "$NESTED_INITRAMFS"
echo ">>> Nested initramfs: $(du -sh "$NESTED_INITRAMFS" | awk '{print $1}')"

# --- Build host initramfs with embedded nested guest ---
echo ">>> Building host initramfs..."
HOST_DIR=$(mktemp -d)
cd "$HOST_DIR"
zcat "$BASE_INITRAMFS" | cpio -idm 2>/dev/null

# Add crosvm binary + shared libs
cp "$CROSVM_BIN" bin/crosvm
chmod +x bin/crosvm
mkdir -p lib
cp "$CROSVM_LIBS"/ld-linux-aarch64.so.1 lib/
cp "$CROSVM_LIBS"/libc.so.6 lib/
cp "$CROSVM_LIBS"/libcap.so.2 lib/
cp "$CROSVM_LIBS"/libgcc_s.so.1 lib/

# Add KVM test binary if available
[ -f "$KVM_TEST" ] && cp "$KVM_TEST" bin/kvm_test && chmod +x bin/kvm_test

# Embed nested guest kernel + initramfs
mkdir -p nested
cp "$PKVM_IMAGE" nested/Image
cp "$NESTED_INITRAMFS" nested/initramfs.cpio.gz
echo ">>> Nested guest embedded: $(du -sh nested/ | awk '{print $1}')"

# Add BusyBox symlinks
cd bin
for cmd in sh ls cat echo mount umount mkdir mknod grep dmesg uname nproc sleep kill free head tail wc awk reboot poweroff; do
    ln -sf busybox "$cmd" 2>/dev/null
done
cd "$HOST_DIR"

# Create init script
cat > init << 'HOSTINIT'
#!/bin/sh
mkdir -p /proc /sys /dev /tmp
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t tmpfs tmpfs /dev
mount -t tmpfs tmpfs /tmp

# Device nodes
mknod /dev/null c 1 3
mknod /dev/zero c 1 5
mknod /dev/console c 5 1
mknod /dev/urandom c 1 9
mknod /dev/kvm c 10 232

sleep 1

echo ""
echo "============================================"
echo "  pKVM + crosvm Nested VM Test"
echo "============================================"
echo ""
echo "Host: $(uname -r), $(nproc) CPUs, $(grep MemTotal /proc/meminfo | awk '{print $2,$3}')"
echo ""

# KVM check
echo "=== KVM Check ==="
ls -la /dev/kvm 2>/dev/null && echo "[PASS] /dev/kvm exists" || echo "[FAIL] /dev/kvm missing"
if [ -x /bin/kvm_test ]; then
    /bin/kvm_test 2>/dev/null | grep -E "PASS|FAIL|INFO"
fi
echo ""

# Launch crosvm nested VM (kernel embedded in initramfs)
if [ -f /nested/Image ] && [ -f /nested/initramfs.cpio.gz ]; then
    echo "============================================"
    echo "  Launching Nested VM via crosvm..."
    echo "============================================"
    echo ""

    export LD_LIBRARY_PATH=/lib
    echo "[INFO] crosvm version:"
    /lib/ld-linux-aarch64.so.1 /bin/crosvm --version 2>&1 || true
    echo "[INFO] Starting crosvm run..."
    /lib/ld-linux-aarch64.so.1 /bin/crosvm run \
        --disable-sandbox \
        --serial type=stdout \
        --mem 128 \
        --cpus 1 \
        --initrd /nested/initramfs.cpio.gz \
        -p "console=ttyS0 earlycon=uart8250,mmio,0x3f8 reboot=t" \
        /nested/Image 2>&1 &
    CROSVM_PID=$!

    # Kill after 300 seconds (TCG emulation is very slow)
    (sleep 300 && kill $CROSVM_PID 2>/dev/null) &

    wait $CROSVM_PID 2>/dev/null
    echo ""
    echo "(crosvm exited)"
else
    echo "[SKIP] No nested guest kernel at /nested/Image"
fi

echo ""
echo "============================================"
echo "  Test Complete"
echo "============================================"

exec /bin/sh
HOSTINIT
chmod +x init

# Pack
find . | cpio -o -H newc 2>/dev/null | gzip > "$OUTPUT_INITRAMFS"
echo ">>> Host initramfs: $OUTPUT_INITRAMFS ($(du -sh "$OUTPUT_INITRAMFS" | awk '{print $1}'))"

# Cleanup
rm -rf "$NESTED_DIR" "$EXTRACT" "$NESTED_INITRAMFS" "$HOST_DIR"

echo ""
echo "=== Done ==="
echo "Initramfs: $OUTPUT_INITRAMFS"
