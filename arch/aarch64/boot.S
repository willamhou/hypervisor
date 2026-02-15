/*
 * ARM64 Hypervisor Boot Code
 *
 * This file contains the entry point for the hypervisor.
 * It runs at EL2 (Hypervisor exception level).
 *
 * Multi-pCPU support: Secondary CPUs (Aff0 != 0) take a separate path.
 * They WFE until BOOT_READY[id] is set by the primary, then set up
 * their own stack and jump to rust_main_secondary(cpu_id).
 *
 * In single-pCPU mode, rust_main_secondary is a weak symbol that
 * resolves to halt — secondary CPUs never wake because BOOT_READY
 * is never set. The 48KB stack cost is the only overhead.
 */

.section .text.boot
.global _start

_start:
    // Read CPU ID from MPIDR_EL1.Aff0
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // x19 = cpu_id (preserved across calls)
    cbnz    x19, secondary_wait    // Non-zero → secondary CPU path

primary_boot:
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

secondary_wait:
    wfe
    adr     x1, BOOT_READY
    ldr     w2, [x1, x19, lsl #2]  // BOOT_READY[cpu_id] (u32)
    cbz     w2, secondary_wait     // Not ready → keep waiting

    // Set up per-pCPU stack: pcpu_stacks + cpu_id * 16KB (top)
    // CPU 1 → pcpu_stacks + 16KB, CPU 2 → pcpu_stacks + 32KB, etc.
    adr     x1, pcpu_stacks
    mov     x0, x19                 // cpu_id (1, 2, or 3)
    lsl     x0, x0, #14            // * 16KB
    add     sp, x1, x0             // sp = base + cpu_id * 16KB (top of this cpu's stack)

    // Pass cpu_id as first argument
    mov     x0, x19
    bl      rust_main_secondary

    // Should never return
    b       halt

// Default weak rust_main_secondary — just halts.
// Overridden by the Rust #[no_mangle] version when multi_pcpu is enabled.
.weak rust_main_secondary
.set rust_main_secondary, halt

// Per-pCPU boot-ready flags (written by primary CPU, read by secondaries)
// Each is a u32 (0 = not ready, 1 = ready). Indexed by cpu_id.
// In single-pCPU mode these are never set, so secondaries never wake.
.global BOOT_READY
.section .data
.align 4
BOOT_READY:
    .word 0     // CPU 0 (unused — primary doesn't wait)
    .word 0     // CPU 1
    .word 0     // CPU 2
    .word 0     // CPU 3

// Per-pCPU stacks for secondary CPUs (3 × 16KB)
// CPU 0 uses the existing stack_top. CPUs 1-3 use these.
.section .bss.pcpu_stacks
.align 16
.global pcpu_stacks
pcpu_stacks:
    .space 3 * 16384   // 3 × 16KB for CPUs 1-3

// Primary CPU stack (16KB)
.section .bss.stack
.align 16
stack_bottom:
    .space 16384
stack_top:

// Weak symbols for BSS
.weak __bss_start
.weak __bss_end
