#!/bin/sh
#
# Minimal Android-style /init for hypervisor Phase 2 validation.
#
# Validates: filesystem mounts, binder support, PL031 RTC, system info.
# Drops to BusyBox shell for interactive debugging.

echo ""
echo "============================================"
echo "  Android Minimal Init (Phase 2)"
echo "  Hypervisor Guest - PID $$"
echo "============================================"
echo ""

# ── Mount filesystems ────────────────────────────────────────────
echo "[init] Mounting filesystems..."
mkdir -p /proc /sys /dev /dev/pts /tmp
mount -t proc proc /proc
echo "[init] Mounted proc on /proc"
mount -t sysfs sysfs /sys
echo "[init] Mounted sysfs on /sys"
mount -t devtmpfs devtmpfs /dev
echo "[init] Mounted devtmpfs on /dev"
mount -t devpts devpts /dev/pts 2>/dev/null
mount -t tmpfs tmpfs /tmp -o size=64m 2>/dev/null

# ── Parse init.rc ────────────────────────────────────────────────
echo "[init] Parsing /init.rc..."
if [ -f /init.rc ]; then
    while IFS= read -r line; do
        case "$line" in
            '#'*|'') continue ;;
            hostname\ *)
                HNAME="${line#hostname }"
                hostname "$HNAME"
                echo "[init] Set hostname: $HNAME"
                ;;
            *)
                echo "[init] RC: $line"
                ;;
        esac
    done < /init.rc
else
    echo "[init] WARN: /init.rc not found (OK for Phase 2)"
fi

# ── Check binder support ────────────────────────────────────────
echo "[init] Checking binder support..."
if grep -q binder /proc/filesystems 2>/dev/null; then
    echo "[init] OK: binder filesystem type registered"
    mkdir -p /dev/binderfs
    if mount -t binder binder /dev/binderfs 2>/dev/null; then
        echo "[init] OK: binderfs mounted at /dev/binderfs"
    else
        echo "[init] WARN: binderfs mount failed"
    fi
else
    echo "[init] WARN: binder not found in /proc/filesystems"
fi

# ── Check PL031 RTC ─────────────────────────────────────────────
echo "[init] Checking PL031 RTC..."
if [ -f /sys/class/rtc/rtc0/since_epoch ]; then
    EPOCH=$(cat /sys/class/rtc/rtc0/since_epoch)
    echo "[init] OK: RTC time (since_epoch): $EPOCH"
elif [ -c /dev/rtc0 ]; then
    echo "[init] OK: /dev/rtc0 device exists"
else
    echo "[init] WARN: No RTC device found"
fi

# ── System info ──────────────────────────────────────────────────
echo ""
echo "[init] System info:"
if [ -f /proc/version ]; then
    echo "[init] Kernel: $(cat /proc/version)"
fi
if [ -f /proc/cpuinfo ]; then
    CPUS=$(grep -c ^processor /proc/cpuinfo)
    echo "[init] CPUs: $CPUS"
fi
if [ -f /proc/meminfo ]; then
    echo "[init] $(head -1 /proc/meminfo)"
fi

# ── Start shell ──────────────────────────────────────────────────
echo ""
echo "[init] Starting shell..."
echo "[init] Type commands at the prompt. Ctrl+A X to exit QEMU."
echo ""

exec /bin/sh
