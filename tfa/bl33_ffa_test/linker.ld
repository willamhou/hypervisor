/* Linker script for BL33 FF-A test client (NS-EL2).
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

    /* 4KB-aligned RXTX buffers for PARTITION_INFO_GET test */
    . = ALIGN(4096);
    .rxtx_bufs (NOLOAD) : {
        _tx_page = .;
        . += 4096;
        _rx_page = .;
        . += 4096;
    }

    /DISCARD/ : {
        *(.comment)
        *(.note*)
    }
}
