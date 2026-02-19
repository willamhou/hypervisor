# Android Boot Phase 2 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Boot the Android-configured kernel (6.6.126) with PL031 RTC emulation, Android DTB, 1GB guest RAM, and a minimal C `/init` that mimics Android init behavior.

**Architecture:** Add PL031 RTC as a new trap-and-emulate device following the PL011 pattern. Create an Android-specific DTB with PL031 node and 1GB memory. Build a minimal C `/init` binary (statically compiled aarch64) that mounts filesystems, validates binder/RTC, and drops to shell. Increase guest RAM from 512MB to 1GB across all Linux targets.

**Tech Stack:** Rust (no_std), ARM64 assembly, C (cross-compiled aarch64-linux-gnu-gcc -static), DTS/DTC, Docker, CPIO/gzip

---

## Task 1: PL031 RTC Unit Tests

**Files:**
- Create: `tests/test_pl031.rs`
- Modify: `tests/mod.rs:1-67` — add module + re-export
- Modify: `src/main.rs:163-167` — add test invocation

**Step 1: Write the test file**

Create `tests/test_pl031.rs` with 4 test cases that will fail until we implement the PL031 device:

```rust
//! PL031 RTC emulation tests

use hypervisor::devices::pl031::VirtualPl031;
use hypervisor::devices::MmioDevice;

pub fn run_pl031_test() {
    hypervisor::uart_puts(b"\n=== Test: PL031 RTC Emulation ===\n");
    let mut pass: u64 = 0;
    let mut fail: u64 = 0;

    let mut rtc = VirtualPl031::new();

    // Test 1: RTCDR returns non-zero (time in seconds from counter)
    {
        let val = rtc.read(0x000, 4); // RTCDR
        if val.is_some() && val.unwrap() > 0 {
            hypervisor::uart_puts(b"  [PASS] RTCDR returns non-zero time\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] RTCDR should return non-zero time\n");
            fail += 1;
        }
    }

    // Test 2: Write RTCLR (load register), read back via RTCDR
    {
        rtc.write(0x008, 1000, 4); // RTCLR = 1000
        // RTCCR must be enabled for RTCDR to advance from loaded value
        rtc.write(0x00C, 1, 4); // RTCCR enable
        let val = rtc.read(0x000, 4).unwrap();
        if val >= 1000 {
            hypervisor::uart_puts(b"  [PASS] RTCLR write + RTCDR readback\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] RTCLR write + RTCDR readback\n");
            fail += 1;
        }
    }

    // Test 3: PeriphID registers match PL031
    {
        let id0 = rtc.read(0xFE0, 4).unwrap();
        let id1 = rtc.read(0xFE4, 4).unwrap();
        let id2 = rtc.read(0xFE8, 4).unwrap();
        let pcell0 = rtc.read(0xFF0, 4).unwrap();
        let pcell1 = rtc.read(0xFF4, 4).unwrap();
        // PL031: PartNumber=0x031, Designer=0x41 (ARM)
        // PeriphID0=0x31, PeriphID1=0x10, PeriphID2=0x04
        // PrimeCellID: 0x0D, 0xF0, 0x05, 0xB1
        if id0 == 0x31 && id1 == 0x10 && id2 == 0x04
            && pcell0 == 0x0D && pcell1 == 0xF0
        {
            hypervisor::uart_puts(b"  [PASS] PeriphID/PrimeCellID correct\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] PeriphID/PrimeCellID mismatch\n");
            fail += 1;
        }
    }

    // Test 4: Unknown offset returns 0
    {
        let val = rtc.read(0x100, 4).unwrap();
        if val == 0 {
            hypervisor::uart_puts(b"  [PASS] Unknown offset returns 0\n");
            pass += 1;
        } else {
            hypervisor::uart_puts(b"  [FAIL] Unknown offset should return 0\n");
            fail += 1;
        }
    }

    hypervisor::uart_puts(b"  Results: ");
    hypervisor::uart_put_u64(pass);
    hypervisor::uart_puts(b" passed, ");
    hypervisor::uart_put_u64(fail);
    hypervisor::uart_puts(b" failed\n");
    assert!(fail == 0, "PL031 RTC tests failed");
}
```

**Step 2: Register test in tests/mod.rs**

