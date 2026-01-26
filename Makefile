.PHONY: all build run debug clean

# Target binary name
TARGET := hypervisor
BUILD_DIR := target/aarch64-unknown-none/debug
BINARY := $(BUILD_DIR)/$(TARGET)
BINARY_BIN := $(BUILD_DIR)/$(TARGET).bin

# QEMU configuration
QEMU := qemu-system-aarch64
QEMU_FLAGS := -machine virt \
              -cpu cortex-a57 \
              -smp 1 \
              -m 1G \
              -nographic \
              -kernel $(BINARY_BIN)

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
	@echo "  all     - Build the hypervisor (default)"
	@echo "  build   - Build the hypervisor"
	@echo "  run     - Build and run in QEMU"
	@echo "  debug   - Build and run in QEMU with GDB server"
	@echo "  clean   - Clean build artifacts"
	@echo "  check   - Check code without building"
	@echo "  clippy  - Run clippy linter"
	@echo "  fmt     - Format code"
