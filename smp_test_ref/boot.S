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
    /* EARLIEST possible debug: write 'B' to VGA + serial before anything */
    movl $0xB8000, %edi
    movw $0x4F42, (%edi)  /* 'B' in white-on-blue at VGA[0] */
    movw $0x4F4B, 2(%edi) /* 'K' at VGA[1] */
    /* Serial: init COM1 + send 'B' */
    movw $0x3F9, %dx
    movb $0x00, %al
    outb %al, %dx
    movw $0x3FB, %dx
    movb $0x80, %al
    outb %al, %dx
    movw $0x3F8, %dx
    movb $0x01, %al
    outb %al, %dx
    movw $0x3F9, %dx
    movb $0x00, %al
    outb %al, %dx
    movw $0x3FB, %dx
    movb $0x03, %al
    outb %al, %dx
    movw $0x3F8, %dx
    movb $0x42, %al       /* 'B' */
    outb %al, %dx

    /* multiboot passes: eax=magic, ebx=info ptr */
    call main

    /* Exit via isa-debug-exit */
    cli
.Lhalt:
    hlt
    jmp .Lhalt
