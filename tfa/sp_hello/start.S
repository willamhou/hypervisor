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
.equ FFA_DIRECT_REQ_64,         0xC400006F
.equ FFA_DIRECT_RESP_64,        0xC4000070
.equ FFA_MEM_RETRIEVE_REQ_32,   0x84000074
.equ FFA_MEM_RETRIEVE_RESP,     0x84000075
.equ FFA_MEM_RELINQUISH,        0x84000076
.equ FFA_MEM_SHARE_32,          0x84000073
.equ FFA_MEM_RECLAIM,           0x84000077
.equ FFA_SUCCESS_32,            0x84000061
.equ MEM_TEST_MAGIC,            0xABCD0001
.equ SHARED_PAGE_MAGIC,         0xCAFEFACE
.equ SP_TO_SP_SHARE_MAGIC,      0xABCD0002
.equ SP_TO_SP_RECLAIM_MAGIC,    0xABCD0004
.equ SP1_PARTITION_ID,          0x8001
.equ SP1_SHARE_PAGE,            0x0e3F0000
.equ SP1_SHARE_WRITTEN,         0xFACE0001

.equ SP_STACK_TOP,       0x0e400000

_start:
    /* Set up stack */
    ldr     x0, =SP_STACK_TOP
    mov     sp, x0

    /* Install EL1 exception vector table */
    adr     x0, sp_vector_table
    msr     vbar_el1, x0
    isb

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
    /* Check if this is a DIRECT_REQ_32 or DIRECT_REQ_64 */
    ldr     x8, =FFA_DIRECT_REQ_32
    cmp     x0, x8
    b.eq    .Ldirect_req
    ldr     x8, =FFA_DIRECT_REQ_64
    cmp     x0, x8
    b.ne    .Lunknown_msg

.Ldirect_req:
    /* Save incoming function ID to determine response type.
     * x15 = 0 for 32-bit REQ, 1 for 64-bit REQ */
    ldr     x8, =FFA_DIRECT_REQ_64
    cmp     x0, x8
    cset    x15, eq

    /* Swap source/dest in x1 for the response.
     * x1 = (source << 16) | dest → response: (dest << 16) | source */
    lsr     x8, x1, #16         /* x8 = source */
    and     x9, x1, #0xFFFF     /* x9 = dest (our SP ID) */
    orr     x1, x8, x9, lsl #16 /* x1 = (dest << 16) | source */

    /* Check for memory test trigger: x3 == MEM_TEST_MAGIC */
    ldr     x8, =MEM_TEST_MAGIC
    cmp     x3, x8
    b.eq    .Lmem_test

    /* Check for SP-to-SP share test: x3 == SP_TO_SP_SHARE_MAGIC */
    ldr     x8, =SP_TO_SP_SHARE_MAGIC
    cmp     x3, x8
    b.eq    .Lshare_test

    /* Check for SP-to-SP reclaim test: x3 == SP_TO_SP_RECLAIM_MAGIC */
    ldr     x8, =SP_TO_SP_RECLAIM_MAGIC
    cmp     x3, x8
    b.eq    .Lreclaim_test

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
    bl      .Lload_resp_id       /* x0 = matching RESP function ID */
    mov     x2, xzr
    add     x4, x4, #0x2000
    smc     #0
    b       .Lmsg_loop

.Lfast_path:
    /* Build DIRECT_RESP: echo x3,x5,x6,x7; modify x4 += 0x1000 as proof */
    bl      .Lload_resp_id       /* x0 = matching RESP function ID */
    mov     x2, xzr
    /* x3 passed through unchanged */
    add     x4, x4, #0x1000     /* SP signature: x4 += 0x1000 */
    /* x5-x7 passed through unchanged */
    smc     #0

    /* After ERET from SPMC with next request */
    b       .Lmsg_loop

/* Helper: load matching DIRECT_RESP function ID based on x15 (0=32bit, 1=64bit) */
.Lload_resp_id:
    cbz     x15, 1f
    ldr     x0, =FFA_DIRECT_RESP_64
    ret
1:
    ldr     x0, =FFA_DIRECT_RESP_32
    ret

.Lmem_test:
    /* Memory test: DIRECT_REQ with x3=MEM_TEST_MAGIC, x4=handle_lo, x5=handle_hi, x6=shared_page_ipa.
     *
     * 1. Call FFA_MEM_RETRIEVE_REQ_32 with handle from x4/x5
     * 2. Write SHARED_PAGE_MAGIC to shared page at IPA from x6
     * 3. Call FFA_MEM_RELINQUISH with handle
     * 4. Respond with DIRECT_RESP: x4 += 0x4000 (memory test signature)
     */
    /* Save response x1 (swapped src/dest), req type, handle + IPA across SMC calls */
    mov     x19, x1                 /* response x1 (already swapped above) */
    mov     x20, x4                 /* handle_lo */
    mov     x21, x5                 /* handle_hi */
    mov     x22, x6                 /* shared page IPA */
    mov     x23, x15                /* req type (0=32bit, 1=64bit) — save in callee-saved */

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

    /* Step 2: Write magic to shared page */
    ldr     w8, =SHARED_PAGE_MAGIC
    str     w8, [x22]

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
    mov     x15, x23                /* restore req type from callee-saved */
    bl      .Lload_resp_id          /* x0 = matching RESP function ID */
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
    mov     x15, x23                /* restore req type from callee-saved */
    bl      .Lload_resp_id          /* x0 = matching RESP function ID */
    mov     x1, x19                 /* response x1 */
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr                 /* x4 = 0 signals error */
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