Add to `tests/mod.rs` after line 34 (`pub mod test_page_ownership;`):

```rust
pub mod test_pl031;
```

Add to re-exports after line 67 (`pub use test_page_ownership::run_page_ownership_test;`):

```rust
pub use test_pl031::run_pl031_test;
```

**Step 3: Wire test into main.rs**

Add after `tests::run_page_ownership_test();` (line 164) and before `tests::run_ffa_test();` (line 167):

```rust
    // Run the PL031 RTC test
    tests::run_pl031_test();
```

**Step 4: Verify tests fail to compile**

Run: `make clean && make 2>&1 | head -20`
Expected: Compilation error — `hypervisor::devices::pl031` module doesn't exist yet.

---

## Task 2: PL031 RTC Device Implementation

**Files:**
- Create: `src/devices/pl031.rs`
- Modify: `src/devices/mod.rs:1-105` — add module, Device variant, match arms

**Step 1: Create the PL031 emulator**

Create `src/devices/pl031.rs`:

```rust
//! Virtual PL031 RTC (ARM PrimeCell Real Time Clock)
//!
//! Trap-and-emulate PL031 with:
//! - RTCDR: returns monotonic counter converted to seconds
//! - RTCLR: sets the RTC base time
//! - RTCCR: control register (bit 0 = start/enable)
//! - RTCMR: match register (alarm, stubbed)
//! - Interrupt registers: stubbed (no interrupt needed for boot)
//! - PrimeCell ID registers for Linux rtc-pl031.c probe

use crate::devices::MmioDevice;

/// PL031 base address (QEMU virt convention)
pub const PL031_BASE: u64 = 0x0901_0000;
const PL031_SIZE: u64 = 0x1000;

// ── Register offsets ────────────────────────────────────────────────
const RTCDR: u64   = 0x000; // Data Register (RO) — current time in seconds
const RTCMR: u64   = 0x004; // Match Register (RW) — alarm
const RTCLR: u64   = 0x008; // Load Register (WO) — set time
const RTCCR: u64   = 0x00C; // Control Register (RW) — bit 0 = enable
const RTCIMSC: u64 = 0x010; // Interrupt Mask Set/Clear
const RTCRIS: u64  = 0x014; // Raw Interrupt Status
const RTCMIS: u64  = 0x018; // Masked Interrupt Status
const RTCICR: u64  = 0x01C; // Interrupt Clear

// PL031 Peripheral ID registers (read by Linux amba bus during probe)
const PERIPHID0: u64 = 0xFE0;
const PERIPHID1: u64 = 0xFE4;
const PERIPHID2: u64 = 0xFE8;
const PERIPHID3: u64 = 0xFEC;
const PCELLID0: u64  = 0xFF0;
const PCELLID1: u64  = 0xFF4;
const PCELLID2: u64  = 0xFF8;
const PCELLID3: u64  = 0xFFC;

/// Read the ARM generic timer counter frequency (CNTFRQ_EL0).
fn counter_frequency() -> u64 {
    let freq: u64;
    unsafe { core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq); }
    freq
}

/// Read the ARM generic timer virtual counter (CNTVCT_EL0).
fn counter_value() -> u64 {
    let count: u64;
    unsafe { core::arch::asm!("mrs {}, cntvct_el0", out(reg) count); }
    count
}

/// Virtual PL031 RTC device.
pub struct VirtualPl031 {
    /// Base time loaded via RTCLR (seconds since epoch)
    load_value: u32,
    /// Counter snapshot when RTCLR was last written
    load_counter: u64,
    /// Match register (alarm — not used during boot)
    match_value: u32,
    /// Control register (bit 0 = RTC started)
    control: u32,
    /// Interrupt mask
    imsc: u32,
    /// Raw interrupt status
    ris: u32,
}

impl VirtualPl031 {
    pub fn new() -> Self {
        Self {
            load_value: 0,
            load_counter: counter_value(),
            match_value: 0,
            control: 1, // RTC enabled by default (QEMU behavior)
            imsc: 0,
            ris: 0,
        }
    }

    /// Current RTC time in seconds.
    /// When control bit 0 is set, time advances from load_value.
    fn current_time(&self) -> u32 {
        if self.control & 1 == 0 {
            return self.load_value;
        }
        let freq = counter_frequency();
        if freq == 0 {
            return self.load_value;
        }
        let elapsed_ticks = counter_value().wrapping_sub(self.load_counter);
        let elapsed_secs = elapsed_ticks / freq;
        self.load_value.wrapping_add(elapsed_secs as u32)
    }
}

impl MmioDevice for VirtualPl031 {
    fn read(&mut self, offset: u64, _size: u8) -> Option<u64> {
        let value = match offset {
            RTCDR  => self.current_time() as u64,
            RTCMR  => self.match_value as u64,
            RTCCR  => self.control as u64,
            RTCIMSC => self.imsc as u64,
            RTCRIS  => self.ris as u64,
            RTCMIS  => (self.ris & self.imsc) as u64,

            // PL031 Peripheral ID (required for Linux rtc-pl031.c probe)
            // Part number = 0x031, Designer = 0x41 (ARM), Revision = 0
            PERIPHID0 => 0x31,
            PERIPHID1 => 0x10, // bits [7:4]=designer[3:0]=0x1, bits [3:0]=part[11:8]=0x0
            PERIPHID2 => 0x04, // bits [7:4]=revision=0, bits [3:0]=designer[7:4]=0x4
            PERIPHID3 => 0x00,
            PCELLID0  => 0x0D, // PrimeCell ID (same for all PrimeCell peripherals)
            PCELLID1  => 0xF0,
            PCELLID2  => 0x05,
            PCELLID3  => 0xB1,

            _ => 0,
        };
        Some(value)
    }

    fn write(&mut self, offset: u64, value: u64, _size: u8) -> bool {
        match offset {
            RTCMR => {
                self.match_value = value as u32;
                true
            }
            RTCLR => {
                self.load_value = value as u32;
                self.load_counter = counter_value();
                true
            }
            RTCCR => {
                self.control = (value & 1) as u32;
                true
            }
            RTCIMSC => {
                self.imsc = (value & 1) as u32;
                true
            }
            RTCICR => {
                self.ris &= !(value as u32);
                true
            }
            _ => true, // Unknown — accept silently
        }
    }

    fn base_address(&self) -> u64 { PL031_BASE }
    fn size(&self) -> u64 { PL031_SIZE }
}
```

