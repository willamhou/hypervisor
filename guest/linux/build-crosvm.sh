#!/bin/bash
# Build crosvm (Google VMM) for aarch64-unknown-linux-gnu
# Cross-compiles with minimal features for nested VM boot on pKVM
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive

echo "=== Building crosvm for aarch64 ==="

# Install build dependencies
echo ">>> Installing build dependencies..."
apt-get update -qq
dpkg --add-architecture arm64
apt-get update -qq
apt-get install -y -qq \
    build-essential pkg-config git curl ca-certificates \
    gcc-aarch64-linux-gnu g++-aarch64-linux-gnu \
    protobuf-compiler libprotobuf-dev python3 \
    libcap-dev:arm64 libfdt-dev:arm64 \
    libclang-dev clang \
    2>&1 | tail -3

# Install Rust
export RUSTUP_HOME=/build/.rustup
export CARGO_HOME=/build/.cargo
if [ ! -f "$CARGO_HOME/bin/rustup" ]; then
    echo ">>> Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable 2>&1 | tail -3
fi
export PATH="$CARGO_HOME/bin:$PATH"

cd /build

# Clone crosvm
if [ ! -d crosvm ]; then
    echo ">>> Cloning crosvm..."
    git clone --depth 1 https://chromium.googlesource.com/crosvm/crosvm 2>&1 | tail -3
fi
cd crosvm

# Init submodules (minijail)
git submodule update --init --depth 1 2>&1 | tail -3
echo ">>> crosvm commit: $(git rev-parse --short HEAD)"

# Patch rust-toolchain to include aarch64 target
if [ -f rust-toolchain ] || [ -f rust-toolchain.toml ]; then
    CHANNEL=$(grep 'channel' rust-toolchain* | head -1 | sed 's/.*"\(.*\)".*/\1/')
    echo ">>> Patching rust-toolchain for aarch64 (channel=$CHANNEL)..."
    cat > rust-toolchain << TOOLCHAIN
[toolchain]
channel = "$CHANNEL"
components = [ "rustfmt", "clippy", "llvm-tools-preview" ]
targets = [ "aarch64-unknown-linux-gnu" ]
TOOLCHAIN
fi

# Ensure toolchain is synced
rustup show 2>&1 | tail -3
echo ">>> Rust: $(rustc --version)"
echo ">>> Targets: $(rustup target list --installed | tr '\n' ' ')"

# Configure cross-compilation
mkdir -p .cargo
cat > .cargo/config.toml << 'CARGO'
[target.aarch64-unknown-linux-gnu]
linker = "aarch64-linux-gnu-gcc"
CARGO

export CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
export CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++
export PKG_CONFIG_ALLOW_CROSS=1
export BINDGEN_EXTRA_CLANG_ARGS="--sysroot=/usr/aarch64-linux-gnu -I/usr/aarch64-linux-gnu/include"

# Build with minimal features (no GPU, audio, USB — just serial + block)
echo ">>> Building crosvm (--no-default-features)..."
cargo build \
    --target aarch64-unknown-linux-gnu \
    --release \
    --no-default-features \
    2>&1

echo ""
ls -lh target/aarch64-unknown-linux-gnu/release/crosvm

# Copy binary
cp target/aarch64-unknown-linux-gnu/release/crosvm /output/crosvm

# Copy required shared libraries
echo ">>> Collecting shared library dependencies..."
aarch64-linux-gnu-readelf -d target/aarch64-unknown-linux-gnu/release/crosvm | grep NEEDED

mkdir -p /output/crosvm-libs
for lib in libc.so.6 libm.so.6 libdl.so.2 libpthread.so.0 libgcc_s.so.1 ld-linux-aarch64.so.1 libcap.so.2; do
    find /usr/aarch64-linux-gnu/lib/ /usr/lib/aarch64-linux-gnu/ -name "$lib*" -exec cp -L {} /output/crosvm-libs/ \; 2>/dev/null || true
done
echo ">>> Libs:"
ls -la /output/crosvm-libs/

echo ""
echo "=== crosvm build complete ==="
echo "Binary: /output/crosvm ($(du -sh /output/crosvm | awk '{print $1}'))"
echo "Libs:   /output/crosvm-libs/"