.Lshare_test:
    /* SP-to-SP share test: x3=SP_TO_SP_SHARE_MAGIC, x7=receiver_sp_id
     *
     * 1. Write SP1_SHARE_WRITTEN to SP1_SHARE_PAGE (our own Secure DRAM)
     * 2. Call FFA_MEM_SHARE_32(sender=SP1, receiver=x7, ipa=SP1_SHARE_PAGE, pages=1)
     * 3. Return DIRECT_RESP: x3=echo magic, x4=handle_lo, x5=handle_hi, x6=page_ipa
     */
    mov     x19, x1                 /* response x1 (already swapped src/dest) */
    mov     x27, x7                 /* receiver SP ID */
    mov     x23, x15                /* req type (0=32bit, 1=64bit) */

    /* Step 1: Write magic to our share page */
    ldr     x22, =SP1_SHARE_PAGE
    ldr     w8, =SP1_SHARE_WRITTEN
    str     w8, [x22]

    /* Step 2: FFA_MEM_SHARE_32(sender=SP1, receiver=x27, ipa=SP1_SHARE_PAGE, pages=1) */
    ldr     x0, =FFA_MEM_SHARE_32
    ldr     w1, =SP1_PARTITION_ID
    lsl     w1, w1, #16             /* sender = SP1 in x1[31:16] */
    mov     x2, xzr
    mov     x3, x22                 /* IPA = SP1_SHARE_PAGE */
    mov     x4, #1                  /* 1 page */
    mov     x5, x27                 /* receiver SP ID */
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* Check: x0 == FFA_SUCCESS_32 */
    ldr     x8, =FFA_SUCCESS_32
    cmp     x0, x8
    b.ne    .Lshare_test_error

    /* Save handle from MEM_SHARE response — persist in memory for RECLAIM */
    mov     x20, x2                 /* handle_lo */
    mov     x21, x3                 /* handle_hi */
    adr     x8, saved_share_handle
    stp     x20, x21, [x8]

    /* Step 3: Return DIRECT_RESP with handle + page IPA */
    mov     x15, x23
    bl      .Lload_resp_id
    mov     x1, x19
    mov     x2, xzr
    ldr     x3, =SP_TO_SP_SHARE_MAGIC  /* echo magic back */
    mov     x4, x20                 /* handle_lo */
    mov     x5, x21                 /* handle_hi */
    mov     x6, x22                 /* page IPA = SP1_SHARE_PAGE */
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

.Lshare_test_error:
    /* MEM_SHARE failed — respond with error */
    mov     x15, x23
    bl      .Lload_resp_id
    mov     x1, x19
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr                 /* x4 = 0 signals error */
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

.Lreclaim_test:
    /* SP-to-SP reclaim: x3=SP_TO_SP_RECLAIM_MAGIC
     * Calls FFA_MEM_RECLAIM with saved handle from previous MEM_SHARE.
     * Returns DIRECT_RESP: x3=echo magic, x4=handle_lo, x5=FFA_SUCCESS status
     */
    mov     x19, x1                 /* response x1 */
    mov     x23, x15                /* req type */

    /* Load saved handle */
    adr     x8, saved_share_handle
    ldp     x20, x21, [x8]

    /* FFA_MEM_RECLAIM: x1=handle_lo, x2=handle_hi, x3=flags(0) */
    ldr     x0, =FFA_MEM_RECLAIM
    mov     x1, x20                 /* handle_lo */
    mov     x2, x21                 /* handle_hi */
    mov     x3, xzr                 /* flags = 0 */
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* Save reclaim result */
    mov     x24, x0                 /* FFA_SUCCESS or error */

    /* Return DIRECT_RESP: x3=magic, x4=handle_lo, x5=reclaim status */
    mov     x15, x23
    bl      .Lload_resp_id
    mov     x1, x19
    mov     x2, xzr
    ldr     x3, =SP_TO_SP_RECLAIM_MAGIC  /* echo magic */
    mov     x4, x20                 /* handle_lo */
    mov     x5, x24                 /* reclaim result (x0 from SMC) */
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lmsg_loop

/* Saved handle from last SP-to-SP MEM_SHARE (persists across DIRECT_REQ) */
.align 3
saved_share_handle:
    .quad   0   /* handle_lo */
    .quad   0   /* handle_hi */

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

/* ============ EL1 exception vectors ============ */
.macro VEC target
    b       \target
    .space  (0x80 - 4)
.endm

.align 11
sp_vector_table:
    /* Current EL with SP0 */
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    /* Current EL with SPx */
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    /* Lower EL using AArch64 */
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    /* Lower EL using AArch32 */
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected
    VEC .Lvec_unexpected

.Lvec_unexpected:
    /* Report failure to SPMC instead of jumping into unmapped memory. */
    ldr     x0, =0x84000060      /* FFA_ERROR */
    mov     x1, xzr
    mov     x2, #-2              /* FFA_INVALID_PARAMETERS */
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0
    b       .Lvec_unexpected

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
