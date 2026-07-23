/* Minimal multiboot header — linked at 0x200000 (start of first LOAD segment) */
.section .multiboot_header, "a", @progbits
.align 4
.long 0x1BADB002  /* magic */
.long 0x03        /* flags: align + meminfo */
.long 0xE4524FFB  /* checksum */
