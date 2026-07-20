/*
 * Minimal SMP AP-wake reference test.
 * Multiboot1 header + 32-bit C entry.
 * Assembles with GAS, linked as i386 ELF.
 */

.set MAGIC,   0x1BADB002
.set FLAGS,   0
.set CHECKSUM, -(MAGIC + FLAGS)

.section .multiboot, "a"
.align 4
.long MAGIC
.long FLAGS
.long CHECKSUM

.section .text
.code32
.global _start
.extern main

_start:
    /* multiboot passes: eax=magic, ebx=info ptr */
    /* We'll pass them to main() as arguments */
    call main

    /* Exit via isa-debug-exit */
    cli
.Lhalt:
    hlt
    jmp .Lhalt
