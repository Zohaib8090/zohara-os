/* Place .multiboot at the very start of the binary, before .rodata.
 * This ensures the header is within the first 8KB of the loaded image
 * for GRUB multiboot, while keeping QEMU's direct ELF loading working. */
SECTIONS {
    .multiboot : {
        KEEP(*(.multiboot))
    }
}
INSERT BEFORE .rodata;
