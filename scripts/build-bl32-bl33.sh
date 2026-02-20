#!/bin/bash
# Build trivial BL32 (S-EL2) and BL33 (NS-EL2) hello-world binaries.
# Runs inside a Debian container.
#
# Usage (from project root):
#   docker run --rm -v $(pwd)/tfa:/output -v $(pwd):/src \
#       debian:bookworm-slim bash /src/scripts/build-bl32-bl33.sh
#
# Output: /output/bl32.bin, /output/bl33.bin

set -euo pipefail

echo "=== Building trivial BL32 (S-EL2) and BL33 (NS-EL2) ==="

# Install cross-compiler
apt-get update -qq
apt-get install -y -qq gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu 2>/dev/null

# Build BL32 (S-EL2 hello)
echo ">>> Building BL32..."
cd /src/tfa/bl32_hello
aarch64-linux-gnu-as start.S -o /tmp/bl32.o
aarch64-linux-gnu-ld -T linker.ld /tmp/bl32.o -o /tmp/bl32.elf
aarch64-linux-gnu-objcopy -O binary /tmp/bl32.elf /output/bl32.bin
echo "    BL32: $(ls -lh /output/bl32.bin | awk '{print $5}')"

# Build BL33 (NS-EL2 hello)
echo ">>> Building BL33..."
cd /src/tfa/bl33_hello
aarch64-linux-gnu-as start.S -o /tmp/bl33.o
aarch64-linux-gnu-ld -T linker.ld /tmp/bl33.o -o /tmp/bl33.elf
aarch64-linux-gnu-objcopy -O binary /tmp/bl33.elf /output/bl33.bin
echo "    BL33: $(ls -lh /output/bl33.bin | awk '{print $5}')"

echo ">>> Done! BL32 and BL33 ready in /output/"
