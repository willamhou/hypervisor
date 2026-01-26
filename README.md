# ARM64 Hypervisor

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

An open-source Type-1 Hypervisor for ARM64, written in Rust, supporting both traditional virtualization and confidential computing (TEE/FF-A/RME).

## 🎯 Project Status

**Current Milestone**: M0 - Project Initialization (Week 1-2)

- [x] Requirements document
- [x] Development plan
- [x] Project structure
- [ ] Rust environment setup
- [ ] First boot in QEMU
- [ ] "Hello from EL2!" output

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) for the full roadmap.

## 🌟 Features (Planned)

### Core Virtualization (M1-M2)
- ✅ **vCPU Management**: Create and manage virtual CPUs
- ✅ **Stage-2 Memory Virtualization**: IPA to PA translation
- ✅ **GICv3 Interrupt Virtualization**: Virtual interrupt controller
- ✅ **virtio Devices**: virtio-console, virtio-blk
- ✅ **SMP Support**: Multi-core virtual machines

### Security Extensions (M3-M5)
- 🔒 **FF-A (Firmware Framework)**: Secure Partition communication
- 🔒 **TEE Support**: Secure Hypervisor (S-EL2) with OP-TEE integration
- 🔒 **RME & CCA**: Realm Management Extension for confidential computing
- 🔒 **Remote Attestation**: Verify Realm integrity

## 🚀 Quick Start

### Prerequisites

1. **Rust Toolchain** (nightly):
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default nightly
rustup target add aarch64-unknown-none
rustup component add rust-src rustfmt clippy
```

2. **ARM64 Cross-Compilation Tools**:
```bash
# Ubuntu/Debian
sudo apt install gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu

# macOS
brew install aarch64-elf-gcc
```

3. **QEMU**:
```bash
# Ubuntu/Debian
sudo apt install qemu-system-aarch64

# macOS
brew install qemu
```

4. **GDB** (optional, for debugging):
```bash
# Ubuntu/Debian
sudo apt install gdb-multiarch

# macOS
brew install gdb
```

### Building

```bash
# Build the hypervisor
make build

# Or use cargo directly
cargo build --target aarch64-unknown-none
```

### Running

```bash
# Run in QEMU
make run

# Expected output:
# ========================================
#   ARM64 Hypervisor - Milestone 0
# ========================================
#
# Hello from EL2!
# ...
```

To exit QEMU: Press `Ctrl+A` then `X`

### Debugging

```bash
# Terminal 1: Start QEMU with GDB server
make debug

# Terminal 2: Connect GDB
gdb-multiarch target/aarch64-unknown-none/debug/hypervisor
(gdb) target remote :1234
(gdb) break rust_main
(gdb) continue
```

## 📁 Project Structure

```
hypervisor/
├── arch/
│   └── aarch64/
│       ├── boot.S              # Assembly boot code
│       └── linker.ld           # Linker script
├── src/
│   ├── main.rs                 # Rust entry point
│   ├── lib.rs                  # Library root
│   └── uart.rs                 # UART driver (PL011)
├── docs/
│   └── design/                 # Design documents
├── Cargo.toml                  # Rust package config
├── Makefile                    # Build automation
├── REQUIREMENTS.md             # Project requirements
└── DEVELOPMENT_PLAN.md         # Development roadmap
```

## 📚 Documentation

- [Requirements Document](REQUIREMENTS.md) - Detailed project requirements
- [Development Plan](DEVELOPMENT_PLAN.md) - Milestone-based development roadmap
- Design Documents (coming soon in `docs/design/`)

## 🛠️ Development

### Code Style

```bash
# Format code
make fmt

# Run linter
make clippy

# Check without building
make check
```

### Testing

Testing infrastructure is being developed. TDD approach will be followed.

## 🗺️ Roadmap

| Milestone | Description | Timeline | Status |
|-----------|-------------|----------|--------|
| M0 | Project Initialization | Week 1-2 | 🚧 In Progress |
| M1 | MVP - Basic Virtualization | Week 3-10 | 📅 Planned |
| M2 | Enhanced Features | Week 11-18 | 📅 Planned |
| M3 | FF-A Implementation | Week 19-28 | 📅 Planned |
| M4 | Secure EL2 & TEE | Week 29-36 | 📅 Planned |
| M5 | RME & CCA | Week 37-52+ | 📅 Planned |

**Total Estimated Time**: 12-14 months

## 🤝 Contributing

This project is in early development. Contributions are welcome!

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

Please read the [Development Plan](DEVELOPMENT_PLAN.md) to understand the project direction.

## 📖 Learning Resources

### ARM Architecture
- [ARM Architecture Reference Manual](https://developer.arm.com/documentation/) - Official ARM documentation
- ARM RME Specification
- FF-A Specification v1.1/v1.2

### Reference Projects
- [KVM/ARM](https://www.kernel.org/doc/html/latest/virt/kvm/arm/) - Linux kernel ARM virtualization
- [ARM Trusted Firmware-A](https://github.com/ARM-software/arm-trusted-firmware) - EL3 firmware
- [OP-TEE](https://github.com/OP-TEE/optee_os) - Open Portable TEE
- [TF-RMM](https://git.trustedfirmware.org/TF-RMM/tf-rmm.git/) - ARM's reference RMM

## 📄 License

This project is dual-licensed under:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

You may choose either license for your use.

## 👤 Author

Willam Hou - [@willamhou](https://github.com/willamhou)

## 🙏 Acknowledgments

- ARM for excellent architecture documentation
- The Rust embedded community
- KVM, Xen, and other open-source hypervisors for inspiration

---

**Note**: This is an educational and research project. It is not production-ready and should not be used in production environments without thorough testing and security audits.
