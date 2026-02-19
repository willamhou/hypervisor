.PHONY: all build run debug clean

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
QEMU := qemu-system-aarch64
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
	@echo "  run-android   - Build and run with Android-configured kernel (Phase 1)"
	@echo "  debug     - Build and run in QEMU with GDB server"
	@echo "  clean     - Clean build artifacts"
	@echo "  check     - Check code without building"
	@echo "  clippy    - Run clippy linter"
	@echo "  fmt       - Format code"
