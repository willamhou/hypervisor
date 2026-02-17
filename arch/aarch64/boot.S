/*
 * ARM64 Hypervisor Boot Code
 *
 * This file contains the entry point for the hypervisor.
 * It runs at EL2 (Hypervisor exception level).
 *
 * Multi-pCPU support: QEMU virt uses PSCI to start secondary CPUs.
 * Secondary CPUs are powered off at boot; the primary issues real
 * PSCI CPU_ON (SMC) to QEMU firmware with secondary_entry as the
 * entry point. Each secondary reads MPIDR for its cpu_id, sets up
 * a per-CPU stack, and calls rust_main_secondary(cpu_id).
 *
 * In single-pCPU mode, rust_main_secondary is a weak symbol that
 * resolves to halt — secondary CPUs are never powered on.
 */

.section .text.boot
.global _start

_start:
    // Read CPU ID from MPIDR_EL1.Aff0
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // x19 = cpu_id (preserved across calls)
    cbnz    x19, halt              // Non-zero at initial boot = unexpected, halt

primary_boot:
    // Save DTB address from QEMU (passed in x0) before stack/BSS setup
    mov     x20, x0                    // x20 = DTB address (callee-saved)

    // Set up stack (adrp+add for >1MB BSS distance)
    adrp    x0, stack_top
    add     x0, x0, :lo12:stack_top
    mov     sp, x0

    // Clear BSS section (adrp+add for >1MB BSS distance)
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
clear_bss:
    cmp     x0, x1
    b.ge    clear_bss_done
    str     xzr, [x0], #8
    b       clear_bss
clear_bss_done:

    // Pass DTB address as first argument to rust_main
    mov     x0, x20
    bl      rust_main

halt:
    // Infinite loop if we return or error
    wfe
    b       halt

/*
 * Secondary CPU entry point — called via real PSCI CPU_ON from QEMU firmware.
 * QEMU starts the CPU at this address in EL2 with MMU off, caches off.
 * We read MPIDR for cpu_id, set up a per-CPU stack, and jump to Rust.
 */
.global secondary_entry
secondary_entry:
    // Read CPU ID from MPIDR_EL1.Aff0
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // x19 = cpu_id

    // Set up per-pCPU stack: pcpu_stacks + cpu_id * 128KB (top)
    // CPU 1 → pcpu_stacks + 128KB, CPU 2 → pcpu_stacks + 256KB, etc.
    adrp    x1, pcpu_stacks
    add     x1, x1, :lo12:pcpu_stacks
    mov     x0, x19                 // cpu_id (1, 2, or 3)
    lsl     x0, x0, #17            // * 128KB
    add     sp, x1, x0             // sp = base + cpu_id * 128KB (top of this cpu's stack)

    // Pass cpu_id as first argument
    mov     x0, x19
    bl      rust_main_secondary

    // Should never return
    b       halt

// Default weak rust_main_secondary — just halts.
// Overridden by the Rust #[no_mangle] version when multi_pcpu is enabled.
.weak rust_main_secondary
.set rust_main_secondary, halt

// Per-pCPU stacks for secondary CPUs (3 × 128KB)
// CPU 0 uses the existing stack_top. CPUs 1-3 use these.
.section .bss.pcpu_stacks
.align 16
.global pcpu_stacks
pcpu_stacks:
    .space 3 * 131072  // 3 × 128KB for CPUs 1-3

// Primary CPU stack (128KB — must accommodate DeviceManager on stack in tests)
.section .bss.stack
.align 16
stack_bottom:
    .space 131072
stack_top:

// Weak symbols for BSS
.weak __bss_start
.weak __bss_end
