/*
 * ARM64 Exception Vector Table for EL2
 *
 * This file implements the exception vector table and VM entry/exit routines.
 *
 * Exception Vector Table Layout (each entry is 0x80 bytes = 128 bytes):
 *
 * VBAR_EL2 + 0x000: Synchronous exception from Current EL with SP_EL0
 * VBAR_EL2 + 0x080: IRQ from Current EL with SP_EL0
 * VBAR_EL2 + 0x100: FIQ from Current EL with SP_EL0
 * VBAR_EL2 + 0x180: SError from Current EL with SP_EL0
 * VBAR_EL2 + 0x200: Synchronous exception from Current EL with SP_ELx
 * VBAR_EL2 + 0x280: IRQ from Current EL with SP_ELx
 * VBAR_EL2 + 0x300: FIQ from Current EL with SP_ELx
 * VBAR_EL2 + 0x380: SError from Current EL with SP_ELx
 * VBAR_EL2 + 0x400: Synchronous exception from Lower EL (AArch64)
 * VBAR_EL2 + 0x480: IRQ from Lower EL (AArch64)
 * VBAR_EL2 + 0x500: FIQ from Lower EL (AArch64)
 * VBAR_EL2 + 0x580: SError from Lower EL (AArch64)
 * VBAR_EL2 + 0x600: Synchronous exception from Lower EL (AArch32)
 * VBAR_EL2 + 0x680: IRQ from Lower EL (AArch32)
 * VBAR_EL2 + 0x700: FIQ from Lower EL (AArch32)
 * VBAR_EL2 + 0x780: SError from Lower EL (AArch32)
 *
 * Per-CPU context pointer: TPIDR_EL2 holds the current vCPU context pointer.
 * Each physical CPU has its own TPIDR_EL2, so this is inherently per-CPU
 * (no shared global needed for multi-pCPU).
 */

.section .text.exception

/*
 * Macro to create exception vector entry
 * Each entry must be exactly 128 bytes (0x80)
 */
.macro vector_entry label
.align 7  // Align to 128 bytes
\label:
    b       exception_handler
    .skip   (. - \label - 4), 0
.endm

/*
 * Macro for IRQ vector entry (branches to irq_exception_handler)
 */
.macro vector_entry_irq label
.align 7
\label:
    b       irq_exception_handler
    .skip   (. - \label - 4), 0
.endm

/*
 * Exception Vector Table
 * This must be 2KB aligned (0x800 alignment)
 */
.align 11
.global exception_vector_table
exception_vector_table:
    // Current EL with SP_EL0
    vector_entry sync_current_el_sp0
    vector_entry irq_current_el_sp0
    vector_entry fiq_current_el_sp0
    vector_entry serror_current_el_sp0

    // Current EL with SP_ELx
    vector_entry sync_current_el_spx
    vector_entry irq_current_el_spx
    vector_entry fiq_current_el_spx
    vector_entry serror_current_el_spx

    // Lower EL (AArch64) - This is where guest exceptions come from
    vector_entry sync_lower_el_aarch64
    vector_entry_irq irq_lower_el_aarch64
    vector_entry fiq_lower_el_aarch64
    vector_entry serror_lower_el_aarch64

    // Lower EL (AArch32) - Not supported
    vector_entry sync_lower_el_aarch32
    vector_entry irq_lower_el_aarch32
    vector_entry fiq_lower_el_aarch32
    vector_entry serror_lower_el_aarch32

/*
 * Exception Handler
 *
 * When an exception occurs while the guest is running:
 * 1. Save guest context
 * 2. Call Rust handler
 * 3. Restore guest context
 * 4. Return to guest or host
 */
