# Android Boot Phase 1 — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot AOSP android14-6.6-lts kernel on the hypervisor with existing BusyBox initramfs, validating hypervisor compatibility with Android-configured kernel.

**Architecture:** Build AOSP common kernel via Docker (same pattern as existing `guest/linux/build-kernel.sh`), produce an arm64 `Image` file, and boot it with existing `make run-linux` infrastructure. No hypervisor code changes needed — this is a kernel swap. If new sysreg traps appear, add minimal emulation stubs.

**Tech Stack:** AOSP kernel source (repo/git), Docker for cross-compilation, existing QEMU/Makefile infrastructure.

---

### Task 1: Create Android Kernel Build Script

**Files:**
- Create: `guest/android/build-kernel.sh`
- Create: `guest/android/android-virt.config` (kernel config fragment)

**Step 1: Create kernel config fragment**

This fragment enables QEMU virt platform drivers on top of GKI defconfig.

```bash
# File: guest/android/android-virt.config
# Config fragment for AOSP android14-6.6 on QEMU virt
# Applied on top of gki_defconfig via scripts/kconfig/merge_config.sh

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

# Android-specific (needed for later phases, harmless to enable now)
CONFIG_ANDROID=y
CONFIG_ANDROID_BINDER_IPC=y
CONFIG_ANDROID_BINDERFS=y
```

Save to `guest/android/android-virt.config`.

**Step 2: Create the build script**

```bash
#!/bin/bash
# Build AOSP android14-6.6-lts kernel for QEMU virt + our hypervisor
# Usage: docker run --rm -v $PWD:/work -v $PWD/guest/android:/output debian:bookworm-slim bash /work/guest/android/build-kernel.sh
set -euo pipefail

BRANCH=common-android14-6.6-lts
NCPU=$(nproc)
export ARCH=arm64
export CROSS_COMPILE=aarch64-linux-gnu-

echo "=== Building AOSP kernel (${BRANCH}) for arm64 ==="
echo "=== Using ${NCPU} CPUs ==="

# Install build dependencies
apt-get update -qq
apt-get install -y -qq git gcc-aarch64-linux-gnu make bc flex bison libssl-dev libelf-dev wget python3 kmod cpio rsync 2>/dev/null

# Clone kernel source if not present
cd /build
if [ ! -d "common" ]; then
    echo ">>> Cloning AOSP common kernel (${BRANCH})..."
    git clone --depth=1 --branch ${BRANCH} \
        https://android.googlesource.com/kernel/common common
fi

cd common

# Start with GKI defconfig
echo ">>> Generating gki_defconfig..."
make ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- gki_defconfig

# Merge our virt platform config fragment
echo ">>> Merging android-virt.config..."
scripts/kconfig/merge_config.sh -m .config /output/android-virt.config

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
git commit -m "feat: add AOSP android14-6.6 kernel build script for QEMU virt"
```

---

### Task 2: Build the AOSP Kernel

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

Expected: `guest/android/Image` is an ARM64 Linux kernel Image, ~30-50MB.
The `file` command should show: `Linux kernel ARM64 boot executable Image`.