**Step 2: Add PL031 to Device enum and DeviceManager**

In `src/devices/mod.rs`, add module declaration after line 8 (`pub mod virtio;`):

```rust
pub mod pl031;
```

Add variant to `Device` enum (after `VirtioNet` variant, line 42):

```rust
    Pl031(pl031::VirtualPl031),
```

Add match arms to all 6 `Device` impl methods. For each method (`read`, `write`, `base_address`, `size`, `pending_irq`, `ack_irq`), add after the `VirtioNet` arm:

```rust
            Device::Pl031(d) => d.read(offset, size),      // in read()
            Device::Pl031(d) => d.write(offset, value, size), // in write()
            Device::Pl031(d) => d.base_address(),           // in base_address()
            Device::Pl031(d) => d.size(),                   // in size()
            Device::Pl031(d) => d.pending_irq(),            // in pending_irq()
            Device::Pl031(d) => d.ack_irq(),                // in ack_irq()
```

**Step 3: Build and run tests**

Run: `make clean && make run 2>&1 | grep -E "PL031|test_pl031|PASS|FAIL"`
Expected: All 4 PL031 tests pass. All 30 test suites pass (29 existing + 1 new).

**Step 4: Commit**

```bash
git add src/devices/pl031.rs src/devices/mod.rs tests/test_pl031.rs tests/mod.rs src/main.rs
git commit -m "feat: add PL031 RTC trap-and-emulate device"
```

---

## Task 3: Register PL031 in VM Device Setup

**Files:**
- Modify: `src/vm.rs:66-84` — register PL031 device

**Step 1: Add PL031 registration**

In `src/vm.rs`, inside `Vm::new()`, add after the GICR registration block (after line 83):

```rust
        crate::global::DEVICES[id].register_device(crate::devices::Device::Pl031(
            crate::devices::pl031::VirtualPl031::new(),
        ));
```

This is unconditional (not gated by feature flags) — PL031 is lightweight and useful for all guests. The `0x09010000` address is never mapped in Stage-2, so MMIO accesses naturally trap to the hypervisor.