exception_handler:
    // We need to save the guest context
    // TPIDR_EL2 holds the per-CPU context pointer (set by enter_guest)

    // Save guest registers to stack temporarily
    stp     x0, x1, [sp, #-16]!
    stp     x2, x3, [sp, #-16]!

    // Load the context pointer from per-CPU TPIDR_EL2
    mrs     x0, tpidr_el2

    // Check if context pointer is valid
    cbz     x0, exception_no_context

    // Save general purpose registers to VcpuContext
    // VcpuContext layout: gp_regs (31*8 bytes), sys_regs, sp, pc

    // Restore x2, x3 from stack and save to context
    ldp     x2, x3, [sp], #16
    stp     x2, x3, [x0, #16]   // x2, x3

    // Restore x0, x1 from stack and save to context
    ldp     x2, x3, [sp], #16
    stp     x2, x3, [x0, #0]    // x0, x1

    // Save x4-x30
    stp     x4, x5, [x0, #32]
    stp     x6, x7, [x0, #48]
    stp     x8, x9, [x0, #64]
    stp     x10, x11, [x0, #80]
    stp     x12, x13, [x0, #96]
    stp     x14, x15, [x0, #112]
    stp     x16, x17, [x0, #128]
    stp     x18, x19, [x0, #144]
    stp     x20, x21, [x0, #160]
    stp     x22, x23, [x0, #176]
    stp     x24, x25, [x0, #192]
    stp     x26, x27, [x0, #208]
    stp     x28, x29, [x0, #224]
    str     x30, [x0, #240]      // x30 (LR)

    // Save system registers (offset 248 = 31*8)
    // We'll save key system registers
    mrs     x2, sp_el1
    str     x2, [x0, #248]       // sp_el1

    mrs     x2, elr_el1
    str     x2, [x0, #256]       // elr_el1

    mrs     x2, spsr_el1
    str     x2, [x0, #264]       // spsr_el1

    // Save SP and PC
    // Offset calculation:
    // GP regs: 31*8 = 248
    // Sys regs: 17*8 = 136
    // Total before sp: 248 + 136 = 384
    mov     x2, sp
    str     x2, [x0, #384]       // sp

    mrs     x2, elr_el2
    str     x2, [x0, #392]       // pc

    // Save SPSR_EL2 (guest PSTATE) to context
    mrs     x2, spsr_el2
    str     x2, [x0, #400]       // spsr_el2

    // Call Rust exception handler
    // x0 already contains context pointer
    bl      handle_exception

    // Check return value: true (1) = continue, false (0) = exit
    cbz     w0, guest_exit

    // Continue running guest - restore context and re-enter
    // Load the context pointer from per-CPU TPIDR_EL2
    mrs     x0, tpidr_el2

    // Restore guest general purpose registers
    ldp     x2, x3, [x0, #16]
    ldp     x4, x5, [x0, #32]
    ldp     x6, x7, [x0, #48]
    ldp     x8, x9, [x0, #64]
    ldp     x10, x11, [x0, #80]
    ldp     x12, x13, [x0, #96]
    ldp     x14, x15, [x0, #112]
    ldp     x16, x17, [x0, #128]
    ldp     x18, x19, [x0, #144]
    ldp     x20, x21, [x0, #160]
    ldp     x22, x23, [x0, #176]
    ldp     x24, x25, [x0, #192]
    ldp     x26, x27, [x0, #208]
    ldp     x28, x29, [x0, #224]
    ldr     x30, [x0, #240]

    // Restore system registers
    ldr     x1, [x0, #248]       // sp_el1
    msr     sp_el1, x1

    ldr     x1, [x0, #256]       // elr_el1
    msr     elr_el1, x1

    ldr     x1, [x0, #264]       // spsr_el1
    msr     spsr_el1, x1

    // Restore PC (into ELR_EL2 for ERET)
    ldr     x1, [x0, #392]       // pc (offset 392)
    msr     elr_el2, x1

    // Restore SPSR_EL2 from context (may have been modified by handler)
    ldr     x1, [x0, #400]       // spsr_el2
    msr     spsr_el2, x1

    // Restore x0, x1 last
    ldp     x0, x1, [x0, #0]

    // Enter guest with ERET
    eret

guest_exit:
    // Guest wants to exit - return to enter_guest's caller
    // We need to restore the host context that was saved by enter_guest

    // Check if this is a WFI exit (ExitReason::WfiWfe)
    // Read ESR_EL2 and check EC field
    mrs     x10, esr_el2
    lsr     x10, x10, #26           // Extract EC (bits 31:26)
    and     x10, x10, #0x3F
    cmp     x10, #0x1               // EC = 0x01 for WFI/WFE
    beq     guest_exit_wfi

    // Normal exit (not WFI)
    // Restore host callee-saved registers
    ldp     x19, x20, [sp], #16
    ldp     x21, x22, [sp], #16
    ldp     x23, x24, [sp], #16
    ldp     x25, x26, [sp], #16
    ldp     x27, x28, [sp], #16
    ldp     x29, x30, [sp], #16

    // Return 0 (success) to enter_guest's caller
    mov     x0, #0
    ret

guest_exit_irq:
    // IRQ-triggered exit (e.g., preemptive timer).
    // ESR_EL2 is NOT valid for IRQs, so skip the EC check.
    // PC is already correctly saved in VcpuContext - do not advance it.
    ldp     x19, x20, [sp], #16
    ldp     x21, x22, [sp], #16
    ldp     x23, x24, [sp], #16
    ldp     x25, x26, [sp], #16
    ldp     x27, x28, [sp], #16
    ldp     x29, x30, [sp], #16

    // Return 0 (normal exit) to enter_guest's caller
    mov     x0, #0
    ret

guest_exit_wfi:
    // WFI exit - PC already advanced by Rust handler (context.pc += 4).
    // Do NOT advance PC here to avoid double-advance.

    // Restore host callee-saved registers
    ldp     x19, x20, [sp], #16
    ldp     x21, x22, [sp], #16
    ldp     x23, x24, [sp], #16
    ldp     x25, x26, [sp], #16
    ldp     x27, x28, [sp], #16
    ldp     x29, x30, [sp], #16

    // Return 1 (WFI) to enter_guest's caller
    mov     x0, #1
    ret

exception_no_context:
    // No valid vCPU context (TPIDR_EL2=0) — host-level fault.
    // Restore stack from the two pushes, then jump to diagnostic printer.
    ldp     x2, x3, [sp], #16
    ldp     x0, x1, [sp], #16
    b       fault_diag_print

/*
 * IRQ Exception Handler (for interrupts from Lower EL)
 *
 * When a physical IRQ fires while the guest is running (e.g., virtual timer),
 * with IMO=1 in HCR_EL2, the IRQ traps here. Unlike sync exceptions,
 * ESR_EL2 is NOT valid for IRQs - we must acknowledge via ICC_IAR1_EL1.
 */
irq_exception_handler:
    // Save guest registers to stack temporarily
    stp     x0, x1, [sp, #-16]!
    stp     x2, x3, [sp, #-16]!

    // Load the context pointer from per-CPU TPIDR_EL2
    mrs     x0, tpidr_el2

    // Check if context pointer is valid
    cbz     x0, exception_no_context

    // Save general purpose registers to VcpuContext
    ldp     x2, x3, [sp], #16
    stp     x2, x3, [x0, #16]   // x2, x3

    ldp     x2, x3, [sp], #16
    stp     x2, x3, [x0, #0]    // x0, x1

    stp     x4, x5, [x0, #32]
    stp     x6, x7, [x0, #48]
    stp     x8, x9, [x0, #64]
    stp     x10, x11, [x0, #80]
    stp     x12, x13, [x0, #96]
    stp     x14, x15, [x0, #112]
    stp     x16, x17, [x0, #128]
    stp     x18, x19, [x0, #144]
    stp     x20, x21, [x0, #160]
    stp     x22, x23, [x0, #176]
    stp     x24, x25, [x0, #192]
    stp     x26, x27, [x0, #208]
    stp     x28, x29, [x0, #224]
    str     x30, [x0, #240]      // x30 (LR)

    // Save system registers
    mrs     x2, sp_el1
    str     x2, [x0, #248]       // sp_el1

    mrs     x2, elr_el1
    str     x2, [x0, #256]       // elr_el1

    mrs     x2, spsr_el1
    str     x2, [x0, #264]       // spsr_el1

    // Save SP and PC
    mov     x2, sp
    str     x2, [x0, #384]       // sp

    mrs     x2, elr_el2
    str     x2, [x0, #392]       // pc

    // Save SPSR_EL2 (guest PSTATE) to context
    mrs     x2, spsr_el2
    str     x2, [x0, #400]       // spsr_el2

    // Call Rust IRQ handler
    bl      handle_irq_exception

    // Check return value: true (1) = continue, false (0) = exit
    // IRQ exits must NOT go through guest_exit (which reads stale ESR_EL2).
    // Use guest_exit_irq which returns 0 without checking ESR_EL2.
    cbz     w0, guest_exit_irq

    // Restore context and re-enter guest
    mrs     x0, tpidr_el2

    ldp     x2, x3, [x0, #16]
    ldp     x4, x5, [x0, #32]
    ldp     x6, x7, [x0, #48]
    ldp     x8, x9, [x0, #64]
    ldp     x10, x11, [x0, #80]
    ldp     x12, x13, [x0, #96]
    ldp     x14, x15, [x0, #112]
    ldp     x16, x17, [x0, #128]
    ldp     x18, x19, [x0, #144]
    ldp     x20, x21, [x0, #160]
    ldp     x22, x23, [x0, #176]
    ldp     x24, x25, [x0, #192]
    ldp     x26, x27, [x0, #208]
    ldp     x28, x29, [x0, #224]
    ldr     x30, [x0, #240]

    // Restore system registers
    ldr     x1, [x0, #248]       // sp_el1
    msr     sp_el1, x1

    ldr     x1, [x0, #256]       // elr_el1
    msr     elr_el1, x1

    ldr     x1, [x0, #264]       // spsr_el1
    msr     spsr_el1, x1

    // Restore PC
    ldr     x1, [x0, #392]       // pc
    msr     elr_el2, x1

    // Restore SPSR_EL2 from context (handler may have cleared I bit)
    ldr     x1, [x0, #400]       // spsr_el2
    msr     spsr_el2, x1

    // Restore x0, x1 last
    ldp     x0, x1, [x0, #0]

    // Return to guest
    eret

/*
 * Enter Guest
 *
 * extern "C" fn enter_guest(context: *mut VcpuContext) -> u64;
 *
 * Arguments:
 *   x0 = pointer to VcpuContext
 *
 * Returns:
 *   x0 = 0 (normal exit)
 */
.global enter_guest
enter_guest:
    // Save the context pointer in per-CPU TPIDR_EL2.
    // Each physical CPU has its own TPIDR_EL2, so this is safe for multi-pCPU.
    msr     tpidr_el2, x0

    // Save host context (callee-saved registers x19-x30, sp)
    stp     x29, x30, [sp, #-16]!
    stp     x27, x28, [sp, #-16]!
    stp     x25, x26, [sp, #-16]!
    stp     x23, x24, [sp, #-16]!
    stp     x21, x22, [sp, #-16]!
    stp     x19, x20, [sp, #-16]!

    // Restore guest general purpose registers
    ldp     x2, x3, [x0, #16]
    ldp     x4, x5, [x0, #32]
    ldp     x6, x7, [x0, #48]
    ldp     x8, x9, [x0, #64]
    ldp     x10, x11, [x0, #80]
    ldp     x12, x13, [x0, #96]
    ldp     x14, x15, [x0, #112]
    ldp     x16, x17, [x0, #128]
    ldp     x18, x19, [x0, #144]
    ldp     x20, x21, [x0, #160]
    ldp     x22, x23, [x0, #176]
    ldp     x24, x25, [x0, #192]
    ldp     x26, x27, [x0, #208]
    ldp     x28, x29, [x0, #224]
    ldr     x30, [x0, #240]

    // Restore system registers
    ldr     x1, [x0, #248]       // sp_el1
    msr     sp_el1, x1

    ldr     x1, [x0, #256]       // elr_el1
    msr     elr_el1, x1

    ldr     x1, [x0, #264]       // spsr_el1
    msr     spsr_el1, x1

    // Restore PC (into ELR_EL2 for ERET)
    ldr     x1, [x0, #392]       // pc (offset 392)
    msr     elr_el2, x1

    // Restore SPSR_EL2 from context (default 0x3c5 = DAIF masked + EL1h)
    ldr     x1, [x0, #400]       // spsr_el2
    msr     spsr_el2, x1

    // Restore x0, x1 last
    ldp     x0, x1, [x0, #0]

    // Enter guest with ERET
    eret

    // When guest exits (exception), we return here
    // Restore host context
    ldp     x19, x20, [sp], #16
    ldp     x21, x22, [sp], #16
    ldp     x23, x24, [sp], #16
    ldp     x25, x26, [sp], #16
    ldp     x27, x28, [sp], #16
    ldp     x29, x30, [sp], #16

    // Return 0 (success)
    mov     x0, #0
    ret

/*
 * Diagnostic fault handler for exceptions when TPIDR_EL2=0 (no vCPU context).
 * Prints ESR_EL2, ELR_EL2, FAR_EL2, HPFAR_EL2 to UART for debugging.
 * Used during S-EL2 SPMC boot to diagnose Data Aborts on secure memory.
 */
fault_diag_print:
    mov     x10, #0x09000000        // PL011 UART base

    // Print "[FAULT] exception_no_context\n"
    adr     x11, .Lfault_msg
    bl      .Luart_str

    // Print "ESR_EL2  = 0x"
    adr     x11, .Lesr_msg
    bl      .Luart_str
    mrs     x12, esr_el2
    bl      .Luart_hex64

    // Print "ELR_EL2  = 0x"
    adr     x11, .Lelr_msg
    bl      .Luart_str
    mrs     x12, elr_el2
    bl      .Luart_hex64

    // Print "FAR_EL2  = 0x"
    adr     x11, .Lfar_msg
    bl      .Luart_str
    mrs     x12, far_el2
    bl      .Luart_hex64

    // Print "HPFAR_EL2= 0x"
    adr     x11, .Lhpfar_msg
    bl      .Luart_str
    mrs     x12, hpfar_el2
    bl      .Luart_hex64

    // Halt
1:  wfe
    b       1b

/*
 * Print null-terminated string at x11 to UART at x10.
 * Clobbers: x13
 */
.Luart_str:
2:  ldrb    w13, [x11], #1
    cbz     w13, 3f
    str     w13, [x10]              // UARTDR at offset 0
    b       2b
3:  ret

/*
 * Print 64-bit value in x12 as hex to UART at x10, followed by newline.
 * Clobbers: x13, x14, x15
 */
.Luart_hex64:
    mov     x14, #60                // shift = 60 (16 nibbles)
4:  lsr     x13, x12, x14
    and     x13, x13, #0xF
    cmp     x13, #10
    b.lt    5f
    add     x13, x13, #('a' - 10)
    b       6f
5:  add     x13, x13, #'0'
6:  str     w13, [x10]
    subs    x14, x14, #4
    b.ge    4b
    // newline
    mov     w13, #'\n'
    str     w13, [x10]
    ret

// String data (outside vector table, no alignment issues)
.Lfault_msg:
    .asciz  "[FAULT] exception_no_context\n"
.Lesr_msg:
    .asciz  "ESR_EL2  = 0x"
.Lelr_msg:
    .asciz  "ELR_EL2  = 0x"
.Lfar_msg:
    .asciz  "FAR_EL2  = 0x"
.Lhpfar_msg:
    .asciz  "HPFAR_EL2= 0x"
