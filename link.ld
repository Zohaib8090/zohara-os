/* link.ld */
ENTRY(kernel_main)

SECTIONS {
    /* Standard load address for bootloaders is 1MB */
    . = 1M;

    .text :
    {
        /* Put the executable code here */
        *(.text .text.*)
    }

    .rodata :
    {
        /* Read-only data (like static strings) */
        *(.rodata .rodata.*)
    }

    .data :
    {
        /* Read-write initialized data */
        *(.data .data.*)
    }

    .bss :
    {
        /* Read-write uninitialized data (zero-filled) */
        *(.bss .bss.*)
    }
}