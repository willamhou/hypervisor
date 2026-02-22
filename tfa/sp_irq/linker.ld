/* SP IRQ linker script.
 * Loaded at 0x0e400000 in SEC_DRAM by TF-A BL2. */
ENTRY(_start)
SECTIONS
{
    . = 0x0e400000;
    .text : { *(.text.boot) *(.text*) }
    .rodata : { *(.rodata*) }
    .data : { *(.data*) }
    .bss : { *(.bss*) }
}
