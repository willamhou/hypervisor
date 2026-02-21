/*
 * SP Hello — minimal Secure Partition running at S-EL1.
 *
 * Boot sequence:
 *   1. Print "[SP] Hello from S-EL1"
 *   2. Call FFA_MSG_WAIT (SMC) to signal idle to SPMC
 *   3. On wakeup (SPMC ERETs with DIRECT_REQ args in x0-x7):
 *      - Echo x3-x7 back via FFA_MSG_SEND_DIRECT_RESP
 *      - Loop back to FFA_MSG_WAIT
 *
 * Loaded at 0x0e300000 by TF-A BL2. Stack at 0x0e400000.
 */

.section .text.boot
.global _start

.equ UART_BASE,          0x09000000
.equ UARTDR,             0x000
.equ UARTFR,             0x018

.equ FFA_MSG_WAIT,       0x8400006B
.equ FFA_DIRECT_REQ_32,  0x8400006F
.equ FFA_DIRECT_RESP_32, 0x84000070

.equ SP_STACK_TOP,       0x0e400000

_start:
    /* Set up stack */
    ldr     x0, =SP_STACK_TOP
    mov     sp, x0

    /* Print banner */
    adr     x0, str_banner
    bl      uart_print

    /* Signal SPMC: SP init complete via FFA_MSG_WAIT */
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

    /* Swap source/dest in x1 for the response.
     * x1 = (source << 16) | dest → response: (dest << 16) | source */
    lsr     x8, x1, #16         /* x8 = source */
    and     x9, x1, #0xFFFF     /* x9 = dest (our SP ID) */
    orr     x1, x8, x9, lsl #16 /* x1 = (dest << 16) | source */

    /* Build DIRECT_RESP: echo x3-x7, x2=0 */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    /* x3-x7 already contain the payload — pass through */
    smc     #0

    /* After ERET from SPMC with next request */
    b       .Lmsg_loop

.Lunknown_msg:
    /* Unknown message — respond with FFA_MSG_WAIT to go idle */
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

/* ============ UART print subroutine ============ */
/* x0 = pointer to null-terminated string. Clobbers x10-x12. */
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
    .asciz "[SP] Hello from S-EL1!\r\n"
