// src/arch/x86_64/mod.rs

use core::arch::global_asm;

global_asm!(include_str!("boot.S"));

pub mod paging;
pub mod page_fault;
pub mod syscall;
pub mod e820;

pub use crate::keyboard::try_get_key;

global_asm!(r#"
.global timer_irq_handler
timer_irq_handler:
    push rax
    push rcx
    push rdx
    push rsi
    push rdi
    push r8
    push r9
    push r10
    push r11
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    sub rsp, 256
    movdqu [rsp], xmm0
    movdqu [rsp+16], xmm1
    movdqu [rsp+32], xmm2
    movdqu [rsp+48], xmm3
    movdqu [rsp+64], xmm4
    movdqu [rsp+80], xmm5
    movdqu [rsp+96], xmm6
    movdqu [rsp+112], xmm7
    movdqu [rsp+128], xmm8
    movdqu [rsp+144], xmm9
    movdqu [rsp+160], xmm10
    movdqu [rsp+176], xmm11
    movdqu [rsp+192], xmm12
    movdqu [rsp+208], xmm13
    movdqu [rsp+224], xmm14
    movdqu [rsp+240], xmm15
    mov rdi, rsp
    and rsp, -16
    call timer_handler_rust
    mov rsp, rax
    movdqu xmm0, [rsp]
    movdqu xmm1, [rsp+16]
    movdqu xmm2, [rsp+32]
    movdqu xmm3, [rsp+48]
    movdqu xmm4, [rsp+64]
    movdqu xmm5, [rsp+80]
    movdqu xmm6, [rsp+96]
    movdqu xmm7, [rsp+112]
    movdqu xmm8, [rsp+128]
    movdqu xmm9, [rsp+144]
    movdqu xmm10, [rsp+160]
    movdqu xmm11, [rsp+176]
    movdqu xmm12, [rsp+192]
    movdqu xmm13, [rsp+208]
    movdqu xmm14, [rsp+224]
    movdqu xmm15, [rsp+240]
    add rsp, 256
    mov dx, 0x20
    mov al, 0x20
    out dx, al
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    pop r11
    pop r10
    pop r9
    pop r8
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rax
    iretq
"#);

global_asm!(r#"
.global syscall_entry
syscall_entry:
    push rax
    push rdi
    push rsi
    push rdx
    sub rsp, 8
    mov rdi, rax
    mov rsi, [rsp+24]
    mov rdx, [rsp+16]
    mov rcx, [rsp+8]
    call {dispatch}
    mov [rsp+32], rax
    add rsp, 8
    pop rdx
    pop rsi
    pop rdi
    pop rax
    iretq
"#,
dispatch = sym crate::syscall::dispatch,
);

pub fn halt() -> ! {
    unsafe {
        core::arch::asm!("cli");
        loop { core::arch::asm!("hlt"); }
    }
}

pub fn init_sse() {
    unsafe {
        let mut cr0: u64;
        core::arch::asm!("mov {}, cr0", out(reg) cr0);
        cr0 &= !(1 << 2);
        cr0 |= 1 << 1;
        core::arch::asm!("mov cr0, {}", in(reg) cr0);
        let mut cr4: u64;
        core::arch::asm!("mov {}, cr4", out(reg) cr4);
        cr4 |= 1 << 9;
        core::arch::asm!("mov cr4, {}", in(reg) cr4);
    }
}

pub fn write_serial(byte: u8) {
    unsafe {
        let mut status: u8 = 0;
        while status & 0x20 == 0 {
            core::arch::asm!("in al, dx", out("al") status, in("dx") 0x3FDu16);
        }
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") byte);
    }
}

pub fn init_timer() {
    unsafe {
        let divisor: u16 = 11932;
        core::arch::asm!("out dx, al", in("dx") 0x43u16, in("al") 0x36u8);
        core::arch::asm!("out dx, al", in("dx") 0x40u16, in("al") (divisor & 0xFF) as u8);
        core::arch::asm!("out dx, al", in("dx") 0x40u16, in("al") (divisor >> 8) as u8);
    }
}

/// Patch the TSS base address into the GDT (already loaded by boot.S)
/// and load the TSS register.
pub fn init_gdt_tss() {
    extern "C" { static tss64: u8; static gdt64_full: u8; }
    unsafe {
        let tss_base = &tss64 as *const u8 as usize;
        let gdt_base = &gdt64_full as *const u8 as usize;

        // Zero TSS.
        core::ptr::write_bytes(tss64 as *const u8 as *mut u8, 0, 104);
        // Set RSP0 = top of current task's kernel stack (for Ring 3 → Ring 0 transitions).
        let task_idx = crate::task::current_task();
        let kernel_stack_top = crate::task::current_task_ref().stack.as_ptr() as usize + 32768;
        core::ptr::write_volatile((tss64 as *const u8 as *mut u8).add(0x04) as *mut u64, kernel_stack_top as u64);
        // IOPB offset = 104 (no I/O bitmap).
        core::ptr::write_volatile((tss64 as *const u8 as *mut u8).add(0x66) as *mut u16, 104);

        // Patch GDT entry 5 (offset 0x28) with TSS base.
        // Entry 5 = TSS descriptor lower 8 bytes.
        let tb = tss_base as u64;
        let lo: u64 = 103
            | ((tb & 0xFFFF) << 16)
            | (((tb >> 16) & 0xFF) << 32)
            | (0x89u64 << 40)
            | (((tb >> 24) & 0xFF) << 56);
        core::ptr::write_volatile((gdt_base + 0x28) as *mut u64, lo);
        core::ptr::write_volatile((gdt_base + 0x30) as *mut u64, 0u64);

        // Load TSS register (selector 0x28).
        core::arch::asm!("ltr ax", in("ax") 0x28u16);
    }
}
