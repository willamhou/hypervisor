/* ARM64 Hypervisor Linker Script */

ENTRY(_start)

SECTIONS
{
    /* QEMU virt machine: RAM starts at 0x40000000 */
    . = 0x40000000;

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
        *(.bss.pcpu_stacks)
        *(COMMON)
        __bss_end = .;
    }

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
    }
}
