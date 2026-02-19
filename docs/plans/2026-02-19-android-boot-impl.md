# Android Boot Phase 1 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot upstream Linux 6.6 LTS kernel with Android config (Binder, Binderfs) on the hypervisor with existing BusyBox initramfs, validating hypervisor compatibility with Android-configured kernel.

**Architecture:** Build upstream kernel.org 6.6 LTS kernel via Docker (same pattern as existing `guest/linux/build-kernel.sh`), with Android-specific config fragment (Binder IPC, Binderfs, etc.) on top of arm64 `defconfig`. Produce an arm64 `Image` file and boot it with existing `make run-linux` infrastructure. No hypervisor code changes needed — this is a kernel swap. If new sysreg traps appear, add minimal emulation stubs. Phase 3 will switch to a real AOSP kernel source with Clang/LLVM.

**Tech Stack:** Upstream kernel.org 6.6 LTS source, GCC cross-compiler, Docker for cross-compilation, existing QEMU/Makefile infrastructure.

---

### Task 1: Create Android Kernel Build Script

**Files:**
- Create: `guest/android/build-kernel.sh`
- Create: `guest/android/android-virt.config` (kernel config fragment)

**Step 1: Create kernel config fragment**

This fragment enables Android-specific features + QEMU virt platform drivers on top of arm64 defconfig.

```bash
# File: guest/android/android-virt.config
# Config fragment for upstream 6.6 LTS with Android features on QEMU virt
# Applied on top of arm64 defconfig via scripts/config

# Virtio (built-in, not modules — our hypervisor loads kernel directly)
CONFIG_VIRTIO=y
CONFIG_VIRTIO_MMIO=y
CONFIG_VIRTIO_BLK=y
CONFIG_VIRTIO_NET=y
CONFIG_VIRTIO_CONSOLE=y

# Block device + filesystem
CONFIG_BLK_DEV=y
CONFIG_EXT4_FS=y
CONFIG_TMPFS=y
CONFIG_DEVTMPFS=y
CONFIG_DEVTMPFS_MOUNT=y

# Initramfs support
CONFIG_BLK_DEV_INITRD=y
CONFIG_RD_GZIP=y

# Serial console (PL011)
CONFIG_SERIAL_AMBA_PL011=y
CONFIG_SERIAL_AMBA_PL011_CONSOLE=y

# Basic kernel options
CONFIG_PRINTK=y
CONFIG_SMP=y
CONFIG_NR_CPUS=8

# Android-specific (available in upstream 6.6, needed for later phases)
CONFIG_ANDROID=y
CONFIG_ANDROID_BINDER_IPC=y
CONFIG_ANDROID_BINDERFS=y

# Disable BTF (reduces build deps and image size)
CONFIG_DEBUG_INFO_BTF=n
```

Save to `guest/android/android-virt.config`.

**Step 2: Create the build script**

```bash
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
```

Save to `guest/android/build-kernel.sh` and `chmod +x`.

**Step 3: Commit**

```bash
git add guest/android/build-kernel.sh guest/android/android-virt.config
git commit -m "feat: add Linux 6.6 LTS + Android config kernel build script"
```

---

### Task 2: Build the Android-Configured Kernel

**Files:**
- Output: `guest/android/Image` (the compiled kernel)

**Step 1: Create build volume directory**

```bash
mkdir -p guest/android
```

**Step 2: Run Docker build**

```bash
docker run --rm \
    -v /tmp/android-kernel-build:/build \
    -v $PWD/guest/android:/output \
    debian:bookworm-slim \
    bash /output/build-kernel.sh
```

This takes 10-30 minutes depending on CPU count. The `-v /tmp/android-kernel-build:/build` caches the kernel source across rebuilds.

**Step 3: Verify output**

```bash
ls -lh guest/android/Image
file guest/android/Image
```

Expected: `guest/android/Image` is an ARM64 Linux kernel Image, ~20-30MB.
The `file` command should show: `Linux kernel ARM64 boot executable Image`.

**Step 4: Verify initramfs fits in declared DTB region**

```bash
ls -l guest/linux/initramfs.cpio.gz
```

The initramfs must be smaller than 2MB (0x200000 bytes) to fit the DTB declaration `initrd-start=0x54000000, initrd-end=0x54200000`. If larger, adjust `linux,initrd-end` in `guest/linux/guest.dts`.

**Step 5: Add Image to .gitignore (don't commit binaries)**

Add to `.gitignore`:
```
guest/android/Image
```

---

### Task 3: Add Makefile Target for Android Guest

**Files:**
- Modify: `Makefile` (add `run-android` target)

**Step 1: Add Android guest paths and target**

Add after the `run-multi-vm` target (line ~118):

```makefile
# Android guest paths (Phase 1: upstream 6.6 LTS + Android config, reuses Linux DTB/initramfs)
ANDROID_IMAGE ?= guest/android/Image
ANDROID_INITRAMFS ?= guest/linux/initramfs.cpio.gz
ANDROID_DISK ?= guest/linux/disk.img

# QEMU flags for Android guest (2GB RAM for future phases)
QEMU_FLAGS_ANDROID := -machine virt,virtualization=on,gic-version=3 \
              -cpu max \
              -smp 4 \
              -m 2G \
              -nographic \
              -kernel $(BINARY)

# Run hypervisor with Android-configured kernel (Phase 1: BusyBox shell)
run-android:
	@echo "Building hypervisor with Linux guest support..."
	cargo build --target aarch64-unknown-none --features linux_guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with Android-configured kernel..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS_ANDROID) \
	    -device loader,file=$(ANDROID_IMAGE),addr=0x48000000 \
	    -device loader,file=$(LINUX_DTB),addr=0x47000000 \
	    -device loader,file=$(ANDROID_INITRAMFS),addr=0x54000000 \
	    -device loader,file=$(ANDROID_DISK),addr=0x58000000
```

Note: Phase 1 reuses `linux_guest` feature, Linux DTB, initramfs, and disk image. Only the kernel Image comes from `guest/android/`. A separate Android DTB will be created in Phase 2 when bootargs diverge.

**Step 2: Update help target**

Add to the help echo list:
```makefile
	@echo "  run-android   - Build and run with Android-configured kernel (Phase 1)"
```

**Step 3: Commit**

```bash
git add Makefile
git commit -m "feat: add make run-android target for Android-configured kernel"
```

---

### Task 4: Boot Test and Debug

**Step 1: Run the Android-configured kernel**

```bash
make clean && make run-android
```

Wait for boot output. Expected success output:
```
[INIT] Initializing at EL2...
[INIT] Parsing host DTB...
...
[    0.000000] Booting Linux on physical CPU 0x0
[    0.000000] Linux version 6.6.126 (...)
...
[    0.xxx] smp: Brought up 1 node, 4 CPUs
...
[    0.xxx] virtio_blk virtio0: [vda] ...
...
/ #
```

**Step 2: If new sysreg traps appear**

The 6.6 kernel may access system registers not trapped by our current hypervisor config. If you see:
```
Unknown exception class: 0x18 (EC_MSR_MRS_TRAPPED)
```

Check `exception.rs` — the trap handler logs the ISS (instruction-specific syndrome) field which encodes the system register. Common registers that might need stubs:

| Register | Op0 | Op1 | CRn | CRm | Op2 | Action |
|----------|-----|-----|-----|-----|-----|--------|
| PMCR_EL0 | 3 | 3 | 9 | 12 | 0 | Return 0 (no PMU) |
| PMCCNTR_EL0 | 3 | 3 | 9 | 13 | 0 | Return 0 |

Note: Our exception handler already has a catch-all for PMU registers `(3, 3, 9, _, _)` and `(3, 0, 9, _, _)` that returns 0 for reads and ignores writes. Most traps should be handled automatically.

Add stubs to `handle_msr_mrs_trapped()` in `src/arch/aarch64/hypervisor/exception.rs` if needed — return 0 for reads, ignore writes, advance PC by 4.

**Step 3: If kernel panic on init**

If the kernel boots but panics looking for `/init`:
- Verify initramfs is loaded at correct address (0x54000000)
- Check DTB `linux,initrd-start`/`linux,initrd-end` match actual initramfs size
- Verify with: `ls -l guest/linux/initramfs.cpio.gz` (must be < 2MB)

**Step 4: Validate success criteria**

In the QEMU serial output, verify:
- [ ] `Linux version 6.6.126` (upstream kernel version string)
- [ ] `smp: Brought up 1 node, 4 CPUs`
- [ ] `virtio_blk virtio0: [vda]`
- [ ] BusyBox shell prompt `/ #`
- [ ] No `Unknown exception` flooding
- [ ] Binder driver loaded (check `dmesg | grep binder` in shell)

**Step 5: Commit any hypervisor fixes**

If sysreg stubs were added:
```bash
git add src/arch/aarch64/hypervisor/exception.rs
git commit -m "fix: add sysreg emulation stubs for 6.6 LTS kernel compatibility"
```

---

### Task 5: Update Documentation

**Files:**
- Modify: `CLAUDE.md` — add `make run-android` to build commands
- Modify: `DEVELOPMENT_PLAN.md` — update Android boot progress

**Step 1: Update CLAUDE.md build commands section**

Add after `make run-multi-vm`:
```
make run-android  # Build + boot Linux 6.6 LTS with Android config (Phase 1: BusyBox shell)
```

**Step 2: Commit**

```bash
git add CLAUDE.md DEVELOPMENT_PLAN.md
git commit -m "docs: add Android boot Phase 1 documentation"
```

---

## Phase 1 Summary

| Task | What | Est. Time |
|------|------|-----------|
| 1 | Create build script + config fragment | 15 min |
| 2 | Build kernel via Docker | 10-30 min (mostly waiting) |
| 3 | Add Makefile target | 5 min |
| 4 | Boot test + debug traps | 30-60 min |
| 5 | Update docs | 10 min |

**Total: ~1-1.5 hours**, mostly waiting for kernel compilation.

**Exit criteria:** Upstream Linux 6.6 LTS kernel with Android config (Binder IPC) boots to BusyBox shell with 4 vCPUs and virtio-blk on our hypervisor. `dmesg | grep binder` shows binder driver loaded.

**Phase 3 transition:** Switch from upstream kernel.org 6.6 to real AOSP kernel source with Clang/LLVM toolchain.
