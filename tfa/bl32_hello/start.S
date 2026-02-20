/*
 * Trivial S-EL2 BL32 payload for TF-A boot chain validation.
 *
 * Entry from SPMD (EL3):
 *   x0 = TOS_FW_CONFIG (SPMC manifest DTB) physical address
 *   x1 = HW_CONFIG (hardware DTB) physical address
 *   x4 = Core linear ID
 *
 * Prints "Hello from S-EL2!" to PL011 UART at 0x09000000, then spins.
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
    /* Spin forever — SPMD expects SPMC to return via SMC,
       but for this trivial test we just halt. */
    wfe
    b 2b

.align 3
msg:
    .asciz "Hello from S-EL2! (BL32 trivial)\r\n"
