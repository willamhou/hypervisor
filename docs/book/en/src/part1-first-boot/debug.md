# Debugging Notes: What Went Wrong

Two commits to get "Hello from EL2!" — the scaffolding commit (`609459b`) and the fix commit (`b2ff49f`). What went wrong in between?

## Bug 1: Wrong Load Address

The initial linker script set the base address to `0x80000000`:

```ld
. = 0x80000000;   /* "typical for EL2" — wrong */
```

QEMU's `virt` machine places RAM at `0x40000000`. When using `-kernel`, QEMU loads the binary to the start of RAM. Our binary expected to be at 0x80000000 but was actually at 0x40000000 — every `adr` instruction computed wrong addresses. Stack setup pointed to garbage memory. BSS zeroing corrupted random regions.

**Fix**: change the linker base to `0x40000000`.

```ld
. = 0x40000000;   /* QEMU virt: RAM starts here */
```

**Lesson**: the linker script's base address must match where the binary is actually loaded. With `-kernel`, QEMU loads to RAM start. This seems obvious in retrospect, but when debugging bare-metal, "obviously wrong" and "correct" produce the same symptom: silence.

## Bug 2: Over-Engineered UART Driver

The first commit included a full PL011 UART driver (`src/uart.rs`, 112 lines):
- `Uart` struct with `read_reg`/`write_reg` via `read_volatile`/`write_volatile`
- TX FIFO full check (busy-wait on `UART_FR.TXFF`)
- `fmt::Write` trait implementation
- `print!`/`println!` macros

This linked but produced no output. The problem: `core::fmt` machinery in `no_std` with our custom target triggered code generation that didn't work with the minimal runtime we had. The `println!` macro expanded to complex formatting code that couldn't run before we had a proper memory model.

**Fix**: replace the entire UART driver with 10 lines of inline assembly:

```rust
fn uart_puts(s: &[u8]) {
    unsafe {
        let uart_base = 0x09000000usize;
        for &byte in s {
            core::arch::asm!(
                "str {val:w}, [{addr}]",
                addr = in(reg) uart_base,
                val = in(reg) byte as u32,
                options(nostack),
            );
        }
    }
}
```

No volatile reads, no FIFO checks, no formatting traits. Just `str` to the UART data register. QEMU's PL011 never says "FIFO full" for reasonable output rates, so the busy-wait was unnecessary.

**Lesson**: in bare-metal, start with the absolute minimum that could work. Add complexity only after the simple version runs. The sophisticated UART driver was correct code, but it was too much for a system that couldn't even set up its stack yet.

## Bug 3: EL2 Check Removed

The original `boot.S` verified `CurrentEL == 2` and branched to `halt` if not. This was removed in the fix commit. Why?

The check itself was fine. But it added 4 instructions before the real work (stack setup) and, more importantly, it was untestable — if we're not at EL2, there's no UART output to tell us. The `halt` loop and the "working correctly" loop look the same from outside: nothing on the serial console.

**Lesson**: don't add runtime checks when the failure mode is indistinguishable from the success mode. Instead, validate assumptions with documentation and QEMU flags (`virtualization=on`).

## The AI Collaboration Story

This milestone was where the AI pair programming workflow was still being established. The first commit was largely Claude-generated: project structure, Makefile, boot.S, UART driver, custom target spec. It was a reasonable starting point — all the right pieces were there.

But it didn't work. The load address was wrong, and the UART driver was over-engineered. The fix commit shows the debugging process: strip everything down to the minimum, fix one thing at a time.

This pattern repeated throughout the project: AI generates a comprehensive first draft, human debugs the integration issues. The AI was good at "here's how a PL011 UART driver works" but wrong about "where does QEMU load the binary." The architecture knowledge was solid; the platform-specific details needed verification.
