# Android Boot on Hypervisor — Design Document

Date: 2026-02-19

## Goal

Boot full Android (AOSP) on the ARM64 hypervisor, progressing through 4 phases from kernel-only to complete Android system.

## Current State

- Linux 6.12.12 boots to BusyBox shell with 4 vCPUs, virtio-blk, virtio-net
- All existing infrastructure (GIC, UART, Stage-2, SMP, FF-A) works
- 29 test suites, 158 assertions passing

## Phase Overview

| Phase | Target | Kernel Source | Success Criteria |
|-------|--------|---------------|------------------|
| 1 | Android-configured kernel + BusyBox shell | Upstream kernel.org 6.6 LTS + Android config fragment (GCC) | `smp: Brought up 1 node, 4 CPUs`, binder driver loaded |
| 2 | Android minimal init | Same kernel | Android `/init` starts, mounts filesystems, prints to serial |
| 3 | Android system partition | AOSP common kernel (Clang/LLVM) | `/system` mounted, core services start |
| 4 | Full Android boot | AOSP common kernel | All services running, `adb shell` accessible |

## Phase 1: Upstream 6.6 LTS + Android Config + BusyBox Shell

### What

Build upstream Linux 6.6 LTS from kernel.org with Android-specific config options (Binder IPC, Binderfs) on top of arm64 `defconfig`. Reuse existing BusyBox initramfs. Validate hypervisor compatibility with Android-configured kernel.

### Why Upstream Kernel (Not AOSP Source)

AOSP GKI kernels use Clang/LLVM toolchain and Kleaf/Bazel build system. Using upstream kernel.org source with GCC:
1. Reuses the exact same Docker build pattern as our existing `guest/linux/build-kernel.sh`
2. Binder IPC and Binderfs are available in upstream 6.6 — no AOSP patches needed for Phase 1
3. Avoids Clang/LLVM toolchain setup complexity
4. Phase 3 will switch to real AOSP kernel when we need Android-specific patches

### Kernel Build

Source: `https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.6.126.tar.xz`

Config: arm64 `defconfig` + Android config fragment:
```
CONFIG_VIRTIO_BLK=y
CONFIG_VIRTIO_NET=y
CONFIG_VIRTIO_MMIO=y
CONFIG_ANDROID=y
CONFIG_ANDROID_BINDER_IPC=y
CONFIG_ANDROID_BINDERFS=y
CONFIG_DEBUG_INFO_BTF=n
```

Build via Docker (same pattern as existing Linux kernel build):
```
docker run --rm \
    -v /tmp/android-kernel-build:/build \
    -v $PWD/guest/android:/output \
    debian:bookworm-slim \
    bash /output/build-kernel.sh
```

Output: `Image` file (~20-30MB), drop-in replacement for current kernel.

### Hypervisor Changes

None. Existing `linux_guest` feature works — just swap the kernel Image file. Reuse existing Linux DTB (bootargs identical for Phase 1).

### Validation

- Boot to BusyBox shell prompt `/ #`
- `smp: Brought up 1 node, 4 CPUs`
- virtio-blk detected: `virtio_blk virtio0: [vda]`
- Binder driver loaded: `dmesg | grep binder`
- No new traps or exceptions vs current Linux kernel

## Phase 2: Android Minimal Init

### What