**Step 4: Add Image to .gitignore (don't commit binaries)**

Add to `.gitignore` (create if not exists):
```
guest/android/Image
```

---

### Task 3: Create Android DTB (Device Tree)

**Files:**
- Create: `guest/android/guest.dts`
- Output: `guest/android/guest.dtb`

**Step 1: Create DTS file**

Start from existing `guest/linux/guest.dts` but with Android-appropriate bootargs. The key difference is the kernel command line — Android uses `androidboot.*` parameters.

```dts
/dts-v1/;

/ {
	interrupt-parent = <0x8002>;
	#size-cells = <0x02>;
	#address-cells = <0x02>;
	compatible = "linux,dummy-virt";

	chosen {
		bootargs = "earlycon=pl011,0x09000000 console=ttyAMA0 earlyprintk loglevel=8 nokaslr rdinit=/init";
		stdout-path = "/pl011@9000000";
		linux,initrd-start = <0x00 0x54000000>;
		linux,initrd-end = <0x00 0x54200000>;
	};

	psci {
		migrate = <0xc4000005>;
		cpu_on = <0xc4000003>;
		cpu_off = <0x84000002>;
		cpu_suspend = <0xc4000001>;
		method = "hvc";
		compatible = "arm,psci-0.2\0arm,psci";
	};

	memory@48000000 {
		reg = <0x00 0x48000000 0x00 0x20000000>;
		device_type = "memory";
	};

	cpus {
		#size-cells = <0x00>;
		#address-cells = <0x01>;

		cpu@0 {
			reg = <0x00>;
			compatible = "arm,cortex-a53";
			device_type = "cpu";
			enable-method = "psci";
		};

		cpu@1 {
			reg = <0x01>;
			compatible = "arm,cortex-a53";
			device_type = "cpu";
			enable-method = "psci";
		};

		cpu@2 {
			reg = <0x02>;
			compatible = "arm,cortex-a53";
			device_type = "cpu";
			enable-method = "psci";
		};

		cpu@3 {
			reg = <0x03>;
			compatible = "arm,cortex-a53";
			device_type = "cpu";
			enable-method = "psci";
		};
	};

	timer {
		interrupts = <0x01 0x0d 0x104 0x01 0x0e 0x104 0x01 0x0b 0x104 0x01 0x0a 0x104>;
		always-on;
		compatible = "arm,armv8-timer\0arm,armv7-timer";
	};

	apb-pclk {
		phandle = <0x8000>;
		clock-output-names = "clk24mhz";
		clock-frequency = <0x16e3600>;
		#clock-cells = <0x00>;
		compatible = "fixed-clock";
	};

	pl011@9000000 {
		clock-names = "uartclk\0apb_pclk";
		clocks = <0x8000 0x8000>;
		interrupts = <0x00 0x01 0x04>;
		reg = <0x00 0x9000000 0x00 0x1000>;
		compatible = "arm,pl011\0arm,primecell";
	};

	virtio_mmio@a000000 {
		dma-coherent;
		interrupts = <0x00 0x10 0x01>;
		reg = <0x00 0xa000000 0x00 0x200>;
		compatible = "virtio,mmio";
	};

	virtio_mmio@a000200 {
		dma-coherent;
		interrupts = <0x00 0x11 0x01>;
		reg = <0x00 0xa000200 0x00 0x200>;
		compatible = "virtio,mmio";
	};

	intc@8000000 {
		phandle = <0x8002>;
		reg = <0x00 0x8000000 0x00 0x10000 0x00 0x80a0000 0x00 0xf60000>;
		#redistributor-regions = <0x01>;
		compatible = "arm,gic-v3";
		ranges;
		#size-cells = <0x02>;
		#address-cells = <0x02>;
		interrupt-controller;
		#interrupt-cells = <0x03>;
	};
};
```

Save to `guest/android/guest.dts`.

**Step 2: Compile DTB**

```bash
dtc -I dts -O dtb -o guest/android/guest.dtb guest/android/guest.dts
```

**Step 3: Commit**

```bash
git add guest/android/guest.dts guest/android/guest.dtb
git commit -m "feat: add Android guest DTB for QEMU virt"
```

---

### Task 4: Add Makefile Target for Android Guest

**Files:**
- Modify: `Makefile` (add `run-android` target)

**Step 1: Add Android guest paths and target**

Add after the `run-multi-vm` target (line ~118):

```makefile
# Android guest paths
ANDROID_IMAGE ?= guest/android/Image
ANDROID_DTB ?= guest/android/guest.dtb
ANDROID_INITRAMFS ?= guest/linux/initramfs.cpio.gz
ANDROID_DISK ?= guest/linux/disk.img

# Run hypervisor with Android kernel (Phase 1: BusyBox shell)
run-android:
	@echo "Building hypervisor with Linux guest support..."
	cargo build --target aarch64-unknown-none --features linux_guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with Android kernel..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(ANDROID_IMAGE),addr=0x48000000 \
	    -device loader,file=$(ANDROID_DTB),addr=0x47000000 \
	    -device loader,file=$(ANDROID_INITRAMFS),addr=0x54000000 \
	    -device loader,file=$(ANDROID_DISK),addr=0x58000000
```

Note: Phase 1 reuses `linux_guest` feature, Linux initramfs, and Linux disk image. Only the kernel Image and DTB come from `guest/android/`.

**Step 2: Update help target**

Add to the help echo list:
```makefile
	@echo "  run-android   - Build and run with AOSP kernel (Android Phase 1)"
```

**Step 3: Commit**

```bash
git add Makefile
git commit -m "feat: add make run-android target for AOSP kernel boot"
```

---

### Task 5: Boot Test and Debug

**Step 1: Run the Android kernel**

```bash
make clean && make run-android
```

Wait for boot output. Expected success output:
```
[INIT] Initializing at EL2...
[INIT] Parsing host DTB...
...
[    0.000000] Booting Linux on physical CPU 0x0
[    0.000000] Linux version 6.6.xxx-android14-... (...)
...
[    0.xxx] smp: Brought up 1 node, 4 CPUs
...
[    0.xxx] virtio_blk virtio0: [vda] ...
...
/ #
```

**Step 2: If new sysreg traps appear**

The AOSP kernel may access system registers not trapped by our current hypervisor config. If you see:
```
Unknown exception class: 0x18 (EC_MSR_MRS_TRAPPED)
```

Check `exception.rs` — the trap handler logs the ISS (instruction-specific syndrome) field which encodes the system register. Common Android kernel registers that might need stubs:

| Register | Op0 | Op1 | CRn | CRm | Op2 | Action |
|----------|-----|-----|-----|-----|-----|--------|
| PMCR_EL0 | 3 | 3 | 9 | 12 | 0 | Return 0 (no PMU) |
| PMCCNTR_EL0 | 3 | 3 | 9 | 13 | 0 | Return 0 |
| CNTV_CTL_EL0 | 3 | 3 | 14 | 3 | 1 | Passthrough (should not trap) |

Add stubs to `handle_msr_mrs_trapped()` in `src/arch/aarch64/hypervisor/exception.rs` if needed — return 0 for reads, ignore writes, advance PC by 4.

**Step 3: If kernel panic on init**

If the kernel boots but panics looking for `/init`:
- Verify initramfs is loaded at correct address (0x54000000)
- Check DTB `linux,initrd-start`/`linux,initrd-end` match
- Try adjusting initramfs-end if the AOSP kernel expects different size

**Step 4: Validate success criteria**

In the QEMU serial output, verify:
- [ ] `Linux version 6.6.xxx-android14-` (AOSP kernel version string)
- [ ] `smp: Brought up 1 node, 4 CPUs`
- [ ] `virtio_blk virtio0: [vda]`
- [ ] BusyBox shell prompt `/ #`
- [ ] No `Unknown exception` flooding

**Step 5: Commit any hypervisor fixes**

If sysreg stubs were added:
```bash
git add src/arch/aarch64/hypervisor/exception.rs
git commit -m "fix: add sysreg emulation stubs for AOSP kernel compatibility"
```

---

### Task 6: Update Documentation

**Files:**
- Modify: `CLAUDE.md` — add `make run-android` to build commands, note AOSP kernel support
- Modify: `DEVELOPMENT_PLAN.md` — update Android boot progress

**Step 1: Update CLAUDE.md build commands section**

Add after `make run-multi-vm`:
```
make run-android  # Build + boot AOSP android14-6.6 kernel (Phase 1: BusyBox shell)
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
| 2 | Build AOSP kernel via Docker | 10-30 min (mostly waiting) |
| 3 | Create Android DTB | 10 min |
| 4 | Add Makefile target | 5 min |
| 5 | Boot test + debug traps | 30-60 min |
| 6 | Update docs | 10 min |

**Total: ~1.5-2 hours**, mostly waiting for kernel compilation.

**Exit criteria:** AOSP android14-6.6 kernel boots to BusyBox shell with 4 vCPUs and virtio-blk on our hypervisor.
