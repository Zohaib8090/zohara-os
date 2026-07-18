// src/arch/arm64/syscall.rs

//! aarch64 syscall handler. Entered via `svc #0` from either EL1 or EL0.
//!
//! From EL1: current EL, SPx vectors → `svc_handler` in exception.S
//! From EL0: lower EL, SP0 vectors → `user_svc_handler` in exception.S
//!
//! Both trampolines call `handle_syscall(frame)` below, which reads the
//! syscall number and arguments from the saved register frame, dispatches
//! to `crate::syscall::dispatch`, and writes the return value back.

use core::arch::asm;

static mut LAST_SYSCALL_RET: u64 = 0;

#[no_mangle]
pub extern "C" fn handle_syscall(frame: *mut u64) -> u64 {
    let num = unsafe { *frame.add(8) } as usize;       // x8 at offset 64
    let arg0 = unsafe { *frame } as usize;              // x0 at offset 0
    let arg1 = unsafe { *frame.add(1) } as usize;      // x1 at offset 8
    let arg2 = unsafe { *frame.add(2) } as usize;      // x2 at offset 16

    let ret = crate::syscall::dispatch(num, arg0, arg1, arg2);

    // Advance past the 4-byte `svc` instruction.
    let elr: u64;
    unsafe { asm!("mrs {}, elr_el1", out(reg) elr); }
    let next = elr + 4;
    unsafe { asm!("msr elr_el1, {}", in(reg) next); }

    unsafe { LAST_SYSCALL_RET = ret as u64; }
    ret as u64
}

pub fn init() {}

pub fn raw_syscall(num: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    unsafe {
        asm!(
            "svc #0",
            "dmb ish",
            in("x0") arg0,
            in("x1") arg1,
            in("x2") arg2,
            in("x8") num,
        );
    }
    unsafe { LAST_SYSCALL_RET as isize }
}