Boot same kernel with Android ramdisk containing `/init` (Android's init process). Get Android init to parse `init.rc`, mount basic filesystems, and print to serial console.

### New Hypervisor Device: PL031 RTC

Android init reads system time early in boot. Without an RTC, `date` fails and init may hang.

**PL031 register map** (ARM PrimeCell RTC, 4KB MMIO at `0x09010000`):
| Offset | Register | Read/Write | Description |
|--------|----------|------------|-------------|
| 0x000 | RTCDR | RO | Data register (seconds since epoch) |
| 0x004 | RTCMR | RW | Match register (alarm) |
| 0x008 | RTCLR | WO | Load register (set time) |
| 0x00C | RTCCR | RW | Control register (bit 0 = enable) |
| 0x010 | RTCIMSC | RW | Interrupt mask |
| 0x014 | RTCRIS | RO | Raw interrupt status |
| 0x018 | RTCMIS | RO | Masked interrupt status |
| 0x01C | RTCICR | WO | Interrupt clear |
| 0xFE0-0xFFC | PeriphID/PrimeCellID | RO | ID registers (same pattern as PL011 UART) |

**Implementation**: Trap-and-emulate (Stage-2 unmapped, 4KB page at `0x09010000`). Shadow state holds current time. RTCDR returns host monotonic counter converted to seconds. No interrupt needed for initial boot.

Estimated: ~150 LOC in `src/devices/pl031.rs`.

### Android DTB

Phase 2 creates a separate `guest/android/guest.dts` with:
- `androidboot.hardware=virt` in bootargs
- PL031 RTC node at `0x09010000`
- Adjusted memory size if needed

### RAM Increase

Current: 1GB QEMU, 512MB guest.
Required: 2GB QEMU, 1GB+ guest.

Changes:
- Makefile: `-m 2G` (already set in Phase 1's `QEMU_FLAGS_ANDROID`)
- `platform.rs`: adjust `GUEST_MEMORY_SIZE` for android feature
- Stage-2: map additional RAM region

### Android Ramdisk

Source: Extract from AOSP build (`ramdisk.img` from `aosp_cf_arm64_phone-userdebug` build) or build minimal Android init from source.

Simpler approach: build a minimal ramdisk with just Android `/init` binary + `init.rc` that mounts proc/sys and prints to console. This validates the init path without full AOSP complexity.

### Validation

- Android init starts: `init: Loading /init.rc`
- Basic filesystem mounts succeed
- Serial console output from init

## Phase 3: Android System Partition (Switch to AOSP Kernel)

### What

Mount a real Android `/system` partition via virtio-blk. Start core Android services (servicemanager, logd, etc).

### AOSP Kernel Switch

Phase 3 switches from upstream kernel.org to real AOSP kernel source:
- Source: `https://android.googlesource.com/kernel/common` branch `common-android15-6.6`
- Build: Docker with Clang/LLVM (`LLVM=1`), or `tools/bazel run //common:kernel_aarch64_dist`
- This gives us GKI with Android-specific patches (energy-aware scheduling, cgroup v2 tweaks, etc.)

### Multiple virtio-blk Devices

Current: 1 virtio-blk at slot 0 (`0x0a000000`, INTID 48).
Required: 2-3 virtio-blk devices for system/vendor/userdata.

Use existing `platform::virtio_slot(n)` infrastructure:
- Slot 0: `0x0a000000` (INTID 48) — system.img
- Slot 1: `0x0a000200` (INTID 49) — currently virtio-net, needs rethink
- Slot 2: `0x0a000400` (INTID 50) — vendor.img or userdata.img

**Device slot conflict**: Slot 1 is currently used by virtio-net. Options:
1. Move virtio-net to slot 3 (`0x0a000600`, INTID 51)
2. Use `android_guest` feature flag to configure different device layout
3. Increase MAX_DEVICES from 8 to 12

Recommended: Option 2 — `android_guest` feature flag with Android-specific device layout.

### Kernel Command Line

```
root=/dev/vda rootfstype=ext4 ro init=/init console=ttyAMA0 androidboot.hardware=virt
```

Passed via DTB `/chosen/bootargs` node.

### Validation

- `/system` mounts from virtio-blk
- `servicemanager` starts
- `logd` starts, `logcat` works

## Phase 4: Full Android Boot

### What

Complete AOSP boot with all services. SELinux permissive (initially). `adb shell` accessible via virtio-net or virtio-console.

### Additional Requirements

- **Binder IPC**: Kernel config `CONFIG_ANDROID_BINDER_IPC=y` (kernel-side, no hypervisor change)
- **SELinux**: Initially `androidboot.selinux=permissive` in kernel cmdline
- **Properties**: `androidboot.hardware=virt` tells init which `.rc` files to load
- **Display**: Not required for headless boot. `ANDROID_NO_DISPLAY=1` or virtio-console for shell
- **ADB over network**: Existing virtio-net + auto-IP can route ADB traffic

### AOSP Build Target

```bash
repo init -u https://android.googlesource.com/platform/manifest -b android-14.0.0_r1
repo sync -j$(nproc)
source build/envsetup.sh
lunch aosp_cf_arm64_phone-userdebug
make -j$(nproc)
```

Output: `system.img`, `vendor.img`, `userdata.img`, `ramdisk.img`, `kernel`

Note: Cuttlefish target (`aosp_cf_arm64_phone`) produces images for virtio-PCI hardware. Adapting these for our virtio-mmio hypervisor will require device tree and fstab modifications.

### Validation

- All Android services start (check `getprop sys.boot_completed`)
- `adb shell` works over virtio-net
- Basic apps launchable (headless verification via `am start`)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| 6.6 LTS kernel needs new sysreg traps | Low | Medium | Existing PMU catch-all handles most; add stubs incrementally |
| Android init needs unimplemented device | Medium | Medium | Stub devices, return zeros |
| Binder not available in upstream 6.6 | Very Low | High | Binder has been upstream since 3.19; verified in 6.6 |
| AOSP build environment too large (Phase 3+) | Low | Medium | Use Docker, cloud build |
| Performance too slow (no KVM) | High | Low | TCG is slow but functional; focus on correctness |
| SELinux blocks boot (Phase 4) | Medium | Medium | Use permissive mode |
| Cuttlefish images need adaptation (Phase 4) | High | Medium | Modify fstab + DTB for virtio-mmio |

## Architecture Diagram

```
QEMU Host (x86_64 or aarch64)
  └─ Hypervisor @ EL2
       ├─ Stage-2 Page Tables (identity map)
       ├─ GIC Virtualization (GICD write-through, GICR trap)
       ├─ PL011 UART (trap-and-emulate)
       ├─ PL031 RTC (trap-and-emulate) [NEW - Phase 2]
       ├─ VirtIO-blk #0 (system.img) [Phase 3]
       ├─ VirtIO-blk #1 (vendor.img) [Phase 3]
       ├─ VirtIO-blk #2 (userdata.img) [Phase 4]
       ├─ VirtIO-net (auto-IP, ADB) [existing]
       ├─ FF-A Proxy (stub SPMC)
       └─ SMP Scheduler (4 vCPUs)
            └─ Android Guest @ EL1
                 ├─ Phase 1: kernel.org 6.6 LTS + Android config (GCC)
                 ├─ Phase 3+: AOSP common kernel android15-6.6 (Clang)
                 ├─ Android Init (/init + init.rc) [Phase 2+]
                 ├─ /system (ext4 on virtio-blk) [Phase 3+]
                 ├─ /vendor (ext4 on virtio-blk) [Phase 3+]
                 └─ Android Services (servicemanager, logd, ...) [Phase 3+]
```

## Implementation Priority

Phase 1 is the immediate next step. It requires:
1. Create build script + Android config fragment for upstream 6.6 LTS
2. Build kernel via Docker (same pattern as existing Linux build)
3. Add `make run-android` Makefile target (reuses Linux DTB + initramfs)
4. Test boot and debug any new traps
5. Verify binder driver loads (`dmesg | grep binder`)

Estimated effort: 1-2 hours for Phase 1 (mostly kernel compilation time).

Phase 3 will switch to AOSP kernel source with Clang/LLVM when Android-specific patches become necessary.
