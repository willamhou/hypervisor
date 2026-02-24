# Testing: Serial Output as Proof of Life

## The First "Test"

At this stage there's no test framework, no assertions, no CI. The test is: does `make run` produce the expected output on the QEMU serial console?

```
$ make run
Starting QEMU...
========================================
  ARM64 Hypervisor - Milestone 0
========================================

Hello from EL2!

System Information:
  - Exception Level: EL2 (Hypervisor)
  - Architecture: AArch64
  - Target: QEMU virt machine

Project initialized successfully!
========================================
```

If you see this, the entire chain works:
- `aarch64-linux-gnu-gcc` compiled `boot.S`
- `build.rs` archived it into `libboot.a`
- Cargo cross-compiled Rust to `aarch64-unknown-none`
- The linker placed `_start` at the load address
- QEMU loaded the binary and started executing at EL2
- Assembly set up the stack and jumped to Rust
- Rust wrote bytes to the UART via inline assembly
- QEMU's PL011 emulation forwarded them to your terminal

If you see nothing — silence. No error message, no crash dump. Just a blank terminal. That's the reality of bare-metal debugging: failure is absence.

## Why Not a Real Test Framework?

We'll build one later (Part 2 adds the first HVC-based test, Part 3 adds proper test orchestration). But for now, serial output is the only feedback channel we have. The hypervisor runs on QEMU with `-nographic`, which routes PL011 UART to stdio. Every `uart_puts()` call is a primitive assertion: "I reached this point in execution."

This "printf debugging" approach stays useful even after we have a proper test harness. When something goes wrong at EL2 and the test framework itself might be broken, `uart_puts(b"GOT HERE\n")` is sometimes the only tool that works.

## Exiting QEMU

`Ctrl+A` then `X` sends QEMU's termination sequence. This is not a guest-initiated exit — it's the QEMU monitor killing the VM. Later, we add `PSCI SYSTEM_RESET` for clean shutdown, but at this stage the hypervisor just loops on WFE forever.
