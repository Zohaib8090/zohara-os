// src/arch/x86_64/syscall.rs

//! x86_64 syscall entry glue.
//!
//! The entry stub (`syscall_entry`) is a `global_asm!` block in `mod.rs`. On
//! `int 0x80` the CPU has already pushed SS/RSP/RFLAGS/CS/RIP onto the stack.
//! The stub pushes the syscall-relevant registers, aligns the stack, calls the
//! shared `crate::syscall::dispatch`, writes the return value into the saved
//! `rax` slot, and returns via `iretq`. Software `int` does not go through
//! the PIC, so no EOI is sent.

use core::arch::asm;

// The assembly entry stub (defined via `global_asm!` in `mod.rs`).
extern "C" {
    fn syscall_entry();
}

/// Register the syscall entry stub at IDT vector 0x80 (128).
///
/// Called from `crate::syscall::init()` during boot, after `init_idt`.
pub fn init() {
    unsafe {
        crate::interrupts::set_syscall_handler(syscall_entry as *const () as usize);
    }
}

/// Issue a syscall from (currently kernel-mode) test code via `int 0x80`.
///
/// Mirrors the ABI a future userspace would use: number in `rax`, args in
/// `rdi`/`rsi`/`rdx`, return in `rax`.
pub fn raw_syscall(num: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    let mut ret: isize;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") num,
            in("rdi") arg0,
            in("rsi") arg1,
            in("rdx") arg2,
            lateout("rax") ret,
            // The C ABI may clobber these across the call; mark them so the
            // compiler doesn't assume they survive.
            out("rcx") _,
            out("r11") _,
        );
    }
    ret
}
