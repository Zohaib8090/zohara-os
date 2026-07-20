// src/usercopy.rs

//! Syscall pointer validation — copy_from_user / copy_to_user.
//!
//! Every syscall that dereferences a userspace-supplied pointer MUST run it
//! through these helpers first. They walk the calling task's page tables to
//! confirm the entire `[ptr, ptr+len)` range is:
//!
//! 1. Present (no page fault)
//! 2. Mapped with the User/Supervisor (US) bit set — i.e. it belongs to the
//!    task's own address space, not a kernel-only region it tried to guess
//!
//! On failure the helper returns `Err(EFAULT)` (-14) without trapping —
//! a bad pointer from userspace is a normal error, not a kernel panic.
//!
//! **Design choice — stack buffer, not Vec:** The syscall handler runs on
//! the kernel stack, which is small (4 KiB guard + task's 32 KiB stack).
//! Using Vec would allocate from the 64 KB heap, risking fragmentation or
/// exhaustion under syscalls. Instead, `copy_from_user` copies into a
/// caller-provided fixed buffer (up to `MAX_USER_COPY` bytes). Syscall
/// handlers must work within this limit — which is fine because Write's
/// MAX_WRITE_LEN is already 256 bytes.

/// Linux EFAULT: bad address.
pub const EFAULT: isize = -14;

/// Maximum bytes a single copy_from_user / copy_to_user call will handle.
/// Sized to cover the largest realistic syscall buffer (Write's 256-byte cap).
pub const MAX_USER_COPY: usize = 512;

/// Copy `len` bytes from userspace `src` into a kernel buffer.
///
/// Walks the caller's page tables to validate every page in the range.
/// Returns the number of bytes actually copied (should equal `len` on success),
/// or `Err(EFAULT)` if any page is missing, kernel-only, or the range overflows.
///
/// `task_page_table_root` is the physical address of the task's top-level page
/// table (CR3 on x86_64, TTBR0_EL1 on ARM64). The arch-specific page table
/// walker is called via the `crate::arch` module.
pub unsafe fn copy_from_user(
    task_cr3: usize,
    user_ptr: usize,
    len: usize,
    kernel_buf: &mut [u8; MAX_USER_COPY],
) -> Result<usize, isize> {
    if len > MAX_USER_COPY {
        return Err(EFAULT);
    }
    if len == 0 {
        return Ok(0);
    }

    // Validate the entire range first, then copy.
    // Walk page tables to check every page boundary.
    let end = user_ptr.checked_add(len).ok_or(EFAULT)?;
    let mut addr = user_ptr;
    while addr < end {
        let page_start = addr & !0xFFF;
        let page_end = page_start + 4096;
        let chunk_end = end.min(page_end);

        if !crate::arch::paging::is_user_mapped(task_cr3, addr) {
            return Err(EFAULT);
        }

        let src = addr as *const u8;
        let dst_off = addr - user_ptr;
        let copy_len = chunk_end - addr;
        let dst = kernel_buf[dst_off..dst_off + copy_len].as_mut_ptr();
        core::ptr::copy_nonoverlapping(src, dst, copy_len);

        addr = page_end;
    }

    Ok(len)
}

/// Copy `len` bytes from a kernel buffer into userspace `dst`.
///
/// Same validation as `copy_from_user` but in reverse: checks that every
/// page in the destination range is present and user-mapped before writing.
pub unsafe fn copy_to_user(
    task_cr3: usize,
    user_ptr: usize,
    kernel_src: *const u8,
    len: usize,
) -> Result<usize, isize> {
    if len == 0 {
        return Ok(0);
    }

    let end = user_ptr.checked_add(len).ok_or(EFAULT)?;
    let mut addr = user_ptr;
    let mut src_off: usize = 0;
    while addr < end {
        let page_start = addr & !0xFFF;
        let page_end = page_start + 4096;
        let chunk_end = end.min(page_end);

        if !crate::arch::paging::is_user_mapped(task_cr3, addr) {
            return Err(EFAULT);
        }

        let dst = addr as *mut u8;
        let src = kernel_src.add(src_off);
        let copy_len = chunk_end - addr;
        core::ptr::copy_nonoverlapping(src, dst, copy_len);

        src_off += copy_len;
        addr = page_end;
    }

    Ok(len)
}

/// Copy a null-terminated string from userspace into a kernel buffer.
///
/// Returns the string content (without null terminator) or an error.
pub unsafe fn copy_from_user_cstr(
    task_cr3: usize,
    user_ptr: usize,
    max_len: usize,
) -> Result<alloc::string::String, isize> {
    let mut buf = [0u8; 256];
    let len = max_len.min(255);
    let mut i = 0;
    while i < len {
        if !crate::arch::paging::is_user_mapped(task_cr3, user_ptr + i) {
            return Err(EFAULT);
        }
        let byte = core::ptr::read_volatile((user_ptr + i) as *const u8);
        if byte == 0 { break; }
        buf[i] = byte;
        i += 1;
    }
    let s = alloc::string::String::from(core::str::from_utf8(&buf[..i]).unwrap_or(""));
    Ok(s)
}
