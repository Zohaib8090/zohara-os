# boot.S — bootstrap with full GDT (Ring 0+3 + TSS placeholder)
#
# The GDT is in .rodata with all entries pre-built except the TSS.
# TSS lives in .bss. Rust patches the TSS base into the GDT at runtime.

.set ALIGN,    1<<0
.set MEMINFO,  1<<1
.set FLAGS,    ALIGN | MEMINFO
.set MAGIC,    0x1BADB002
.set CHECKSUM, -(MAGIC + FLAGS)
 .section .multiboot, "a"
 .align 4
 .long MAGIC
 .long FLAGS
 .long CHECKSUM

 .section .note.Xen, "a", @note
 .align 4
 .long 4
 .long 8
 .long 18
 .asciz "Xen"
 .align 4
 .quad _start

 .section .bss
 .align 4
 .global mb_magic
 .global mb_info
 mb_magic: .skip 4
 mb_info:  .skip 4

 .section .bss
 .align 4096
 p4_table:
     .skip 4096
 p3_table:
     .skip 4096
 p2_table:
     .skip 4096
 stack_bottom:
     .skip 32768
 stack_top:

 .section .bss
 .align 16
 .global tss64
 tss64:
     .skip 104

 # Full GDT in .rodata.
 # Layout (7 entries × 8 bytes = 56 bytes):
 #   0x00: null
 #   0x08: kernel code (P=1, DPL=0, S=1, E=1, L=1)
 #   0x10: kernel data (P=1, DPL=0, S=1, W=1)
 #   0x18: user code   (P=1, DPL=3, S=1, E=1, L=1)
 #   0x20: user data   (P=1, DPL=3, S=1, W=1)
 #   0x28: TSS lo (placeholder — Rust patches base address at runtime)
 #   0x30: TSS hi (placeholder)
 .section .data
 .align 8
 .global gdt64_full
gdt64_full:
 .quad 0                                                              # 0x00 null
 .quad (1<<43)|(1<<44)|(1<<47)|(1<<53)                               # 0x08 kernel code
 .quad (1<<41)|(1<<44)|(1<<47)                                        # 0x10 kernel data
 .quad (1<<41)|(1<<43)|(1<<44)|(1<<45)|(1<<46)|(1<<47)|(1<<53)       # 0x18 user code
 .quad (1<<41)|(1<<44)|(1<<45)|(1<<46)|(1<<47)                        # 0x20 user data
 .quad 0                                                              # 0x28 TSS lo (patched)
 .quad 0                                                              # 0x30 TSS hi (patched)

 .align 8
gdt64_ptr:
 .word 55                                                   # limit = 7*8-1
 .quad gdt64_full

 .section .text
 .code32
 .global _start

 _start:
     # Save Multiboot magic (eax) and info pointer (ebx) before paging clobbers them.
     mov [mb_magic], eax
     mov [mb_info], ebx

     mov esp, offset stack_top

     mov eax, offset p3_table
     or eax, 0b11
     mov [p4_table], eax

     mov eax, offset p2_table
     or eax, 0b11
     mov [p3_table], eax

     mov ecx, 0
 .Lmap_p2_table:
     mov eax, 0x200000
     mul ecx
     or eax, 0b10000011
     mov [p2_table + ecx * 8], eax
     inc ecx
     cmp ecx, 512
     jne .Lmap_p2_table

     mov eax, offset p4_table
     mov cr3, eax

     mov eax, cr4
     or eax, 1 << 5
     mov cr4, eax

     mov ecx, 0xC0000080
     rdmsr
     or eax, 1 << 8
     wrmsr

     mov eax, cr0
     or eax, 1 << 31 | 1 << 0
     mov cr0, eax

     # Load the full GDT (already has Ring 3 + TSS placeholder).
     lgdt [gdt64_ptr]

     ljmp 8, offset _start64

 .code64
 _start64:
     xor ax, ax
     mov ds, ax
     mov es, ax
     mov fs, ax
     mov gs, ax
     mov ss, ax

     call kernel_main

     cli
 .Lhalt_loop:
     hlt
     jmp .Lhalt_loop
