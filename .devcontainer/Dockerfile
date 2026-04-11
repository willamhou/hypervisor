FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# Install aarch64 cross-compiler and QEMU
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-aarch64-linux-gnu \
    binutils-aarch64-linux-gnu \
    qemu-system-arm \
    qemu-efi-aarch64 \
    ipxe-qemu \
    && rm -rf /var/lib/apt/lists/*

# Install Rust nightly with aarch64 target
RUN rustup toolchain install nightly \
    && rustup default nightly \
    && rustup target add aarch64-unknown-none \
    && rustup component add rust-src rustfmt clippy
