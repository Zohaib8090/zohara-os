ENTRY(_start)

SECTIONS {
    . = 0x200000;

    .text : ALIGN(4K)
    {
        KEEP(*(.multiboot))
        KEEP(*(.text._start))
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4K)
    {
        bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        bss_end = .;
    }

    _end = .;
}
