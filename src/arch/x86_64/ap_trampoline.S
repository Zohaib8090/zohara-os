/* ap_trampoline.S — AP trampoline.
 *
 * Assembled with: as --32
 * Linked at 0x8000, output as flat binary.
 *
 * Layout at 0x8000:
 *   Code: 0x00 - ~0x80
 *   0x8080: cr3, 0x8088: stack, 0x8090: entry
 *   0x8098: 6-byte gdt_ptr for 64-bit GDT
 *   0x80A0: 32-bit GDT (24 bytes), 0x80B8: 6-byte gdt32_ptr
 */

    .code16
    .section .text
    .global ap_trampoline_start

ap_trampoline_start:
    cli
    xor %ax, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %ss
    mov $0x7000, %sp

    /* Enable A20 */
    in $0x92, %al
    or $2, %al
    out %al, $0x92

    /* LGDT from 0x80B8 — manually encoded (8 bytes):
     * 0x67 = addr32 prefix, 0x0F 01 /5 = LGDT, 0x15 = [disp32]
     * disp32 = 0x000080B8 (little-endian) */
    .byte 0x67, 0x0F, 0x01, 0x15, 0xB8, 0x80, 0x00, 0x00

    /* Enable protected mode (32-bit in 16-bit mode = 0x66 prefix) */
    .byte 0x66
    mov %cr0, %eax
    .byte 0x66
    or $1, %eax
    .byte 0x66
    mov %eax, %cr0

    /* Far jump to 32-bit code — hand-encoded:
     * 0x66 = operand32, 0xEA = far jump, offset32 = 0x802D, sel = 0x0008
     * pmode32 is at offset 0x2D in the binary */
    .byte 0x66, 0xEA
    .long pmode32
    .word 0x0008

pmode32:
    .code32
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss
    mov $0x7000, %esp

    /* Write 'P' to serial */
    mov $0x3F8, %dx
    mov $0x50, %al
    out %al, %dx

    /* Enable PAE */
    mov %cr4, %eax
    or $0x20, %eax
    mov %eax, %cr4

    /* Load PML4 from 0x8080 */
    mov 0x8080, %eax
    mov %eax, %cr3

    /* Enable long mode (EFER.LME) */
    mov $0xC0000080, %ecx
    rdmsr
    or $0x100, %eax
    wrmsr

    /* Enable paging */
    mov %cr0, %eax
    or $0x80000000, %eax
    mov %eax, %cr0

    /* Write 'L' to serial */
    mov $0x3F8, %dx
    mov $0x4C, %al
    out %al, %dx

    /* Load 64-bit GDT from 0x8098 */
    lgdt 0x8098

    /* Far jump to 64-bit code */
    ljmp $0x08, $lmode64

lmode64:
    .code64
    mov $0x10, %ax
    mov %ax, %ds
    mov %ax, %es
    mov %ax, %fs
    mov %ax, %gs
    mov %ax, %ss

    /* Write '6' to serial */
    mov $0x3F8, %dx
    mov $0x36, %al
    out %al, %dx

    /* Load stack from 0x8088 */
    movq 0x8088, %rsp

    /* Write 'S' to serial */
    mov $0x3F8, %dx
    mov $0x53, %al
    out %al, %dx

    /* Call Rust entry from 0x8090 */
    movq 0x8090, %rax
    call *%rax

    cli
.Lhalt:
    hlt
    jmp .Lhalt
