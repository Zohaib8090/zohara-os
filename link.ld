ENTRY(_start)

SECTIONS {
    . = 0x40000000;

    .text : ALIGN(4K)
    {
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
        bss_start = .;       /* Add this: Define the start symbol */
        *(.bss .bss.*)
        *(COMMON)            /* Add this: Include common symbols */
        bss_end = .;         /* Add this: Define the end symbol */
    }

    /* Mark the end of the kernel image. Used by the frame allocator to
       avoid handing out frames that overlap the kernel/heap/bitmap. */
    _end = .;
}