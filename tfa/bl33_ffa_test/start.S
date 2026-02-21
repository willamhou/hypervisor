/*
 * BL33 FF-A Test Client — sends FF-A SMC calls to SPMC via SPMD.
 * Runs at NS-EL2. Prints PASS/FAIL for each test.
 *
 * Entry from BL31 (EL3):
 *   x0 = HW_CONFIG (hardware DTB) physical address
 *   x4 = Core linear ID
 */

.section .text.boot
.global _start

/* PL011 UART */
.equ UART_BASE, 0x09000000
.equ UARTDR,    0x000
.equ UARTFR,    0x018

/* FF-A function IDs (SMC32) */
.equ FFA_ERROR,           0x84000060
.equ FFA_SUCCESS_32,      0x84000061
.equ FFA_VERSION,         0x84000063
.equ FFA_FEATURES,        0x84000064
.equ FFA_PARTITION_INFO,  0x84000068
.equ FFA_ID_GET,          0x84000069
.equ FFA_DIRECT_REQ_32,   0x8400006F
.equ FFA_DIRECT_RESP_32,  0x84000070

.equ FFA_VERSION_1_1,     0x00010001

_start:
    /* Save BL31-provided registers */
    mov     x20, x0            /* HW_CONFIG PA */
    mov     x21, x4            /* Core Id */

    /* Print banner */
    adr     x0, str_banner
    bl      uart_print

    /* ============ Test 1: FFA_VERSION ============ */
    adr     x0, str_t1
    bl      uart_print

    ldr     x0, =FFA_VERSION
    ldr     x1, =FFA_VERSION_1_1   /* caller's version */
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* x0 should be a valid version >= 1.1 (major=1, minor>=1) */
    /* Check major version (bits [31:16]) == 1 */
    lsr     x9, x0, #16
    cmp     x9, #1
    b.ne    .Lfail_1
    /* Check minor version (bits [15:0]) >= 1 */
    and     x9, x0, #0xFFFF
    cmp     x9, #1
    b.lt    .Lfail_1
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest_2
.Lfail_1:
    adr     x0, str_fail
    bl      uart_print

    /* ============ Test 2: FFA_ID_GET ============ */
.Ltest_2:
    adr     x0, str_t2
    bl      uart_print

    ldr     x0, =FFA_ID_GET
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    /* x0 should be FFA_SUCCESS_32 */
    ldr     x9, =FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    .Lfail_2
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest_3
.Lfail_2:
    adr     x0, str_fail
    bl      uart_print

    /* ============ Test 3: FFA_FEATURES(FFA_VERSION) ============ */
.Ltest_3:
    adr     x0, str_t3
    bl      uart_print

    ldr     x0, =FFA_FEATURES
    ldr     x1, =FFA_VERSION       /* query FFA_VERSION support */
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    ldr     x9, =FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    .Lfail_3
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest_4
.Lfail_3:
    adr     x0, str_fail
    bl      uart_print

    /* ============ Test 4: FFA_FEATURES(0xDEAD) -> NOT_SUPPORTED ============ */
.Ltest_4:
    adr     x0, str_t4
    bl      uart_print

    ldr     x0, =FFA_FEATURES
    movz    x1, #0xDEAD            /* unsupported function */
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    ldr     x9, =FFA_ERROR
    cmp     x0, x9
    b.ne    .Lfail_4
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest_5
.Lfail_4:
    adr     x0, str_fail
    bl      uart_print

    /* ============ Test 5: FFA_PARTITION_INFO_GET ============ */
.Ltest_5:
    adr     x0, str_t5
    bl      uart_print

    ldr     x0, =FFA_PARTITION_INFO
    mov     x1, xzr
    mov     x2, xzr
    mov     x3, xzr
    mov     x4, xzr
    mov     x5, xzr
    mov     x6, xzr
    mov     x7, xzr
    smc     #0

    ldr     x9, =FFA_SUCCESS_32
    cmp     x0, x9
    b.ne    .Lfail_5
    /* x2 = count, should be 1 (SP1 registered) */
    cmp     x2, #1
    b.ne    .Lfail_5
    adr     x0, str_pass
    bl      uart_print
    b       .Ltest_6
.Lfail_5:
    adr     x0, str_fail
    bl      uart_print

    /* ============ Test 6: FFA_MSG_SEND_DIRECT_REQ echo ============ */
.Ltest_6:
    adr     x0, str_t6
    bl      uart_print

    ldr     x0, =FFA_DIRECT_REQ_32
    /* x1: source=0x0001 (NWd VM), dest=0x8001 (SP1) */
    movz    x1, #0x8001
    movk    x1, #0x0001, lsl #16
    mov     x2, xzr
    movz    x3, #0xAAAA            /* payload */
    movz    x4, #0xBBBB
    movz    x5, #0xCCCC
    movz    x6, #0xDDDD
    movz    x7, #0xEEEE
    smc     #0

    /* x0 should be FFA_DIRECT_RESP_32 */
    ldr     x9, =FFA_DIRECT_RESP_32
    cmp     x0, x9
    b.ne    .Lfail_6
    /* Check echoed payload x3, x4, x5 */
    movz    x9, #0xAAAA
    cmp     x3, x9
    b.ne    .Lfail_6
    movz    x9, #0xBBBB
    cmp     x4, x9
    b.ne    .Lfail_6
    movz    x9, #0xCCCC
    cmp     x5, x9
    b.ne    .Lfail_6
    adr     x0, str_pass
    bl      uart_print
    b       .Ldone
.Lfail_6:
    adr     x0, str_fail
    bl      uart_print

.Ldone:
    adr     x0, str_done
    bl      uart_print

.Lhalt:
    wfe
    b       .Lhalt

/* ============ UART print subroutine ============ */
/* x0 = pointer to null-terminated string. Clobbers x10-x12. */
uart_print:
    ldr     x10, =UART_BASE
.Lprint_loop:
    ldrb    w11, [x0], #1
    cbz     w11, .Lprint_done
.Lprint_wait:
    ldr     w12, [x10, #UARTFR]    /* UARTFR */
    tbnz    w12, #5, .Lprint_wait  /* wait if TXFF */
    str     w11, [x10, #UARTDR]    /* UARTDR */
    b       .Lprint_loop
.Lprint_done:
    ret

/* ============ String data ============ */
.section .rodata

str_banner:
    .asciz "\r\n========================================\r\n  BL33 FF-A Test Client (NS-EL2)\r\n========================================\r\n\r\n"
str_t1:
    .asciz "  Test 1: FFA_VERSION .............. "
str_t2:
    .asciz "  Test 2: FFA_ID_GET ............... "
str_t3:
    .asciz "  Test 3: FFA_FEATURES(VERSION) .... "
str_t4:
    .asciz "  Test 4: FFA_FEATURES(0xDEAD) ..... "
str_t5:
    .asciz "  Test 5: PARTITION_INFO_GET ........ "
str_t6:
    .asciz "  Test 6: DIRECT_REQ echo .......... "
str_pass:
    .asciz "PASS\r\n"
str_fail:
    .asciz "FAIL\r\n"
str_done:
    .asciz "\r\n  All tests complete.\r\n"