**Step 2: Build and verify no regression**

Run: `make clean && make run 2>&1 | grep -E "suites|FAIL|PL031"`
Expected: All 30 test suites pass, 0 failures.

**Step 3: Commit**

```bash
git add src/vm.rs
git commit -m "feat: register PL031 RTC in VM device manager"
```

---

## Task 4: Increase Guest RAM to 1GB

**Files:**
- Modify: `src/platform.rs:24` — `LINUX_MEM_SIZE` 512MB → 1GB
- Modify: `guest/linux/guest.dts:25-28` — memory size → 1GB
- Modify: `Makefile:19` — `-m 1G` → `-m 2G`

**Step 1: Update platform constant**

In `src/platform.rs`, change line 24:

```rust
pub const LINUX_MEM_SIZE: u64 = 1024 * 1024 * 1024;
```

**Step 2: Update Linux DTB memory size**

In `guest/linux/guest.dts`, change the memory node (line 25-28):

```dts
	memory@48000000 {
		reg = <0x00 0x48000000 0x00 0x40000000>;
		device_type = "memory";
	};
```

(`0x40000000` = 1GB, was `0x20000000` = 512MB)

**Step 3: Update QEMU RAM for standard targets**

In `Makefile`, line 19, change:

```makefile
              -m 2G \
```

(was `-m 1G`)

**Step 4: Recompile DTB**

Run: `dtc -I dts -O dtb -o guest/linux/guest.dtb guest/linux/guest.dts`

**Step 5: Build and test**

Run: `make clean && make run 2>&1 | grep -E "suites|FAIL"`
Expected: All 30 test suites pass.

Run: `make run-linux 2>&1 | head -80`
Expected: Linux boots, `Memory: ... available` shows ~1GB, shell prompt appears.

**Step 6: Commit**

```bash
git add src/platform.rs guest/linux/guest.dts guest/linux/guest.dtb Makefile
git commit -m "feat: increase guest RAM from 512MB to 1GB"
```

---

## Task 5: Create Android DTB

**Files:**
- Create: `guest/android/guest.dts`

**Step 1: Create Android DTB source**

Create `guest/android/guest.dts` based on `guest/linux/guest.dts` with these changes:
- Add `androidboot.hardware=virt` to bootargs
- Change `rdinit=/init` to `init=/init` (Android init is a binary, not a shell script)
- Add PL031 RTC node
- Memory: 1GB (same as updated Linux DTB)

