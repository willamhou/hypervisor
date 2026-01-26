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
    vector_entry irq_lower_el_aarch64
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
    // The context pointer should be in a known location
    // For now, we'll use a global variable (not ideal for SMP)
    
    // Save guest registers to stack temporarily
    stp     x0, x1, [sp, #-16]!
    stp     x2, x3, [sp, #-16]!
    
    // Load the context pointer (we'll store it before entering guest)
    adrp    x0, current_vcpu_context
    add     x0, x0, :lo12:current_vcpu_context
    ldr     x0, [x0]
    
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
    
    // Call Rust exception handler
    // x0 already contains context pointer
    bl      handle_exception
    
    // Check return value: true (1) = continue, false (0) = exit
    cbz     w0, guest_exit
    
    // Continue running guest - restore context and re-enter
    // Load the context pointer again
    adrp    x0, current_vcpu_context
    add     x0, x0, :lo12:current_vcpu_context
    ldr     x0, [x0]
    
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
    
    // Restore SPSR_EL2 to enter guest at EL1
    mov     x1, #0x5
    msr     spsr_el2, x1
    
    // Restore x0, x1 last
    ldp     x0, x1, [x0, #0]
    
    // Enter guest with ERET
    eret

guest_exit:
    // Guest wants to exit - return to caller (host)
    ret

exception_no_context:
    // No valid context - this is a host exception
    // For now, just halt
    wfe
    b       exception_no_context

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
    // Save the context pointer for exception handler
    adrp    x1, current_vcpu_context
    add     x1, x1, :lo12:current_vcpu_context
    str     x0, [x1]
    
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
    
    // Restore SPSR_EL2 to enter guest at EL1
    // SPSR_EL2[3:0] = 0b0101 (EL1h)
    mov     x1, #0x5
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
 * Current vCPU context pointer
 * This is used to communicate between enter_guest and exception handlers
 */
.section .bss
.align 8
current_vcpu_context:
    .skip   8
