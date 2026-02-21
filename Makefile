.PHONY: all build run debug clean build-qemu build-bl32-bl33 build-tfa build-tfa-bl33 build-spmc build-tfa-spmc run-sel2 run-tfa-linux run-spmc

# Auto-load Cargo environment
SHELL := /bin/bash
CARGO_HOME ?= $(HOME)/.cargo
export PATH := $(CARGO_HOME)/bin:$(PATH)

# Target binary name
TARGET := hypervisor
BUILD_DIR := target/aarch64-unknown-none/debug
BINARY := $(BUILD_DIR)/$(TARGET)
BINARY_BIN := $(BUILD_DIR)/$(TARGET).bin

# QEMU configuration
# System QEMU for normal targets (has ROM files for default NIC)
QEMU := qemu-system-aarch64
# Local QEMU 9.2+ for S-EL2 targets (secure=on requires newer QEMU)
QEMU_SEL2 := $(shell test -x tools/qemu-system-aarch64 && echo tools/qemu-system-aarch64 || echo qemu-system-aarch64)
QEMU_FLAGS := -machine virt,virtualization=on,gic-version=3 \
              -cpu max \
              -smp 4 \
              -m 2G \
              -nographic \
              -kernel $(BINARY)

# Build in debug mode
all: build

build:
	@echo "Building hypervisor..."
	cargo build --target aarch64-unknown-none
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)

# Run in QEMU
run: build
	@echo "Starting QEMU..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS)

# Guest ELF path (set via environment variable)
GUEST_ELF ?=

# Run hypervisor with guest ELF (Zephyr)
run-guest:
ifndef GUEST_ELF
	$(error GUEST_ELF is not set. Usage: make run-guest GUEST_ELF=/path/to/zephyr.elf)
endif
	@echo "Building hypervisor with guest support..."
	cargo build --target aarch64-unknown-none --features guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with guest: $(GUEST_ELF)"
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(GUEST_ELF),addr=0x48000000

# Linux guest paths
LINUX_IMAGE ?= guest/linux/Image
LINUX_DTB ?= guest/linux/guest.dtb
LINUX_INITRAMFS ?= guest/linux/initramfs.cpio.gz
LINUX_DISK ?= guest/linux/disk.img

# Run hypervisor with Linux kernel
run-linux:
	@echo "Building hypervisor with Linux guest support..."
	cargo build --target aarch64-unknown-none --features linux_guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with Linux kernel..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(LINUX_IMAGE),addr=0x48000000 \
	    -device loader,file=$(LINUX_DTB),addr=0x47000000 \
	    -device loader,file=$(LINUX_INITRAMFS),addr=0x54000000 \
	    -device loader,file=$(LINUX_DISK),addr=0x58000000

# Run hypervisor with Linux kernel on multiple physical CPUs
run-linux-smp:
	@echo "Building hypervisor with multi-pCPU support..."
	cargo build --target aarch64-unknown-none --features multi_pcpu
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with Linux kernel (multi-pCPU)..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS) \
	    -device loader,file=$(LINUX_IMAGE),addr=0x48000000 \
	    -device loader,file=$(LINUX_DTB),addr=0x47000000 \
	    -device loader,file=$(LINUX_INITRAMFS),addr=0x54000000 \
	    -device loader,file=$(LINUX_DISK),addr=0x58000000

# VM 1 guest paths (default: reuse same kernel/initramfs, separate DTB and disk)
LINUX_DTB_VM1 ?= guest/linux/guest-vm1.dtb
LINUX_DISK_VM1 ?= guest/linux/disk-vm1.img

# QEMU flags for multi-VM (2GB RAM to fit both VMs)
QEMU_FLAGS_MULTI_VM := -machine virt,virtualization=on,gic-version=3 \
              -cpu max \
              -smp 4 \
              -m 2G \
              -nographic \
              -kernel $(BINARY)

# Run hypervisor with two Linux VMs time-sliced on single pCPU
run-multi-vm:
	@echo "Building hypervisor with multi-VM support..."
	cargo build --target aarch64-unknown-none --features multi_vm
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting QEMU with 2 Linux VMs..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU) $(QEMU_FLAGS_MULTI_VM) \
	    -device loader,file=$(LINUX_IMAGE),addr=0x48000000 \
	    -device loader,file=$(LINUX_DTB),addr=0x47000000 \
	    -device loader,file=$(LINUX_INITRAMFS),addr=0x54000000 \
	    -device loader,file=$(LINUX_DISK),addr=0x58000000 \
	    -device loader,file=$(LINUX_IMAGE),addr=0x68000000 \
	    -device loader,file=$(LINUX_DTB_VM1),addr=0x67000000 \
	    -device loader,file=$(LINUX_INITRAMFS),addr=0x74000000 \
	    -device loader,file=$(LINUX_DISK_VM1),addr=0x78000000

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

