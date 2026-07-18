.global _start
.section .text._start

_start:
    # Set up stack pointer for ARMv7
    LDR r0, =stack_top
    MOV sp, r0

    # Branch to Rust kernel entry
    BL kernel_main

.Lhang:
    WFI
    B .Lhang

# Allocate stack space
.section .bss
.align 3
stack_bottom:
.space 16384
stack_top: