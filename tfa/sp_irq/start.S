/*
 * SP IRQ — Secure Partition at S-EL1 with interrupt handling.
 *
 * Boot sequence:
 *   1. Set up stack, install VBAR_EL1, enable virtual Group 1 interrupts
 *   2. Print "[SP2] Hello from S-EL1"
 *   3. Call FFA_MSG_WAIT to signal idle to SPMC
 *   4. On DIRECT_REQ:
 *      - Fast path: x4 += 0x1000, respond
 *      - IRQ slow path (x3 == 0xDEADBEEF): busy-loop until virtual IRQ,
 *        respond with x4 += 0x2000, x5 = INTID (29)
 *
 * Virtual interrupt injection flow:
 *   SPMC sets HCR_EL2.VI → VBAR_EL1+0x280 → HVC(0xFF04) → clears VI
 *
 * IRQ handler (VBAR_EL1 + 0x280, Current EL SPx IRQ):
 *   HVC #0 with x0=0xFF04 (HF_INTERRUPT_GET) → SPMC returns INTID in x0.
 *   Store captured_intid, set irq_handled=1, ERET back to busy-loop.
 *
 * Loaded at 0x0e400000 by TF-A BL2. Stack at 0x0e500000.
 */

.section .text.boot
.global _start

.equ UART_BASE,          0x09000000
.equ UARTDR,             0x000
.equ UARTFR,             0x018

.equ FFA_MSG_WAIT,       0x8400006B
.equ FFA_DIRECT_REQ_32,  0x8400006F
.equ FFA_DIRECT_RESP_32, 0x84000070

.equ SP2_STACK_TOP,      0x0e500000

/* Secure Physical Timer: INTID 29 (PPI) — IRQ delivery */
.equ CNTPS_INTID,        29

_start:
    /* Set up stack */
    ldr     x0, =SP2_STACK_TOP
    mov     sp, x0

    /* Install exception vector table (VBAR_EL1) */
    adr     x0, _vectors
    msr     vbar_el1, x0
    isb

    /* Enable virtual Group 1 interrupts for IRQ reception */
    mov     x0, #0xFF
    msr     icc_pmr_el1, x0        /* Allow all priorities */
    isb
    mov     x0, #1
    msr     icc_igrpen1_el1, x0    /* Enable Group 1 */
    isb

    /* Print banner */
    adr     x0, str_banner
    bl      uart_print

    /* Signal SPMC: SP2 init complete via FFA_MSG_WAIT */
    ldr     x0, =FFA_MSG_WAIT
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* After ERET from SPMC, x0-x7 contain the DIRECT_REQ.
     * Fall through to message loop. */

.Lmsg_loop:
    /* Check if this is a DIRECT_REQ_32 */
    ldr     x8, =FFA_DIRECT_REQ_32
    cmp     x0, x8
    b.ne    .Lunknown_msg

    /* Swap source/dest in x1 for the response */
    lsr     x8, x1, #16
    and     x9, x1, #0xFFFF
    orr     x1, x8, x9, lsl #16

    /* Check for IRQ slow-path trigger: x3 == 0xDEADBEEF */
    ldr     x8, =0xDEADBEEF
    cmp     x3, x8
    b.eq    .Lslow_irq

    /* Fast path: x4 += 0x1000, respond */
    b       .Lfast_path

    /* === IRQ slow path: wait for virtual IRQ === */
.Lslow_irq:
    /* Clear irq_handled flag */
    adr     x8, irq_handled
    str     xzr, [x8]

    /* Clear captured INTID */
    adr     x8, captured_intid
    str     xzr, [x8]

    /* Unmask IRQs (DAIFClr #2 = unmask IRQ bit) */
    msr     daifclr, #2
    isb

    /*
     * NOTE: We do NOT arm the CNTPS (Secure Physical Timer) here.
     * When SCR_EL3.ST=0 (TF-A default), CNTPS_*_EL1 accesses from
     * S-EL1 trap to EL3, causing a hang (EL3 has no handler).
     *
     * Instead, the SPMC at S-EL2 uses CNTHP (Hypervisor Physical Timer)
     * as a poll timer. When CNTHP fires, the S-EL2 IRQ handler detects
     * that SP2 owns INTID 29, queues it, and sets HCR_EL2.VI=1.
     * On ERET, hardware auto-vectors to our IRQ handler (VBAR+0x280).
     * We call HVC (HF_INTERRUPT_GET) to retrieve the INTID from SPMC.
     */

    /* Busy-loop until virtual IRQ handler sets irq_handled */
    adr     x8, irq_handled
    mov     x10, #0x4000000        /* safety: max iterations (64M — must exceed CNHP 10ms) */