```dts
/dts-v1/;

/ {
	interrupt-parent = <0x8002>;
	#size-cells = <0x02>;
	#address-cells = <0x02>;
	compatible = "linux,dummy-virt";

	chosen {
		bootargs = "earlycon=pl011,0x09000000 console=ttyAMA0 earlyprintk loglevel=8 nokaslr init=/init androidboot.hardware=virt";
		stdout-path = "/pl011@9000000";
		linux,initrd-start = <0x00 0x54000000>;
		linux,initrd-end = <0x00 0x54400000>;
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
		reg = <0x00 0x48000000 0x00 0x40000000>;
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

	pl031@9010000 {
		clock-names = "apb_pclk";
		clocks = <0x8000>;
		interrupts = <0x00 0x02 0x04>;
		reg = <0x00 0x9010000 0x00 0x1000>;
		compatible = "arm,pl031\0arm,primecell";
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

**Step 2: Compile DTB**

Run: `dtc -I dts -O dtb -o guest/android/guest.dtb guest/android/guest.dts`

**Step 3: Commit**

```bash
git add guest/android/guest.dts guest/android/guest.dtb
git commit -m "feat: add Android DTB with PL031 RTC and 1GB RAM"
```

---

## Task 6: Minimal C /init Binary

**Files:**
- Create: `guest/android/init.c`
- Create: `guest/android/init.rc`

**Step 1: Write init.c**

Create `guest/android/init.c` — a minimal `/init` (PID 1) that mimics Android init behavior:

```c
/*
 * Minimal Android-style /init for hypervisor Phase 2 validation.
 *
 * Validates: filesystem mounts, binder support, PL031 RTC, system info.
 * Drops to BusyBox shell for interactive debugging.
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/reboot.h>
#include <fcntl.h>
#include <time.h>
#include <errno.h>

static void print_banner(void) {
    printf("\n");
    printf("============================================\n");
    printf("  Android Minimal Init (Phase 2)\n");
    printf("  Hypervisor Guest - PID %d\n", getpid());
    printf("============================================\n\n");
}

static int do_mount(const char *type, const char *target) {
    mkdir(target, 0755);
    if (mount(type, target, type, 0, NULL) != 0) {
        printf("[init] WARN: mount %s on %s: %s\n", type, target, strerror(errno));
        return -1;
    }
    printf("[init] Mounted %s on %s\n", type, target);
    return 0;
}

static void mount_filesystems(void) {
    printf("[init] Mounting filesystems...\n");
    do_mount("proc", "/proc");
    do_mount("sysfs", "/sys");
    do_mount("devtmpfs", "/dev");
    mkdir("/dev/pts", 0755);
    mount("devpts", "/dev/pts", "devpts", 0, NULL);
    mkdir("/tmp", 01777);
    mount("tmpfs", "/tmp", "tmpfs", 0, "size=64m");
}

static void check_binder(void) {
    printf("[init] Checking binder support...\n");

    /* Check if binder filesystem type is registered */
    FILE *f = fopen("/proc/filesystems", "r");
    if (!f) {
        printf("[init] WARN: Cannot read /proc/filesystems\n");
        return;
    }
    char line[128];
    int found = 0;
    while (fgets(line, sizeof(line), f)) {
        if (strstr(line, "binder")) {
            found = 1;
            break;
        }
    }
    fclose(f);

    if (found) {
        printf("[init] OK: binder filesystem type registered\n");
        /* Try to mount binderfs */
        mkdir("/dev/binderfs", 0755);
        if (mount("binder", "/dev/binderfs", "binder", 0, NULL) == 0) {
            printf("[init] OK: binderfs mounted at /dev/binderfs\n");
        } else {
            printf("[init] WARN: binderfs mount failed: %s\n", strerror(errno));
        }
    } else {
        printf("[init] WARN: binder not found in /proc/filesystems\n");
    }
}

static void check_rtc(void) {
    printf("[init] Checking PL031 RTC...\n");

    /* Try reading RTC via /sys/class/rtc/rtc0/ */
    FILE *f = fopen("/sys/class/rtc/rtc0/since_epoch", "r");
    if (f) {
        char buf[32];
        if (fgets(buf, sizeof(buf), f)) {
            printf("[init] OK: RTC time (since_epoch): %s", buf);
        }
        fclose(f);
    } else {
        /* Fallback: read /dev/rtc0 directly */
        int fd = open("/dev/rtc0", O_RDONLY);
        if (fd >= 0) {
            printf("[init] OK: /dev/rtc0 device exists\n");
            close(fd);
        } else {
            printf("[init] WARN: No RTC device found (%s)\n", strerror(errno));
        }
    }

    /* Also print current time */
    time_t t = time(NULL);
    if (t > 0) {
        printf("[init] System time: %ld seconds since epoch\n", (long)t);
    }
}

static void print_system_info(void) {
    printf("\n[init] System info:\n");

    /* Kernel version */
    FILE *f = fopen("/proc/version", "r");
    if (f) {
        char buf[256];
        if (fgets(buf, sizeof(buf), f))
            printf("[init] Kernel: %s", buf);
        fclose(f);
    }

    /* CPU count */
    f = fopen("/proc/cpuinfo", "r");
    if (f) {
        char line[256];
        int cpus = 0;
        while (fgets(line, sizeof(line), f)) {
            if (strncmp(line, "processor", 9) == 0)
                cpus++;
        }
        printf("[init] CPUs: %d\n", cpus);
        fclose(f);
    }

    /* Memory */
    f = fopen("/proc/meminfo", "r");
    if (f) {
        char line[128];
        if (fgets(line, sizeof(line), f))
            printf("[init] %s", line); /* MemTotal line */
        fclose(f);
    }
}

static void parse_init_rc(void) {
    printf("[init] Parsing /init.rc...\n");
    FILE *f = fopen("/init.rc", "r");
    if (!f) {
        printf("[init] WARN: /init.rc not found (OK for Phase 2)\n");
        return;
    }
    char line[256];
    while (fgets(line, sizeof(line), f)) {
        /* Skip comments and blank lines */
        if (line[0] == '#' || line[0] == '\n')
            continue;
        /* Strip newline */
        size_t len = strlen(line);
        if (len > 0 && line[len-1] == '\n')
            line[len-1] = '\0';
        printf("[init] RC: %s\n", line);

        /* Execute known directives */
        if (strncmp(line, "hostname ", 9) == 0) {
            sethostname(line + 9, strlen(line + 9));
            printf("[init] Set hostname: %s\n", line + 9);
        }
    }
    fclose(f);
}

static void start_shell(void) {
    printf("\n[init] Starting shell (PID 2)...\n");
    printf("[init] Type commands at the prompt. Ctrl+A X to exit QEMU.\n\n");

    pid_t pid = fork();
    if (pid == 0) {
        /* Child: exec shell */
        execl("/bin/sh", "sh", NULL);
        /* If sh not found, try busybox */
        execl("/bin/busybox", "sh", NULL);
        printf("[init] ERROR: Cannot exec shell: %s\n", strerror(errno));
        _exit(1);
    } else if (pid > 0) {
        /* Parent (init): reap children forever */
        for (;;) {
            int status;
            pid_t w = wait(&status);
            if (w > 0) {
                printf("[init] Child %d exited (status %d)\n", w, status);
                /* If shell died, restart it */
                printf("[init] Restarting shell...\n");
                pid = fork();
                if (pid == 0) {
                    execl("/bin/sh", "sh", NULL);
                    execl("/bin/busybox", "sh", NULL);
                    _exit(1);
                }
            }
        }
    } else {
        printf("[init] ERROR: fork failed: %s\n", strerror(errno));
        /* Fall through to sleep loop */
        for (;;) sleep(1);
    }
}

int main(void) {
    /* We must be PID 1 */
    if (getpid() != 1) {
        fprintf(stderr, "init: must be PID 1 (got %d)\n", getpid());
        return 1;
    }

    print_banner();
    mount_filesystems();
    parse_init_rc();
    check_binder();
    check_rtc();
    print_system_info();
    start_shell();

    return 0;
}
```

**Step 2: Write init.rc**

Create `guest/android/init.rc`:

```
# Minimal Android-style init.rc for hypervisor Phase 2
# Parsed by /init (guest/android/init.c)

hostname android-virt
```

**Step 3: Commit sources**

```bash
git add guest/android/init.c guest/android/init.rc
git commit -m "feat: add minimal C /init for Android Phase 2"
```

---

## Task 7: Android Initramfs Build Script

**Files:**
- Create: `guest/android/build-initramfs.sh`

**Step 1: Write the build script**

Create `guest/android/build-initramfs.sh`:

```bash
#!/bin/bash
# Build Android Phase 2 initramfs (minimal C /init + BusyBox)
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

# Install cross-compiler
apt-get update -qq
apt-get install -y -qq gcc-aarch64-linux-gnu cpio gzip > /dev/null 2>&1

WORKDIR=/tmp/initramfs-build
rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"

# ── 1. Cross-compile /init ──────────────────────────────────────────
echo "[1/4] Compiling init.c..."
aarch64-linux-gnu-gcc -static -O2 -Wall \
    -o "$WORKDIR/init" /android/init.c
aarch64-linux-gnu-strip "$WORKDIR/init"
echo "  init: $(du -h "$WORKDIR/init" | cut -f1)"

# ── 2. Create directory structure ────────────────────────────────────
echo "[2/4] Creating directory structure..."
cd "$WORKDIR"
mkdir -p bin sbin etc proc sys dev dev/pts tmp mnt usr/bin usr/sbin

# ── 3. Copy BusyBox from existing Linux initramfs ────────────────────
echo "[3/4] Extracting BusyBox from Linux initramfs..."
LINUX_RAMFS=/linux/initramfs.cpio.gz
if [ ! -f "$LINUX_RAMFS" ]; then
    echo "ERROR: $LINUX_RAMFS not found"
    exit 1
fi

# Extract busybox binary and symlinks from existing initramfs
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
           touch stat vi top od hexdump clear reset; do
    ln -sf busybox "$cmd"
done
cd ..

