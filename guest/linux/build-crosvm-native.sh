#!/bin/bash
# Build crosvm NATIVELY on an aarch64 host (no cross-compile).
# Reuses the crosvm-build-cache clone; fixes the minijail bindgen
# 'sys/resource.h not found' failure caused by the cross sysroot.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive

echo "=== Building crosvm natively (aarch64 host) ==="
echo ">>> arch: $(dpkg --print-architecture)"

echo ">>> Installing native build dependencies..."
apt-get update -qq
apt-get install -y -qq \
    build-essential pkg-config git curl ca-certificates \
    protobuf-compiler libprotobuf-dev python3 \
    libcap-dev libfdt-dev libc6-dev \
    libclang-dev clang \
    2>&1 | tail -3

export RUSTUP_HOME=/build/.rustup
export CARGO_HOME=/build/.cargo
if [ ! -f "$CARGO_HOME/bin/rustup" ]; then
    echo ">>> Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable 2>&1 | tail -3
fi
export PATH="$CARGO_HOME/bin:$PATH"

cd /build
if [ ! -d crosvm ]; then
    echo ">>> Cloning crosvm..."
    git clone --depth 1 https://chromium.googlesource.com/crosvm/crosvm 2>&1 | tail -3
fi
cd crosvm
git submodule update --init --depth 1 2>&1 | tail -3
echo ">>> crosvm commit: $(git rev-parse --short HEAD)"

# Pin toolchain to the channel crosvm wants, native target only.
if [ -f rust-toolchain ] || [ -f rust-toolchain.toml ]; then
    CHANNEL=$(grep 'channel' rust-toolchain* | head -1 | sed 's/.*"\(.*\)".*/\1/')
    echo ">>> Patching rust-toolchain (channel=$CHANNEL, native)..."
    cat > rust-toolchain << TOOLCHAIN
[toolchain]
channel = "$CHANNEL"
components = [ "rustfmt", "clippy", "llvm-tools-preview" ]
TOOLCHAIN
fi

# Remove any cross config left by the previous run.
rm -f .cargo/config.toml

rustup show 2>&1 | tail -3
echo ">>> Rust: $(rustc --version)"

# NATIVE build — no --target, no cross sysroot. bindgen/clang find
# /usr/include/aarch64-linux-gnu/sys/resource.h on their own.
echo ">>> Building crosvm natively (--no-default-features)..."
cargo build --release --no-default-features 2>&1

echo ""
ls -lh target/release/crosvm
cp target/release/crosvm /output/crosvm
echo ">>> NEEDED libs:"
readelf -d target/release/crosvm | grep NEEDED || true

mkdir -p /output/crosvm-libs
for lib in libc.so.6 libm.so.6 libdl.so.2 libpthread.so.0 libgcc_s.so.1 ld-linux-aarch64.so.1 libcap.so.2; do
    find /usr/lib/aarch64-linux-gnu/ /lib/aarch64-linux-gnu/ -name "$lib*" -exec cp -L {} /output/crosvm-libs/ \; 2>/dev/null || true
done
echo ">>> Libs:"; ls -la /output/crosvm-libs/
echo "=== crosvm native build complete: /output/crosvm ($(du -sh /output/crosvm | awk '{print $1}')) ==="
