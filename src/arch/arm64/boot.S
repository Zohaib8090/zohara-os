// src/arch/arm64/boot.S

.global _start
.section .text

_start:
    // Only execute on Core 0, put other cores to sleep
    mrs     x0, mpidr_el1
    and     x0, x0, #0xFF
    cbz     x0, .Lmaster_core

.Lparking_loop:
    wfe
    b       .Lparking_loop

.Lmaster_core:
    // Set up stack pointer
    ldr     x0, =stack_top
    mov     sp, x0

    // Clear BSS section to zero (Crucial for safe Rust globals!)
    ldr     x0, =bss_start
    ldr     x1, =bss_end
.Lclear_bss_loop:
    cmp     x0, x1
    b.hs    .Lclear_bss_done
    str     xzr, [x0], #8
    b       .Lclear_bss_loop

.Lclear_bss_done:
    // Enable Floating Point and SIMD (CPACR_EL1.FPEN = 3, bits [21:20])
    mov     x0, #(3 << 20)
    msr     cpacr_el1, x0
    isb

    // Jump straight to Rust kernel main
    bl      kernel_main

.Lhalt_loop:
    wfe
    b       .Lhalt_loop

.section .bss
.align 16
stack_bottom:
.skip 16384
stack_top: