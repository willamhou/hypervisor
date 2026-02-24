# Part 1: First Boot — From Zero to EL2

> From an empty directory to "Hello from EL2!" on QEMU — the first 4 commits that prove bare-metal Rust works at the hypervisor privilege level.

**Commits**: `#1-4 (9b72719..b2ff49f)`

## What We're Building

The goal of this milestone is deceptively simple: print a message from EL2. But to get there, we need to solve several problems that don't exist in normal application development:

1. **No OS, no runtime** — the binary runs directly on hardware (well, QEMU). No `main()` signature, no `libc`, no heap.
2. **Custom entry point** — ARM64 assembly sets up the stack, zeroes BSS, and jumps to Rust.
3. **Custom linker script** — we choose where in physical memory the binary lives.
4. **Custom target** — Rust's standard `aarch64-unknown-none` target needs tweaks for hypervisor use.
5. **Cross-compilation toolchain** — the host is x86_64, the target is ARM64.

By the end of this part, `make run` prints a banner to the QEMU serial console and halts cleanly. That's it. But it proves the entire toolchain works end-to-end.

## Chapters

| Chapter | Content |
|---------|---------|
| [Architecture](arch.md) | ARM64 exception levels, why EL2, how QEMU gets us there |
| [Implementation](impl.md) | `boot.S`, `linker.ld`, `rust_main()`, UART output |
| [Testing](test.md) | QEMU serial output as the first "test" |
| [Debugging Notes](debug.md) | Load address wrong, EL2 entry conditions, the fix commit |