# Run with GDB server (for debugging)
debug: build
	@echo "Starting QEMU with GDB server on port 1234..."
	@echo "In another terminal, run: gdb-multiarch -ex 'target remote :1234' $(BINARY)"
	$(QEMU) $(QEMU_FLAGS) -s -S

# Clean build artifacts
clean:
	cargo clean

# Check code without building
check:
	cargo check --target aarch64-unknown-none

# Run clippy linter
clippy:
	cargo clippy --target aarch64-unknown-none

# Format code
fmt:
	cargo fmt

# === S-EL2 / TF-A targets (Phase 4) ===

# TF-A paths
TFA_DIR := tfa
TFA_FLASH := $(TFA_DIR)/flash.bin

# Build QEMU 9.2.3 from source (one-time, ~5-10 min)
build-qemu:
	@echo "Building QEMU 9.2.3 from source (Docker)..."
	mkdir -p tools
	docker run --rm \
	    -v $(PWD)/tools:/output \
	    -v $(PWD)/scripts:/scripts \
	    -v qemu-build-cache:/build \
	    debian:bookworm-slim bash /scripts/build-qemu.sh

# Build trivial BL32 (S-EL2) and BL33 (NS-EL2) hello binaries
build-bl32-bl33:
	@echo "Building trivial BL32/BL33..."
	docker run --rm \
	    -v $(PWD)/tfa:/output \
	    -v $(PWD):/src \
	    debian:bookworm-slim bash /src/scripts/build-bl32-bl33.sh

# Build TF-A + flash.bin (requires bl32.bin + bl33.bin)
build-tfa: build-bl32-bl33
	@echo "Building TF-A with SPD=spmd (Docker)..."
	docker run --rm \
	    -v $(PWD)/tfa:/output \
	    -v $(PWD):/src \
	    -v tfa-build-cache:/output/tfa-src \
	    debian:bookworm-slim bash /src/scripts/build-tfa.sh

# Boot TF-A with trivial BL32 at S-EL2 (requires QEMU 9.2+ and flash.bin)
run-sel2:
	@test -f $(TFA_FLASH) || (echo "ERROR: $(TFA_FLASH) not found. Run 'make build-tfa' first." && exit 1)
	@echo "Starting QEMU with TF-A boot chain (S-EL2)..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU_SEL2) -machine virt,secure=on,virtualization=on,gic-version=3 \
	    -cpu max -smp 4 -m 2G -nographic \
	    -bios $(TFA_FLASH) -nic none

# TF-A flash.bin with PRELOADED_BL33_BASE (hypervisor loaded via QEMU -device loader)
TFA_FLASH_BL33 := $(TFA_DIR)/flash-bl33.bin

# Build TF-A flash.bin that expects BL33 preloaded at 0x40200000
# (0x40200000 avoids QEMU's auto-generated DTB at 0x40000000-0x40100000)
build-tfa-bl33: build-bl32-bl33
	@echo "Building TF-A with PRELOADED_BL33_BASE=0x40200000 (Docker)..."
	docker run --rm \
	    -v $(PWD)/tfa:/output \
	    -v $(PWD):/src \
	    -v tfa-bl33-build-cache:/output/tfa-src \
	    -e TFA_PRELOADED_BL33_BASE=0x40200000 \
	    debian:bookworm-slim bash /src/scripts/build-tfa.sh
	mv $(TFA_DIR)/flash.bin $(TFA_FLASH_BL33)

# Boot: TF-A → BL32 (stub S-EL2) → BL33 (our hypervisor at NS-EL2) → Linux
run-tfa-linux:
	@test -f $(TFA_FLASH_BL33) || (echo "ERROR: $(TFA_FLASH_BL33) not found. Run 'make build-tfa-bl33' first." && exit 1)
	@echo "Building hypervisor with Linux guest support..."
	cargo build --target aarch64-unknown-none --features linux_guest
	@echo "Creating raw binary..."
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(BINARY_BIN)
	@echo "Starting TF-A → hypervisor → Linux boot chain..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU_SEL2) -machine virt,secure=on,virtualization=on,gic-version=3 \
	    -cpu max -smp 4 -m 2G -nographic \
	    -bios $(TFA_FLASH_BL33) \
	    -device loader,file=$(BINARY_BIN),addr=0x40200000,force-raw=on \
	    -device loader,file=$(LINUX_IMAGE),addr=0x48000000,force-raw=on \
	    -device loader,file=$(LINUX_DTB),addr=0x47000000,force-raw=on \
	    -device loader,file=$(LINUX_INITRAMFS),addr=0x54000000,force-raw=on \
	    -device loader,file=$(LINUX_DISK),addr=0x58000000,force-raw=on \
	    -nic none

# === S-EL2 SPMC targets (Sprint 4.3) ===

# SPMC binary (hypervisor compiled with sel2 feature)
SPMC_BIN := $(BUILD_DIR)/$(TARGET)_spmc.bin

# Build hypervisor as S-EL2 SPMC (BL32)
build-spmc:
	@echo "Building SPMC (sel2 feature)..."
	cargo build --target aarch64-unknown-none --features sel2
	aarch64-linux-gnu-objcopy -O binary $(BINARY) $(SPMC_BIN)
	@echo "SPMC binary: $(SPMC_BIN)"