# sbin symlinks
cd sbin
ln -sf ../bin/busybox ifconfig
ln -sf ../bin/busybox reboot
ln -sf ../bin/busybox halt
cd ..

# ── 4. Copy init.rc and pack ──────────────────────────────────────
echo "[4/4] Packing initramfs..."
cp /android/init.rc "$WORKDIR/init.rc"

# Create cpio archive
cd "$WORKDIR"
find . | cpio -o -H newc 2>/dev/null | gzip > /android/initramfs.cpio.gz

echo ""
echo "=== Android initramfs built ==="
echo "  Output: /android/initramfs.cpio.gz"
echo "  Size: $(du -h /android/initramfs.cpio.gz | cut -f1)"
echo "  Contents:"
echo "    /init          - Android-style init binary (static aarch64)"
echo "    /init.rc       - Minimal config"
echo "    /bin/busybox   - BusyBox (from existing Linux initramfs)"
echo "    /bin/*         - Shell commands (symlinks to busybox)"
```

**Step 2: Build the initramfs via Docker**

Run:
```bash
chmod +x guest/android/build-initramfs.sh
docker run --rm \
    -v $PWD/guest/android:/android \
    -v $PWD/guest/linux:/linux \
    debian:bookworm-slim \
    bash /android/build-initramfs.sh
```

Expected output: `guest/android/initramfs.cpio.gz` (~1.2MB)

**Step 3: Commit**

```bash
git add guest/android/build-initramfs.sh guest/android/initramfs.cpio.gz
git commit -m "feat: add Android initramfs build script + binary"
```

---

## Task 8: Update Makefile for Android Phase 2

**Files:**
- Modify: `Makefile:119-144` — update Android target paths

**Step 1: Update Android paths in Makefile**

Change the Android section (lines 119-144) to use Android-specific DTB and initramfs:

```makefile
# Android guest paths (Phase 2: Android DTB + minimal init)
ANDROID_IMAGE ?= guest/android/Image
ANDROID_DTB ?= guest/android/guest.dtb
ANDROID_INITRAMFS ?= guest/android/initramfs.cpio.gz
ANDROID_DISK ?= guest/linux/disk.img

# QEMU flags for Android guest (2GB RAM)
QEMU_FLAGS_ANDROID := -machine virt,virtualization=on,gic-version=3 \
              -cpu max \
              -smp 4 \
              -m 2G \
              -nographic \
              -kernel $(BINARY)

# Run hypervisor with Android-configured kernel (Phase 2: minimal init)
run-android:
	@echo "Building hypervisor with Linux guest support..."
	cargo build --target aarch64-unknown-none --features linux_guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with Android-configured kernel..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS_ANDROID) \
	    -device loader,file=$(ANDROID_IMAGE),addr=0x48000000 \
	    -device loader,file=$(ANDROID_DTB),addr=0x47000000 \
	    -device loader,file=$(ANDROID_INITRAMFS),addr=0x54000000 \
	    -device loader,file=$(ANDROID_DISK),addr=0x58000000
```

**Step 2: Commit**

```bash
git add Makefile
git commit -m "feat: update Makefile for Android Phase 2 DTB + initramfs"
```

---

## Task 9: Boot Test + Verification

**Step 1: Run all unit tests (no feature flags)**

Run: `make clean && make run 2>&1 | grep -E "suites|FAIL|PL031"`
Expected: 30 test suites pass (29 existing + PL031), 0 failures.

**Step 2: Run existing Linux boot (regression test)**

Run: `make clean && make run-linux`
Expected: Linux 6.12.12 boots to BusyBox shell, `Memory:` shows ~1GB available, all 4 CPUs online.

**Step 3: Boot Android**

Run: `make clean && make run-android`
Expected output (key lines):
```
Android Minimal Init (Phase 2)
Hypervisor Guest - PID 1
[init] Mounted proc on /proc
[init] Mounted sysfs on /sys
[init] Mounted devtmpfs on /dev
[init] Parsing /init.rc...
[init] RC: hostname android-virt
[init] Set hostname: android-virt
[init] Checking binder support...
[init] OK: binder filesystem type registered
[init] OK: binderfs mounted at /dev/binderfs
[init] Checking PL031 RTC...
[init] OK: RTC time (since_epoch): <non-zero>
[init] System info:
[init] Kernel: Linux version 6.6.126 ...
[init] CPUs: 4
[init] MemTotal:   ~1GB
[init] Starting shell (PID 2)...
/ #
```

**Step 4: Verify key features in the shell**

```bash
# Binder
ls /dev/binderfs/
cat /proc/filesystems | grep binder

