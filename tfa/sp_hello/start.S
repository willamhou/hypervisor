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

.equ FFA_MSG_WAIT,              0x8400006B
.equ FFA_DIRECT_REQ_32,         0x8400006F
.equ FFA_DIRECT_RESP_32,        0x84000070
.equ FFA_MEM_RETRIEVE_REQ_32,   0x84000074
.equ FFA_MEM_RETRIEVE_RESP,     0x84000075
.equ FFA_MEM_RELINQUISH,        0x84000076
.equ MEM_TEST_MAGIC,            0xABCD0001
.equ SHARED_PAGE_MAGIC,         0xCAFEFACE

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

    /* Check for memory test trigger: x3 == MEM_TEST_MAGIC */
    ldr     x8, =MEM_TEST_MAGIC
    cmp     x3, x8
    b.eq    .Lmem_test

    /* Check for slow-path trigger: x3 == 0xDEADBEEF */
    ldr     x8, =0xDEADBEEF
    cmp     x3, x8
    b.ne    .Lfast_path

    /* Slow path: busy-loop ~2M iterations (long enough for CNTHP timer) */
    mov     x8, #0x200000       /* 2M iterations */
.Lslow_loop:
    sub     x8, x8, #1
    cbnz    x8, .Lslow_loop

    /* Slow-path response: x4 += 0x2000 (distinguishes from fast path) */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    add     x4, x4, #0x2000
    smc     #0
    b       .Lmsg_loop

.Lfast_path:
    /* Build DIRECT_RESP: echo x3,x5,x6,x7; modify x4 += 0x1000 as proof */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x2, xzr
    /* x3 passed through unchanged */
    add     x4, x4, #0x1000     /* SP signature: x4 += 0x1000 */
    /* x5-x7 passed through unchanged */
    smc     #0

    /* After ERET from SPMC with next request */
    b       .Lmsg_loop

.Lmem_test:
    /* Memory test: DIRECT_REQ with x3=MEM_TEST_MAGIC, x4=handle_lo, x5=handle_hi, x6=shared_page_ipa.
     *
     * 1. Call FFA_MEM_RETRIEVE_REQ_32 with handle from x4/x5
     * 2. Write SHARED_PAGE_MAGIC to shared page at IPA from x6
     * 3. Call FFA_MEM_RELINQUISH with handle
     * 4. Respond with DIRECT_RESP: x4 += 0x4000 (memory test signature)
     */
    /* Save response x1 (swapped src/dest) and handle + IPA across SMC calls */
    mov     x19, x1                 /* response x1 (already swapped above) */
    mov     x20, x4                 /* handle_lo */
    mov     x21, x5                 /* handle_hi */
    mov     x22, x6                 /* shared page IPA */

    /* Debug: print "[SP] MEM_TEST\r\n" */
    adr     x0, str_memtest
    bl      uart_print

    /* Step 1: FFA_MEM_RETRIEVE_REQ_32(handle_lo, handle_hi) */
    ldr     x0, =FFA_MEM_RETRIEVE_REQ_32
    mov     x1, x20                 /* handle_lo */
    mov     x2, x21                 /* handle_hi */
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* Check x0 == FFA_MEM_RETRIEVE_RESP */
    ldr     x8, =FFA_MEM_RETRIEVE_RESP
    cmp     x0, x8
    b.ne    .Lmem_test_error

    /* Debug: print "[SP] RETRIEVE OK\r\n" */
    adr     x0, str_retrieve_ok
    bl      uart_print

    /* Step 2: Write magic to shared page */
    ldr     w8, =SHARED_PAGE_MAGIC
    str     w8, [x22]

    /* Debug: print "[SP] WRITE OK\r\n" */
    adr     x0, str_write_ok
    bl      uart_print

    /* Step 3: FFA_MEM_RELINQUISH(handle_lo, handle_hi) */
    ldr     x0, =FFA_MEM_RELINQUISH
    mov     x1, x20                 /* handle_lo */
    mov     x2, x21                 /* handle_hi */
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* Step 4: Respond with DIRECT_RESP, x4 += 0x4000 as memory test signature */
    ldr     x0, =FFA_DIRECT_RESP_32
    mov     x1, x19                 /* response x1 */
    mov     x2, xzr
    ldr     x3, =MEM_TEST_MAGIC    /* echo magic back */
    add     x4, x20, #0x4000       /* handle_lo + 0x4000 as signature */
    ldr     x5, =SHARED_PAGE_MAGIC /* echo what we wrote */
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

.Lmem_test_error:
    /* RETRIEVE failed — respond with error in x4 */
    ldr     x8, =FFA_DIRECT_RESP_32
    mov     x0, x8
    mov     x1, x19                 /* response x1 */
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr                 /* x4 = 0 signals error */
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
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
str_memtest:
    .asciz "[SP] MEM_TEST\r\n"
str_retrieve_ok:
    .asciz "[SP] RETRIEVE OK\r\n"
str_write_ok:
    .asciz "[SP] WRITE OK\r\n"
