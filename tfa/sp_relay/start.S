/*
 * SP Relay — Secure Partition at S-EL1 for SP-to-SP testing.
 *
 * Boot sequence:
 *   1. Print "[SP3] Hello from S-EL1"
 *   2. Call FFA_MSG_WAIT to signal idle to SPMC
 *   3. On DIRECT_REQ:
 *      - If x3 == RELAY_MAGIC (0x00EE1A00):
 *        Forward DIRECT_REQ to target SP (x4 low 16 bits),
 *        with x5-x7 as payload. Forward response back.
 *      - Else: echo with x4 += 0x2000
 *
 * Loaded at 0x0e500000 by TF-A BL2. Stack at 0x0e600000.
 */

.section .text.boot
.global _start

.equ UART_BASE,          0x09000000
.equ UARTDR,             0x000
.equ UARTFR,             0x018

.equ FFA_MSG_WAIT,              0x8400006B
.equ FFA_DIRECT_REQ_32,         0x8400006F
.equ FFA_DIRECT_RESP_32,        0x84000070
.equ FFA_ERROR,                 0x84000060
.equ SP3_PARTITION_ID,          0x8003
.equ RELAY_MAGIC,               0x00EE1A00

.equ SP_STACK_TOP,       0x0e600000

_start:
    /* Set up stack */
    ldr     x0, =SP_STACK_TOP
    mov     sp, x0

    /* Print banner */
    adr     x0, str_banner
    bl      uart_print

    /* Signal idle to SPMC */
    ldr     w0, =FFA_MSG_WAIT
    smc     #0

    /* ---- Main dispatch loop ---- */
.Lwait_loop:
    /* Check if we received DIRECT_REQ_32 */
    ldr     w9, =FFA_DIRECT_REQ_32
    cmp     w0, w9
    b.ne    .Lunexpected

    /* Save caller info: x1 has source<<16|dest */
    mov     x20, x1         /* save original x1 (source|dest) */
    mov     x21, x3         /* save x3 (magic or payload) */
    mov     x22, x4         /* save x4 */
    mov     x23, x5         /* save x5 */
    mov     x24, x6         /* save x6 */
    mov     x25, x7         /* save x7 */

    /* Extract caller ID from x1[31:16] */
    lsr     x26, x20, #16   /* x26 = caller_id */

    /* Check if relay mode (x3 == RELAY_MAGIC) */
    ldr     w9, =RELAY_MAGIC
    cmp     w21, w9
    b.ne    .Lecho_mode

    /* ---- Relay mode: forward DIRECT_REQ to target SP ---- */
    /* Target SP ID = x4[15:0] */
    and     w27, w22, #0xFFFF   /* x27 = target_sp_id */

    /* Build DIRECT_REQ: source=SP3, dest=target */
    ldr     w0, =FFA_DIRECT_REQ_32
    ldr     w1, =SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w27         /* x1 = SP3<<16 | target */
    mov     x2, #0
    mov     x3, #0              /* no relay magic for target */
    mov     x4, x23             /* forward x5 as x4 */
    mov     x5, x24             /* forward x6 as x5 */
    mov     x6, x25             /* forward x7 as x6 */
    mov     x7, #0
    smc     #0

    /* Check response: FFA_ERROR or DIRECT_RESP */
    ldr     w9, =FFA_ERROR
    cmp     w0, w9
    b.eq    .Lrelay_error

    /* Forward callee's response back to original caller */
    mov     x28, x4             /* save callee's x4 */
    mov     x29, x5             /* save callee's x5 */
    /* x6, x7 from callee are still in registers */
    mov     x10, x6
    mov     x11, x7

    ldr     w0, =FFA_DIRECT_RESP_32
    ldr     w1, =SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w26         /* x1 = SP3<<16 | original_caller */
    mov     x2, #0
    mov     x3, #0
    mov     x4, x28             /* callee's x4 */
    mov     x5, x29             /* callee's x5 */
    mov     x6, x10             /* callee's x6 */
    mov     x7, x11             /* callee's x7 */
    smc     #0
    b       .Lwait_loop

.Lrelay_error:
    /* Forward error back to caller */
    mov     x28, x2             /* save error code */
    ldr     w0, =FFA_DIRECT_RESP_32
    ldr     w1, =SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w26         /* x1 = SP3<<16 | original_caller */
    mov     x2, #0
    mov     x3, #0
    mov     x4, x28             /* error code in x4 */
    mov     x5, #0
    mov     x6, #0
    mov     x7, #0
    smc     #0
    b       .Lwait_loop

    /* ---- Echo mode: x4 += 0x2000 ---- */
.Lecho_mode:
    add     x22, x22, #0x2000
    ldr     w0, =FFA_DIRECT_RESP_32
    ldr     w1, =SP3_PARTITION_ID
    lsl     w1, w1, #16
    orr     w1, w1, w26         /* x1 = SP3<<16 | caller */
    mov     x2, #0
    mov     x3, x21
    mov     x4, x22             /* x4 + 0x2000 */
    mov     x5, x23
    mov     x6, x24
    mov     x7, x25
    smc     #0
    b       .Lwait_loop

.Lunexpected:
    /* Unexpected call — return to MSG_WAIT */
    ldr     w0, =FFA_MSG_WAIT
    smc     #0
    b       .Lwait_loop

/* ---- UART print (NUL-terminated string in x0) ---- */
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

/* ---- String data ---- */
.section .rodata
str_banner:
    .asciz "[SP3] Hello from S-EL1 (sp_relay)\r\n"
