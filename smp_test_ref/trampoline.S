/*
 * Minimal AP trampoline — 16-bit real-mode only, no protected mode switch.
 * After SIPI vector=0x08, AP starts at CS:IP=0x0000:0x8000 in real mode.
 * Simply writes a marker to 0x9000 and halts.
 */
    .code16
    .section .text.trampoline, "ax", @progbits
    .globl trampoline_start
trampoline_start:
    cli
    /* Write 0xAABBCCDD to 0x9000 (real mode, no paging needed) */
    movw $0xCCDD, 0x9000
    movw $0xAABB, 0x9002
    hlt

    .globl trampoline_end
trampoline_end:
