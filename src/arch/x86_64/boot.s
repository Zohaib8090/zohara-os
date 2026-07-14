# Declare constants for the Multiboot header
.set ALIGN,    1<<0             # align loaded modules on page boundaries
.set MEMINFO,  1<<1             # provide memory map
.set FLAGS,    ALIGN | MEMINFO  # multiboot flags
.set MAGIC,    0x1BADB002       # 'magic number' letting the bootloader find the header
.set CHECKSUM, -(MAGIC + FLAGS) # checksum to prove we are multiboot

# Multiboot section
.section .multiboot_header
.align 4
.long MAGIC
.long FLAGS
.long CHECKSUM

# Allocate room for a small stack
.section .bss
.align 16
stack_bottom:
.skip 16384 # 16 Kilobytes of stack space
stack_top:

# Code section
.section .text
.global _start
.type _start, @function

_start:
    # Set up the stack pointer register (rsp)
    mov $stack_top, %rsp

    # Call our platform-independent Rust kernel entry point
    call kernel_main

    # If kernel_main ever returns, freeze the CPU in an infinite loop
    cli
halt_loop:
    hlt
    jmp halt_loop

.size _start, . - _start