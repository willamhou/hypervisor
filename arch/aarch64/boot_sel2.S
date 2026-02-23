/*
 * ARM64 S-EL2 SPMC Boot Code
 *
 * Entry point for the hypervisor running as BL32 (SPMC) at S-EL2.
 * SPMD at EL3 enters here with:
 *   x0 = TOS_FW_CONFIG (SPMC manifest DTB address)
 *   x1 = HW_CONFIG (hardware DTB address)
 *   x4 = core linear ID
 *
 * Only the primary CPU (MPIDR.Aff0 == 0) proceeds; secondaries wait.
 */

.section .text.boot
.global _start

_start:
    // Read CPU ID from MPIDR_EL1.Aff0
    mrs     x19, MPIDR_EL1
    and     x19, x19, #0xFF        // x19 = cpu_id
    cbnz    x19, secondary_wait    // Only primary CPU proceeds

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

secondary_wait:
    wfe
    b       secondary_wait

// ── Secondary warm-boot entry (registered via FFA_SECONDARY_EP_REGISTER) ──
// SPMD enters here on PSCI CPU_ON with:
//   x0 = TOS_FW_CONFIG (manifest DTB)
//   x1 = HW_CONFIG
//   x4 = core linear ID
.global secondary_entry_sel2
secondary_entry_sel2:
    // x4 = core_id from SPMD
    // Set per-CPU stack: base + ((core_id - 1) * 16KB) + 16KB
    // core_id is 1-based for secondaries (CPU1=1, CPU2=2, CPU3=3)
    sub     x5, x4, #1              // 1->0, 2->1, 3->2
    adrp    x6, sel2_pcpu_stacks
    add     x6, x6, :lo12:sel2_pcpu_stacks
    mov     x7, #(16 * 1024)        // 16KB per CPU
    mul     x8, x5, x7
    add     x8, x8, x6              // x8 = base + index * 16KB
    add     sp, x8, x7              // sp = top of stack (grows down)

    // Save arguments in callee-saved registers
    mov     x20, x0                 // manifest_addr
    mov     x21, x1                 // hw_config_addr
    mov     x22, x4                 // core_id

    // Call Rust secondary entry
    mov     x0, x20
    mov     x1, x21
    mov     x2, x22
    bl      rust_main_sel2_secondary

    // Should not return; halt if it does
    b       secondary_wait

// Stack (64KB — smaller than NS-EL2, no guests to manage in minimal scope)
.section .bss.stack
.align 16
stack_bottom:
    .space 65536
stack_top:

// Per-CPU stacks for secondary CPUs (3 CPUs x 16KB = 48KB)
// CPU1 uses offset 0, CPU2 uses offset 16K, CPU3 uses offset 32K
.section .bss.sel2_pcpu_stacks
.align 16
sel2_pcpu_stacks:
    .space 3 * 16 * 1024

// Weak symbols for BSS
.weak __bss_start
.weak __bss_end
