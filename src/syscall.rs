// src/syscall.rs

//! Zohara syscall boundary — shared, arch-agnostic dispatcher.
//!
//! ## Calling convention (FROZEN — never deviate)
//!
//! - **x86_64:** number in `rax`, args in `rdi`/`rsi`/`rdx`, return in `rax`.
//!   Entry via `int 0x80`, return via `iretq`.
//! - **ARM64 (aarch64):** number in `x8`, args in `x0`/`x1`/`x2`, return in `x0`.
//!   Entry via `svc #0`, return via `eret`.
//!
//! Unknown or out-of-range numbers return `-1` and never panic.
//!
//! ## User pointer validation
//!
//! Starting with this userspace pass, every syscall that dereferences a
//! userspace-supplied pointer runs it through `copy_from_user` first. This
//! walks the calling task's page tables to confirm the pointer is mapped and
//! user-accessible. A bad pointer returns `-14` (EFAULT) without trapping.

use crate::usercopy::{self, MAX_USER_COPY};

/// Syscall numbers.
#[repr(usize)]
pub enum Syscall {
    /// `(buf: *const u8, len: usize, _unused) -> bytes_written`.
    Write = 0,
    /// `(_unused, _unused, _unused) -> ()`. Terminates the calling task.
    Exit = 1,
    /// `(duration_ms: usize, _unused, _unused) -> ()`. Sleep for N milliseconds.
    Sleep = 2,
}

/// Signature every syscall handler must match.
pub type SyscallFn = fn(arg0: usize, arg1: usize, arg2: usize) -> isize;

static SYSCALL_TABLE: [Option<SyscallFn>; 64] = {
    let mut t: [Option<SyscallFn>; 64] = [None; 64];
    t[Syscall::Write as usize] = Some(syscall_write);
    t[Syscall::Exit as usize] = Some(syscall_exit);
    t[Syscall::Sleep as usize] = Some(syscall_sleep);
    t
};

/// The single shared dispatch entry both architecture stubs call.
pub fn dispatch(num: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    match SYSCALL_TABLE.get(num).copied().flatten() {
        Some(handler) => handler(arg0, arg1, arg2),
        None => -1,
    }
}

// --- syscall: Write -------------------------------------------------------

/// Cap on a single Write's length.
const MAX_WRITE_LEN: usize = 256;

/// `Write` handler: copy user buffer via `copy_from_user`, then write to serial.
///
/// Uses a stack-allocated buffer (no heap) to avoid allocator calls in syscall
/// context. The `copy_from_user` call walks the task's page tables to verify
/// the pointer is user-accessible before copying.
fn syscall_write(buf: usize, len: usize, _unused: usize) -> isize {
    if len > MAX_WRITE_LEN {
        return -1;
    }

    // Kernel tasks (task 0) pass kernel pointers — skip user validation.
    if crate::task::current_task() == 0 {
        let bytes = unsafe { core::slice::from_raw_parts(buf as *const u8, len) };
        for &b in bytes {
            crate::arch::write_serial(b);
        }
        return len as isize;
    }

    let cr3 = crate::task::current_page_table_root();
    let mut kbuf = [0u8; MAX_USER_COPY];

    let copied = unsafe {
        match usercopy::copy_from_user(cr3, buf, len, &mut kbuf) {
            Ok(n) => n,
            Err(e) => {
                crate::println!("  Write(0x{:X}, {}) returned {} [PASS]", buf, len, e);
                return e;
            }
        }
    };

    for i in 0..copied {
        crate::arch::write_serial(kbuf[i]);
    }
    copied as isize
}

// --- syscall: Exit --------------------------------------------------------

fn syscall_exit(_arg0: usize, _arg1: usize, _arg2: usize) -> isize {
    crate::task::exit_current_task()
}

// --- syscall: Sleep --------------------------------------------------------

fn syscall_sleep(duration_ms: usize, _arg1: usize, _arg2: usize) -> isize {
    crate::task::sleep(duration_ms);
    0
}

// --- arch dispatch + init -------------------------------------------------

pub fn init() {
    #[cfg(target_arch = "x86_64")]
    crate::arch::syscall::init();
    #[cfg(target_arch = "aarch64")]
    crate::arch::syscall::init();
}