.Lwait_irq:
    ldr     x9, [x8]
    cbnz    x9, .Lgot_irq
    sub     x10, x10, #1
    cbz     x10, .Lgot_irq         /* timeout: proceed anyway */
    b       .Lwait_irq

.Lgot_irq:
    /* Re-mask IRQs */
    msr     daifset, #2
    isb

    /* Build response: x4 += 0x2000, x5 = captured INTID */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    add     x4, x4, #0x2000
    adr     x8, captured_intid
    ldr     x5, [x8]
    smc     #0
    b       .Lmsg_loop

.Lfast_path:
    /* Build DIRECT_RESP: x4 += 0x1000, echo rest */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    add     x4, x4, #0x1000
    smc     #0
    b       .Lmsg_loop

.Lunknown_msg:
    /* Unknown message — respond with FFA_MSG_WAIT */
    ldr     x0, =FFA_MSG_WAIT
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

/* ============ Exception vector table (aligned to 2KB) ============ */
.balign 2048
_vectors:
    /* Current EL, SP0, Sync */
    b       .Lvec_unhandled
    .balign 128
    /* Current EL, SP0, IRQ */
    b       .Lvec_unhandled
    .balign 128
    /* Current EL, SP0, FIQ */
    b       .Lvec_unhandled
    .balign 128
    /* Current EL, SP0, SError */
    b       .Lvec_unhandled

    .balign 128
    /* Current EL, SPx, Sync */
    b       .Lvec_unhandled
    .balign 128
    /* Current EL, SPx, IRQ — virtual IRQ handler (HCR_EL2.VI) */
    b       .Lirq_handler
    .balign 128
    /* Current EL, SPx, FIQ */
    b       .Lvec_unhandled
    .balign 128
    /* Current EL, SPx, SError */
    b       .Lvec_unhandled

    .balign 128
    /* Lower EL, AArch64, Sync */
    b       .Lvec_unhandled
    .balign 128
    /* Lower EL, AArch64, IRQ */
    b       .Lvec_unhandled
    .balign 128
    /* Lower EL, AArch64, FIQ */
    b       .Lvec_unhandled
    .balign 128
    /* Lower EL, AArch64, SError */
    b       .Lvec_unhandled

    .balign 128
    /* Lower EL, AArch32, Sync */
    b       .Lvec_unhandled
    .balign 128
    /* Lower EL, AArch32, IRQ */
    b       .Lvec_unhandled
    .balign 128
    /* Lower EL, AArch32, FIQ */
    b       .Lvec_unhandled
    .balign 128
    /* Lower EL, AArch32, SError */
    b       .Lvec_unhandled

/* ============ IRQ handler (Current EL, SPx) ============ */
.Lirq_handler:
    /*
     * Hardware auto-vectors here because SPMC set HCR_EL2.VI=1.
     * Call HF_INTERRUPT_GET (HVC, x0=0xFF04) to retrieve the pending
     * INTID from the SPMC. SPMC returns INTID in x0 and clears VI.
     */
    stp     x0, x1, [sp, #-16]!

    /* HF_INTERRUPT_GET: ask SPMC for the pending virtual INTID */
    ldr     x0, =0xFF04
    hvc     #0
    /* x0 = INTID (e.g. 29) or 0xFFFFFFFF if none */

    /* Store captured INTID */
    adr     x1, captured_intid
    str     x0, [x1]

    /* Set irq_handled = 1 */
    adr     x1, irq_handled
    mov     x0, #1
    str     x0, [x1]

    ldp     x0, x1, [sp], #16
    eret

.Lvec_unhandled:
    /* Infinite loop on unhandled exception */
    wfe
    b       .Lvec_unhandled

/* ============ Data section ============ */
.section .data
.balign 8
irq_handled:
    .quad   0
captured_intid:
    .quad   0

/* ============ UART print subroutine ============ */
.section .text
uart_print:
    ldr     x10, =UART_BASE
.Lprint_loop:
    ldrb    w11, [x0], #1
    cbz     w11, .Lprint_done
.Lprint_wait:
    ldr     w12, [x10, #UARTFR]
    tbnz    w12, #5, .Lprint_wait
    str     w11, [x10, #UARTDR]
    b       .Lprint_loop
.Lprint_done:
    ret

/* ============ String data ============ */
.section .rodata
str_banner:
    .asciz "[SP2] Hello from S-EL1!\r\n"
