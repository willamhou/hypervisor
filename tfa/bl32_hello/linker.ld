/* Linker script for trivial BL32 (S-EL2) payload.
 * Loaded at SEC_DRAM_BASE = 0x0e100000 by TF-A BL2. */

ENTRY(_start)

SECTIONS
{
    . = 0x0e100000;

    .text : {
        *(.text.boot)
        *(.text*)
    }

    .rodata : {
        *(.rodata*)
    }

    .data : {
        *(.data*)
    }

    .bss : {
        *(.bss*)
    }

    /DISCARD/ : {
        *(.comment)
        *(.note*)
    }
}
