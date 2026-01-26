#!/bin/bash
# Build guest test program

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUTPUT_DIR="${SCRIPT_DIR}/../target/guest"

mkdir -p "${OUTPUT_DIR}"

echo "Building guest test program..."

# Compile guest_test.S
aarch64-linux-gnu-gcc -c "${SCRIPT_DIR}/guest_test.S" \
    -o "${OUTPUT_DIR}/guest_test.o" \
    -nostdlib -ffreestanding

# Link guest to a specific address (we'll load it at 0x50000000)
aarch64-linux-gnu-ld "${OUTPUT_DIR}/guest_test.o" \
    -o "${OUTPUT_DIR}/guest_test.elf" \
    -Ttext=0x50000000 \
    --no-dynamic-linker

# Create raw binary
aarch64-linux-gnu-objcopy -O binary \
    "${OUTPUT_DIR}/guest_test.elf" \
    "${OUTPUT_DIR}/guest_test.bin"

# Display info
echo "Guest binary created:"
ls -lh "${OUTPUT_DIR}/guest_test.bin"
aarch64-linux-gnu-readelf -h "${OUTPUT_DIR}/guest_test.elf" | grep Entry
aarch64-linux-gnu-objdump -d "${OUTPUT_DIR}/guest_test.elf" | head -30

echo "Done!"