# Build BL33 FF-A test client (sends FF-A SMCs to SPMC, prints PASS/FAIL)
build-bl33-ffa-test:
	@echo "Building BL33 FF-A test client..."
	aarch64-linux-gnu-as -o $(BUILD_DIR)/bl33_ffa_test.o tfa/bl33_ffa_test/start.S
	aarch64-linux-gnu-ld -T tfa/bl33_ffa_test/linker.ld -o $(BUILD_DIR)/bl33_ffa_test.elf $(BUILD_DIR)/bl33_ffa_test.o
	aarch64-linux-gnu-objcopy -O binary $(BUILD_DIR)/bl33_ffa_test.elf tfa/bl33_ffa_test.bin
	@echo "BL33 test client: tfa/bl33_ffa_test.bin"

# Build TF-A with real SPMC (BL32) + FF-A test client (BL33)
TFA_FLASH_SPMC := $(TFA_DIR)/flash-spmc.bin

# 1. build-bl32-bl33: builds trivial bl32.bin + bl33.bin (Docker, root-owned)
# 2. build-spmc: builds real SPMC binary
# 3. build-bl33-ffa-test: builds FF-A test client binary
# 4. Recipe: Docker overwrites bl32.bin with SPMC, bl33.bin with test client, then builds TF-A
build-tfa-spmc: build-bl32-bl33 build-spmc build-bl33-ffa-test
	@echo "Replacing trivial bl32.bin with real SPMC..."
	docker run --rm \
	    -v $(PWD)/tfa:/output \
	    -v $(PWD)/$(SPMC_BIN):/spmc.bin:ro \
	    debian:bookworm-slim cp /spmc.bin /output/bl32.bin
	@echo "Replacing trivial bl33.bin with FF-A test client..."
	docker run --rm \
	    -v $(PWD)/tfa:/output \
	    -v $(PWD)/tfa/bl33_ffa_test.bin:/bl33_test.bin:ro \
	    debian:bookworm-slim cp /bl33_test.bin /output/bl33.bin
	@echo "Building TF-A with real SPMC as BL32 + FF-A test client as BL33..."
	docker run --rm \
	    -v $(PWD)/tfa:/output \
	    -v $(PWD):/src \
	    -v tfa-spmc-build-cache:/output/tfa-src \
	    debian:bookworm-slim bash /src/scripts/build-tfa.sh
	mv $(TFA_DIR)/flash.bin $(TFA_FLASH_SPMC)

# Boot TF-A with SPMC (S-EL2) + FF-A test client (NS-EL2)
run-spmc:
	@test -f $(TFA_FLASH_SPMC) || (echo "ERROR: $(TFA_FLASH_SPMC) not found. Run 'make build-tfa-spmc' first." && exit 1)
	@echo "Starting QEMU with real SPMC at S-EL2..."
	@echo "Press Ctrl+A then X to exit QEMU"
	$(QEMU_SEL2) -machine virt,secure=on,virtualization=on,gic-version=3 \
	    -cpu max -smp 4 -m 2G -nographic \
	    -bios $(TFA_FLASH_SPMC) -nic none

# Help
help:
	@echo "Available targets:"
	@echo "  all       - Build the hypervisor (default)"
	@echo "  build     - Build the hypervisor"
	@echo "  run       - Build and run in QEMU"
	@echo "  run-guest - Build and run with Zephyr guest (GUEST_ELF=/path/to/elf)"
	@echo "  run-linux - Build and run with Linux kernel guest (single pCPU)"
	@echo "  run-linux-smp - Build and run with Linux kernel (multi-pCPU)"
	@echo "  run-multi-vm  - Build and run with 2 Linux VMs (time-sliced)"
	@echo "  run-android   - Build and run with Android-configured kernel"
	@echo "  run-sel2      - Boot TF-A with BL32 at S-EL2 (Phase 4)"
	@echo "  run-tfa-linux - Boot TF-A -> hypervisor -> Linux (Phase 4)"
	@echo "  build-qemu    - Build QEMU 9.2.3 from source (one-time)"
	@echo "  build-spmc    - Build hypervisor as S-EL2 SPMC (BL32)"
	@echo "  build-tfa-spmc - Build TF-A with real SPMC as BL32"
	@echo "  run-spmc      - Boot TF-A with real SPMC at S-EL2"
	@echo "  build-tfa     - Build TF-A + flash.bin with SPD=spmd"
	@echo "  build-tfa-bl33 - Build TF-A flash.bin with preloaded BL33"
	@echo "  build-bl32-bl33 - Build trivial BL32/BL33 hello binaries"
	@echo "  debug     - Build and run in QEMU with GDB server"
	@echo "  clean     - Clean build artifacts"
	@echo "  check     - Check code without building"
	@echo "  clippy    - Run clippy linter"
	@echo "  fmt       - Format code"
