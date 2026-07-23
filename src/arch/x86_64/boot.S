# boot.S — bootstrap with full GDT (Ring 0+3 + TSS placeholder)
#
# The GDT is in .data with all entries pre-built except the TSS.
# TSS lives in .bss. Rust patches the TSS base into the GDT at runtime.
#
# Bare-metal debug: VGA markers (0xB8000) + serial COM1 at each stage.
# Read the letter on screen to see where boot stops.

.intel_syntax noprefix

.set ALIGN,    1<<0
.set MEMINFO,  1<<1
.set VIDEO_MODE, 1<<2
.set FLAGS,    ALIGN | MEMINFO | VIDEO_MODE
.set MAGIC,    0x1BADB002
.set CHECKSUM, -(MAGIC + FLAGS)
 /* Multiboot header — MUST be within first 8KB of loaded image.
  * VIDEO_MODE flag requires five extra fields after the checksum. */
 .section .multiboot, "a"
 .align 4
 .long MAGIC
 .long FLAGS
 .long CHECKSUM
 .long 0             # mode_type: 0 = linear graphics mode
 .long 0             # width: 0 = let bootloader decide
 .long 0             # height: 0 = let bootloader decide
 .long 0             # depth: 0 = let bootloader decide

 .section .bss
 .align 4
 .global mb_magic
 .global mb_info
 mb_magic: .skip 4
 .align 8
 mb_info:  .skip 8

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

 # Full GDT in .data.
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
 .long gdt64_full

 .section .text
 .code32
 .global _start

 _start:
     # === STAGE A: Force VGA text mode 3 + serial init ===
     # VGA Miscellaneous Output Register: set mode 3
     mov dx, 0x3C2
     mov al, 0x03
     out dx, al
     # Sequencer: reset
     mov dx, 0x3C4
     mov ax, 0x0300
     out dx, ax
     # Sequencer reg 1: enable video
     mov dx, 0x3C4
     mov al, 0x01
     out dx, al
     mov dx, 0x3C5
     in al, dx
     and al, 0xDF
     out dx, al
     # Clear 80x25 screen (4000 bytes)
     cld
     mov edi, 0xB8000
     mov ecx, 2000
     mov ax, 0x0F20
     rep stosw
     # Write 'A' at top-left corner
     mov word ptr [0xB8000], 0x0F41

     # Init COM1 serial: 38400 baud, 8N1, FIFO
     mov dx, 0x3F9
     mov al, 0x00
     out dx, al            # Disable interrupts
     mov dx, 0x3FB
     mov al, 0x80
     out dx, al            # Enable DLAB
     mov dx, 0x3F8
     mov al, 0x03
     out dx, al            # Divisor lo
     mov dx, 0x3F9
     mov al, 0x00
     out dx, al            # Divisor hi
     mov dx, 0x3FB
     mov al, 0x03
     out dx, al            # 8N1
     mov dx, 0x3FA
     mov al, 0xC7
     out dx, al            # FIFO
     mov dx, 0x3FC
     mov al, 0x0B
     out dx, al            # IRQs+RTS/DSR
     # Send 'A' on serial
     mov dx, 0x3F8
     mov al, 0x41
     out dx, al

     # === STAGE B: Save multiboot info ===
     mov [mb_magic], eax
     mov [mb_info], ebx
     mov word ptr [0xB8000], 0x0F42   # 'B' on screen
     mov dx, 0x3F8
     mov al, 0x42
     out dx, al

     # === STAGE C: Set up stack ===
     mov esp, offset stack_top
     mov word ptr [0xB8000], 0x0F43   # 'C' on screen
     mov dx, 0x3F8
     mov al, 0x43
     out dx, al

     # === STAGE D: Build identity-mapped page tables (0-1GB, 2MB pages) ===
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
     mov word ptr [0xB8000], 0x0F44   # 'D' on screen
     mov dx, 0x3F8
     mov al, 0x44
     out dx, al

     # === STAGE E: Enable PAE + long mode ===
     mov eax, offset p4_table
     mov cr3, eax

     mov eax, cr4
     or eax, 1 << 5
     mov cr4, eax

     mov ecx, 0xC0000080
     rdmsr
     or eax, 1 << 8
     wrmsr
     mov word ptr [0xB8000], 0x0F45   # 'E' on screen
     mov dx, 0x3F8
     mov al, 0x45
     out dx, al

     # === STAGE F: Enable paging + protected mode ===
     mov eax, cr0
     or eax, (1 << 31) | (1 << 0)
     mov cr0, eax
     mov dx, 0x3F8
     mov al, 0x46
     out dx, al

     # === STAGE G: Load GDT, jump to 64-bit ===
     lgdt [gdt64_ptr]
     ljmp 8, offset _start64

 .code64
 _start64:
     # === STAGE H: In 64-bit mode ===
     mov word ptr [0xB8000], 0x4747   # 'GG' on screen
     # Send 'G' on serial
     mov dx, 0x3F8
     mov al, 0x47
     out dx, al

     xor ax, ax
     mov ds, ax
     mov es, ax
     mov fs, ax
     mov gs, ax
     mov ss, ax

     mov word ptr [0xB8020], 0x4848   # 'HH' on second line
     mov dx, 0x3F8
     mov al, 0x48
     out dx, al

     # Map the multiboot memory map buffer if it's above 1GB.
     # This allows detect_memory() to read it before paging::init().
     # mb_info is at physical address stored in [mb_info].
     mov rax, qword ptr [mb_info]
     test rax, rax
     jz .Lskip_mmap_map
     # mmap_addr at mb_info + 48 (u32)
     mov ebx, [rax + 48]
     test ebx, ebx
     jz .Lskip_mmap_map
     cmp rbx, 0x40000000
     jb .Lskip_mmap_map       # already below 1GB, no mapping needed
     # mmap_length at mb_info + 44 (u32)
     mov ecx, [rax + 44]
     test ecx, ecx
     jz .Lskip_mmap_map
     # Map the pages containing the mmap buffer.
     # Use P2 slot 508 as a temporary 2MB window.
     mov r12, offset p2_table
     # Get the 2MB-aligned physical address
     mov rax, rbx
     and rax, ~0x1FFFFF
     or rax, 0b10000011       # Present + Write + PageSize (2MB)
     mov [r12 + 508*8], rax
     # Reload CR3 to flush TLB
     mov rax, cr3
     mov cr3, rax

 .Lskip_mmap_map:
     call kernel_main

     # Should never reach here
     mov word ptr [0xB8040], 0x2121   # '!!' on third line
     cli
 .Lhalt_loop:
     hlt
     jmp .Lhalt_loop
