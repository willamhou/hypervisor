/*
 * Trivial S-EL2 BL32 payload for TF-A boot chain validation.
 *
 * Entry from SPMD (EL3):
 *   x0 = TOS_FW_CONFIG (SPMC manifest DTB) physical address
 *   x1 = HW_CONFIG (hardware DTB) physical address
 *   x4 = Core linear ID
 *
 * Prints "Hello from S-EL2!" to PL011 UART, then issues FFA_MSG_WAIT
 * to signal init complete to SPMD, unblocking BL33 boot.
 */

.section .text.boot
.global _start

_start:
    /* Save SPMD-provided registers (for future use) */
    mov x20, x0            /* TOS_FW_CONFIG PA */
    mov x21, x1            /* HW_CONFIG PA */
    mov x22, x4            /* Core ID */

    /* Print message to PL011 UART */
    ldr x5, =0x09000000    /* QEMU virt PL011 base */
    adr x6, msg
1:
    ldrb w7, [x6], #1
    cbz w7, 2f
    str w7, [x5]           /* UARTDR write */
    b 1b

2:
    /* Signal SPMC init complete to SPMD via FFA_MSG_WAIT */
    ldr x0, =0x8400006B    /* FFA_MSG_WAIT */
    mov x1, xzr
    mov x2, xzr
    mov x3, xzr
    smc #0
    b 2b                    /* Loop if SPMD returns */

.align 3
msg:
    .asciz "Hello from S-EL2! (BL32 trivial)\r\n"
