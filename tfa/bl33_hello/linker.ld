/* Linker script for trivial BL33 (NS-EL2) payload.
 * Loaded at 0x40000000 by TF-A BL2. */

ENTRY(_start)

SECTIONS
{
    . = 0x40000000;

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
