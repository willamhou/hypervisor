/*
 * ARM64 Hypervisor Boot Code
 * 
 * This file contains the entry point for the hypervisor.
 * It runs at EL2 (Hypervisor exception level).
 */

.section .text.boot
.global _start

_start:
    // We should be in EL2 when QEMU starts with -machine virt
    // Check current exception level (optional, for debugging)
    mrs     x0, CurrentEL
    lsr     x0, x0, #2          // Extract EL field (bits [3:2])
    cmp     x0, #2              // Should be 2 (EL2)
    b.ne    halt                // If not EL2, halt

    // Set up stack pointer for EL2
    // Stack grows downward, so point to end of stack region
    adr     x0, stack_top
    mov     sp, x0

    // Clear BSS section
    adr     x0, __bss_start
    adr     x1, __bss_end
clear_bss:
    cmp     x0, x1
    b.ge    clear_bss_done
    str     xzr, [x0], #8
    b       clear_bss
clear_bss_done:

    // Jump to Rust main function
    bl      rust_main

halt:
    // Infinite loop if we return or error
    wfe
    b       halt

// Stack (16KB)
.section .bss.stack
.align 16
stack_bottom:
    .space 16384
stack_top:

// Weak symbols for BSS
.weak __bss_start
.weak __bss_end
