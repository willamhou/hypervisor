# Building the Guest Kernel

This guide explains how to build a Linux kernel, initramfs, DTB, and disk image for the hypervisor's Linux guest.

## Prerequisites

- Docker (for cross-compilation environment)
- `dtc` (device tree compiler) — `apt install device-tree-compiler`
- `qemu-system-aarch64` — for DTB extraction and running

## Quick Start

All guest artifacts are pre-built in `guest/linux/`. To rebuild from scratch:

```bash
# 1. Build the kernel
docker run --rm -v $(pwd)/guest/linux:/output -v $(pwd)/guest/linux:/build \
    debian:bookworm-slim bash /build/build-kernel.sh

# 2. Build the initramfs (see below)

# 3. Generate the DTB (see below)

# 4. Create the disk image (see below)

# 5. Boot
make run-linux
```

## Kernel Build

The kernel is built inside a Docker container with cross-compilation tools.

### Build Script: `guest/linux/build-kernel.sh`

```bash
docker run --rm \
    -v $(pwd)/guest/linux:/output \
    -v $(pwd)/guest/linux:/build \
    debian:bookworm-slim bash /build/build-kernel.sh
```

The script:
1. Downloads Linux 6.12.12 source from kernel.org
2. Runs `make defconfig` for arm64
3. Force-enables required CONFIG options
4. Builds `arch/arm64/boot/Image`

### Required CONFIG Options

The arm64 `defconfig` already includes most of what we need. The script explicitly enables:

| Config | Purpose |
|--------|---------|
| `CONFIG_VIRTIO_MMIO=y` | Virtio-mmio transport (NOT built-in on Debian kernels!) |
| `CONFIG_VIRTIO_BLK=y` | Virtio block device |
| `CONFIG_VIRTIO=y` | Core virtio support |
| `CONFIG_BLK_DEV=y` | Block device layer |
| `CONFIG_BLK_DEV_INITRD=y` | Initial ramdisk support |
| `CONFIG_RD_GZIP=y` | Gzip-compressed initramfs |
| `CONFIG_SERIAL_AMBA_PL011=y` | PL011 UART driver |
| `CONFIG_SERIAL_AMBA_PL011_CONSOLE=y` | Console on PL011 |
| `CONFIG_SMP=y` | Symmetric multiprocessing |
| `CONFIG_DEVTMPFS=y` | Automatic device node creation |
| `CONFIG_DEVTMPFS_MOUNT=y` | Auto-mount devtmpfs |

**Why not use Debian's kernel?** The Debian arm64 kernel does NOT include `CONFIG_VIRTIO_MMIO` as built-in — it's either missing or compiled as a module. Since we have no module loading infrastructure, we need a custom kernel with everything built-in.

### Output

- `guest/linux/Image` — ARM64 Linux kernel (~25MB)

## Initramfs

The initramfs provides a minimal BusyBox userspace.

### Building from Scratch

```bash
# Get a static BusyBox binary for arm64
# Option 1: Build from source
# Option 2: Use a pre-built static binary

# Create initramfs directory structure
mkdir -p initramfs/{bin,sbin,etc,proc,sys,dev,tmp}

# Copy BusyBox and create symlinks
cp busybox-aarch64-static initramfs/bin/busybox
cd initramfs/bin
for cmd in sh ls cat echo mount mkdir mknod; do
    ln -s busybox $cmd
done
cd ../..

# Create /init script
cat > initramfs/init << 'EOF'
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sys /sys
mount -t devtmpfs dev /dev
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts

echo "Welcome to Hypervisor Linux Guest"
exec /bin/sh
EOF
chmod +x initramfs/init

# Pack as cpio.gz
cd initramfs
find . | cpio -o -H newc | gzip > ../initramfs.cpio.gz
cd ..
```

### Output

- `guest/linux/initramfs.cpio.gz` — Compressed initramfs (~2MB)

## Device Tree (DTB)

The DTB tells Linux about the virtual hardware the hypervisor provides.

### Generating from QEMU

The easiest way to get a starting DTB is to dump it from QEMU:

```bash
qemu-system-aarch64 \
    -machine virt,virtualization=on,gic-version=3,dumpdtb=virt.dtb \
    -cpu max -smp 4 -m 1G -nographic
```

Then decompile, modify, and recompile:

```bash
dtc -I dtb -O dts virt.dtb > guest.dts
# Edit guest.dts (see below)
dtc -I dts -O dtb guest.dts > guest.dtb
```

### Required DTB Modifications

The key modifications from QEMU's default DTB:

#### 1. Memory Node
Change to match hypervisor's guest memory layout:
```dts
memory@48000000 {
    reg = <0x00 0x48000000 0x00 0x20000000>;  /* 512MB at 0x48000000 */
    device_type = "memory";
};
```

#### 2. Chosen Node
Set kernel command line, console, and initramfs location:
```dts
chosen {
    bootargs = "earlycon=pl011,0x09000000 console=ttyAMA0 earlyprintk loglevel=8 nokaslr rdinit=/init";
    stdout-path = "/pl011@9000000";
    linux,initrd-start = <0x00 0x54000000>;
    linux,initrd-end = <0x00 0x54200000>;
};
```

- `nokaslr`: Disable kernel address space randomization (simplifies debugging)
- `rdinit=/init`: Use initramfs's `/init` script
- Initramfs at 0x54000000 (matches Makefile `-device loader` address)

#### 3. CPU Nodes
4 CPUs with PSCI enable-method:
```dts
cpus {
    cpu@0 { reg = <0x00>; enable-method = "psci"; compatible = "arm,cortex-a53"; };
    cpu@1 { reg = <0x01>; enable-method = "psci"; compatible = "arm,cortex-a53"; };
    cpu@2 { reg = <0x02>; enable-method = "psci"; compatible = "arm,cortex-a53"; };
    cpu@3 { reg = <0x03>; enable-method = "psci"; compatible = "arm,cortex-a53"; };
};
```

#### 4. PSCI Node
```dts
psci {
    method = "hvc";
    cpu_on = <0xc4000003>;     /* PSCI_CPU_ON_64 */
    cpu_off = <0x84000002>;
    cpu_suspend = <0xc4000001>;
    compatible = "arm,psci-0.2", "arm,psci";
};
```

#### 5. Virtio-mmio Node
```dts
virtio_mmio@a000000 {
    compatible = "virtio,mmio";
    reg = <0x00 0xa000000 0x00 0x200>;
    interrupts = <0x00 0x10 0x01>;  /* SPI 16 = INTID 48 */
    dma-coherent;
};
```

#### 6. GIC Node
```dts
intc@8000000 {
    compatible = "arm,gic-v3";
    reg = <0x00 0x8000000 0x00 0x10000     /* GICD */
           0x00 0x80a0000 0x00 0xf60000>;  /* GICR */
    #interrupt-cells = <0x03>;
    interrupt-controller;
};
```

### Output

- `guest/linux/guest.dts` — Device tree source
- `guest/linux/guest.dtb` — Compiled device tree blob

## Disk Image

The virtio-blk device needs a backing disk image.

```bash
# Create a 2MB ext4 disk image
dd if=/dev/zero of=disk.img bs=1M count=2
mkfs.ext4 -F disk.img
```

The disk image is loaded by QEMU at 0x58000000 via `-device loader`.

### Output

- `guest/linux/disk.img` — Raw disk image (~2MB)

## QEMU Invocation

The `make run-linux` target uses:

```bash
qemu-system-aarch64 \
    -machine virt,virtualization=on,gic-version=3 \
    -cpu max -smp 4 -m 1G -nographic \
    -kernel target/aarch64-unknown-none/debug/hypervisor \
    -device loader,file=guest/linux/Image,addr=0x48000000 \
    -device loader,file=guest/linux/guest.dtb,addr=0x47000000 \
    -device loader,file=guest/linux/initramfs.cpio.gz,addr=0x54000000 \
    -device loader,file=guest/linux/disk.img,addr=0x58000000
```

The `-device loader` entries load files into guest physical memory at fixed addresses. The hypervisor's linker places itself at 0x40000000, so there's no overlap.

## File Summary

| File | Address | Purpose |
|------|---------|---------|
| `Image` | 0x48000000 | Linux kernel |
| `guest.dtb` | 0x47000000 | Device tree |
| `initramfs.cpio.gz` | 0x54000000 | BusyBox rootfs |
| `disk.img` | 0x58000000 | Virtio-blk backing |

## Troubleshooting

### "Wrong magic value 0x00000000" for virtio-mmio
The hypervisor must use HPFAR_EL2 (not FAR_EL2) to compute the IPA for MMIO traps. See `docs/design/exception-handling.md`.

### Kernel panics during secondary CPU boot
Check that PSCI CPU_ON sets x0=context_id, SPSR has DAIF masked, and sctlr_el1 has MMU off. See `docs/design/smp-scheduling.md`.

### No virtio devices detected
Ensure the kernel has `CONFIG_VIRTIO_MMIO=y` built-in (not as module). The Debian arm64 kernel lacks this.
