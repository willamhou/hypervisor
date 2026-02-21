/* ARM64 SPMC (S-EL2) Linker Script */

ENTRY(_start)

SECTIONS
{
    /* S-EL2 SPMC loads at SEC_DRAM_BASE (0x0e100000).
       This matches the load_address in tfa/spmc_manifest.dts. */
    . = 0x0e100000;

    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss : {
        __bss_start = .;
        *(.bss .bss.*)
        *(.bss.stack)
        *(COMMON)
        __bss_end = .;
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
    }
}
