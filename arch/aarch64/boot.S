/*
 * ARM64 Hypervisor Boot Code
 * 
 * This file contains the entry point for the hypervisor.
 * It runs at EL2 (Hypervisor exception level).
 */

.section .text.boot
.global _start

_start:
    // Set up stack
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
