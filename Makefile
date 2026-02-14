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
              -m 1G \
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
	@echo "  debug     - Build and run in QEMU with GDB server"
	@echo "  clean     - Clean build artifacts"
	@echo "  check     - Check code without building"
	@echo "  clippy    - Run clippy linter"
	@echo "  fmt       - Format code"
