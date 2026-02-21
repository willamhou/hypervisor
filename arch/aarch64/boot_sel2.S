/*
 * ARM64 S-EL2 SPMC Boot Code
 *
 * Entry point for the hypervisor running as BL32 (SPMC) at S-EL2.
 * SPMD at EL3 enters here with:
 *   x0 = TOS_FW_CONFIG (SPMC manifest DTB address)
 *   x1 = HW_CONFIG (hardware DTB address)
 *   x4 = core linear ID
 *
 * Only the primary CPU (MPIDR.Aff0 == 0) proceeds; secondaries halt.
 */

.section .text.boot
.global _start

_start:
    // Read CPU ID from MPIDR_EL1.Aff0
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // x19 = cpu_id
    cbnz    x19, halt              // Only primary CPU proceeds

    // Save SPMD-provided arguments in callee-saved registers
    mov     x20, x0                // x20 = TOS_FW_CONFIG (manifest DTB)
    mov     x21, x1                // x21 = HW_CONFIG (hardware DTB)
    mov     x22, x4                // x22 = Core ID

    // Set up stack
    adrp    x0, stack_top
    add     x0, x0, :lo12:stack_top
    mov     sp, x0

    // Clear BSS section
    adrp    x0, __bss_start
    add     x0, x0, :lo12:__bss_start
    adrp    x1, __bss_end
    add     x1, x1, :lo12:__bss_end
1:  cmp     x0, x1
    b.ge    2f
    str     xzr, [x0], #8
    b       1b
2:
    // Call Rust entry point
    mov     x0, x20                // manifest_addr
    mov     x1, x21                // hw_config_addr
    mov     x2, x22                // core_id
    bl      rust_main_sel2

halt:
    wfe
    b       halt

// Stack (64KB — smaller than NS-EL2, no guests to manage in minimal scope)
.section .bss.stack
.align 16
stack_bottom:
    .space 65536
stack_top:

// Weak symbols for BSS
.weak __bss_start
.weak __bss_end