# RTC
cat /sys/class/rtc/rtc0/since_epoch
date

# Memory
free -m

# CPUs
nproc
```

**Step 5: Debug any issues**

If PL031 probe fails: check `dmesg | grep pl031` for errors. Common issue: PeriphID mismatch.
If init crashes: check `dmesg | grep init` or boot with `rdinit=/bin/sh` in DTB bootargs to get a debug shell.
If binderfs mount fails: check `dmesg | grep binder` — may need `CONFIG_ANDROID_BINDERFS=y` (should be set in Phase 1 kernel config).

---

## Task 10: Update Documentation

**Files:**
- Modify: `DEVELOPMENT_PLAN.md` — mark Phase 2 items complete
- Modify: `CLAUDE.md` — add PL031 to device table, update test count
- Modify: `docs/plans/2026-02-19-android-boot-design.md` — mark Phase 2 complete

**Step 1: Update DEVELOPMENT_PLAN.md**

Mark Phase 2 checklist items as complete:
```
#### Phase 2: Android minimal init ✅ **已完成**
- [x] PL031 RTC emulation (`src/devices/pl031.rs`, ~100 LOC)
- [x] Android ramdisk (minimal `/init` + `init.rc`)
- [x] 独立 Android DTB (`guest/android/guest.dts`, `androidboot.hardware=virt`)
- [x] RAM 增加到 1GB+ guest
```

**Step 2: Update CLAUDE.md**

Add PL031 to the device table in "Core Abstractions":
```
| `VirtualPl031` | `src/devices/pl031.rs` | PL031 RTC trap-and-emulate, counter→seconds |
```

Add to Device enum:
```
    Pl031(pl031::VirtualPl031),
```

Update test count: 30 test suites, ~162 assertions.

Add PL031 to GIC Emulation / device table:
```
| PL031 RTC | 0x09010000 | Trap-and-emulate | `VirtualPl031` (counter→seconds, PrimeCell ID) |
```

**Step 3: Commit**

```bash
git add DEVELOPMENT_PLAN.md CLAUDE.md docs/plans/2026-02-19-android-boot-design.md
git commit -m "docs: mark Android Boot Phase 2 as complete"
```

---

## Verification Checklist

After all tasks:

| Check | Command | Expected |
|-------|---------|----------|
| Unit tests | `make clean && make run` | 30 suites, 0 failures |
| Linux regression | `make clean && make run-linux` | Linux boots, 1GB RAM, 4 CPUs, shell |
| Android boot | `make clean && make run-android` | Init starts, binder OK, RTC OK, shell |
| Clippy | `make clippy` | No warnings |
| Format | `make fmt` | No changes |

## File Change Summary

| File | Action | Task |
|------|--------|------|
| `src/devices/pl031.rs` | NEW (~120 LOC) | 2 |
| `src/devices/mod.rs` | Modify (variant + arms) | 2 |
| `src/vm.rs` | Modify (register PL031) | 3 |
| `src/platform.rs` | Modify (1GB RAM) | 4 |
| `src/main.rs` | Modify (wire test) | 1 |
| `tests/test_pl031.rs` | NEW (~70 LOC) | 1 |
| `tests/mod.rs` | Modify (add module) | 1 |
| `guest/linux/guest.dts` | Modify (1GB mem) | 4 |
| `guest/linux/guest.dtb` | Regenerated | 4 |
| `guest/android/guest.dts` | NEW | 5 |
| `guest/android/guest.dtb` | NEW | 5 |
| `guest/android/init.c` | NEW (~180 LOC) | 6 |
| `guest/android/init.rc` | NEW | 6 |
| `guest/android/build-initramfs.sh` | NEW | 7 |
| `guest/android/initramfs.cpio.gz` | NEW (built) | 7 |
| `Makefile` | Modify | 4, 8 |
| `DEVELOPMENT_PLAN.md` | Modify | 10 |
| `CLAUDE.md` | Modify | 10 |
