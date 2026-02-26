# Toolchain and Environment

## Overview

The project runs entirely on a Linux x86_64 host, cross-compiling to ARM64. No physical hardware required — QEMU emulates everything including GICv3, Secure World (EL3/S-EL2), and multiple CPUs.

## Rust Toolchain

```toml
# rust-toolchain.toml
[toolchain]
channel = "nightly"
components = ["rust-src", "rustfmt", "clippy"]
```

Nightly is required for:
- `#![no_std]` + `#![no_main]` bare-metal binary
- Inline assembly (`core::arch::asm!`) for ARM64 instructions
- Custom target specification

## Custom Target

```json
// aarch64-unknown-none.json
{
  "llvm-target": "aarch64-unknown-none",
  "panic-strategy": "abort",
  "disable-redzone": true,
  "features": "+strict-align,+neon,+fp-armv8"
}
```

Key settings:
- **`panic-strategy: abort`** — no unwinding in bare-metal
- **`disable-redzone: true`** — interrupts can corrupt the red zone
- **`+neon,+fp-armv8`** — floating-point needed for some Rust codegen paths

## Cross-Compilation Tools

| Tool | Purpose |
|------|---------|
| `aarch64-linux-gnu-gcc` | Assembles `boot.S` and `exception.S` |
| `aarch64-linux-gnu-ar` | Archives assembly objects into `libboot.a` |
| `aarch64-linux-gnu-objcopy` | Converts ELF to raw binary for QEMU |
| `rust-lld` | Links Rust code with custom linker script |

The build system (`build.rs`) cross-compiles ARM64 assembly via `aarch64-linux-gnu-gcc`, archives it into a static library, and links it with the Rust binary using `--whole-archive`.

## QEMU

```bash
# Normal mode (NS-EL2 hypervisor)
qemu-system-aarch64 \
  -machine virt,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic

# Secure World mode (S-EL2 SPMC + TF-A)
qemu-system-aarch64 \
  -machine virt,secure=on,virtualization=on,gic-version=3 \
  -cpu max -smp 4 -m 2G -nographic \
  -bios flash.bin
```

QEMU `virt` machine provides: PL011 UART, GICv3, virtio-mmio bus, PL031 RTC. With `secure=on`, it adds EL3 firmware support for TF-A boot chain.

QEMU 9.2.3 (built from source) is required for `secure=on` + `virtualization=on` combined mode.

## Docker Builds

Heavy cross-compilation tasks run in Docker containers with cached volumes:

| Target | Docker Volume | Purpose |
|--------|---------------|---------|
| Linux kernel | `kernel-build-cache` | Upstream 6.12.12 defconfig |
| pKVM kernel | `pkvm-kernel-build-cache` | AOSP android16-6.12 with `gki_defconfig` |
| TF-A firmware | `tfa-build-cache` / `tfa-pkvm-build-cache` | ARM Trusted Firmware v2.12 |

This keeps the host clean and makes builds reproducible.

## Project Structure

```
hypervisor/
├── src/                    # Rust source
│   ├── main.rs             # Entry + test orchestration
│   ├── vm.rs               # VM lifecycle
│   ├── vcpu.rs             # vCPU state machine
│   ├── arch/aarch64/       # ARM64-specific code
│   │   ├── boot.S          # EL2 entry point
│   │   ├── exception.S     # Exception vectors
│   │   ├── linker.ld       # Memory layout
│   │   └── ...
│   ├── devices/            # Device emulation
│   ├── ffa/                # FF-A v1.1 proxy
│   └── ...
├── tests/                  # 33 test suites
├── tfa/                    # TF-A configs, SP binaries, BL33 test client
├── guest/linux/            # Kernel build scripts, DTBs, initramfs
├── Cargo.toml              # Features: linux_guest, multi_pcpu, sel2, tfa_boot
└── Makefile                # 20+ targets
```

## Quick Start

```bash
# Build + run unit tests (no guest, ~282 assertions)
make run

# Boot Linux to BusyBox shell (4 vCPUs, virtio-blk)
make run-linux

# Boot pKVM + our SPMC (requires Docker builds first)
make build-pkvm-kernel  # ~15-30min first time
make build-tfa-pkvm
make run-pkvm
```
