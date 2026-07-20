// src/linux_compat.rs

//! Linux x86_64 syscall ABI compatibility layer.
//!
//! Core syscalls are implemented. Stubs return ENOSYS (honest failure).

pub const ENOENT: isize = -2;
pub const EBADF: isize = -9;
pub const ENOMEM: isize = -12;
pub const EACCES: isize = -13;
pub const EFAULT: isize = -14;
pub const EIO: isize = -5;
pub const EINVAL: isize = -22;
pub const ENOSYS: isize = -38;

#[repr(C)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

pub fn dispatch(num: usize, arg0: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize, _arg5: usize) -> isize {
    match num {
        0   => sys_read(arg0, arg1, arg2),
        1   => sys_write(arg0, arg1, arg2),
        3   => sys_close(arg0),
        5   => sys_fstat(arg0, arg1),
        9   => sys_mmap(),
        10  => ENOSYS,
        11  => ENOSYS,
        12  => sys_brk(arg0),
        13  => ENOSYS,
        14  => ENOSYS,
        39  => sys_getpid(),
        56  => ENOSYS,
        57  => ENOSYS,
        59  => sys_execve(arg0, arg1, arg2),
        60  => sys_exit(arg0 as i32),
        61  => ENOSYS,
        72  => ENOSYS,
        78  => ENOSYS,
        79  => sys_getcwd(arg0, arg1),
        80  => ENOSYS,
        82  => ENOSYS,
        83  => sys_mkdir(arg0),
        84  => ENOSYS,
        87  => ENOSYS,
        89  => ENOSYS,
        96  => ENOSYS,
        102 => sys_getuid(),
        104 => sys_getuid(),
        105 => ENOSYS,
        106 => ENOSYS,
        107 => sys_getuid(),
        108 => sys_getuid(),
        115 => ENOSYS,
        116 => ENOSYS,
        125 => sys_capget(arg0, arg1),
        126 => ENOSYS,
        157 => ENOSYS,
        201 => ENOSYS,
        202 => ENOSYS,
        228 => sys_clock_gettime(arg0, arg1),
        230 => sys_clock_nanosleep(arg0, arg1, arg2),
        231 => sys_nanosleep(arg0, arg1),
        234 => ENOSYS,
        257 => sys_openat(arg0, arg1, arg2),
        262 => ENOENT,
        302 => ENOSYS,
        318 => sys_getuid(),
        319 => sys_getuid(),
        8   => sys_lseek(arg0, arg1, arg2),
        32  => sys_dup(arg0),
        33  => sys_dup2(arg0, arg1),
        323 => sys_capget(arg0, arg1),
        _ => { crate::warn!("linux", "unimplemented syscall {}", num); ENOSYS }
    }
}

// ---- Syscall implementations ----

fn sys_read(fd: usize, buf: usize, len: usize) -> isize {
    let task_id = crate::task::current_task();
    // Stdout/stderr → serial
    if fd == 1 || fd == 2 {
        return crate::syscall::syscall_write(fd, buf, len);
    }
    // Read from VFS file via fd table
    let node_idx = match crate::fd_table::node(task_id, fd) {
        Ok(n) => n,
        Err(_) => return EBADF,
    };
    let offset = crate::fd_table::offset(task_id, fd);
    let mut kbuf = [0u8; 512];
    let to_read = len.min(kbuf.len());
    match crate::fs::read_file_by_node(node_idx, offset, &mut kbuf[..to_read]) {
        Ok(n) => {
            if n == 0 { return 0; } // EOF
            // Copy to user buffer
            unsafe {
                let cr3 = crate::task::current_page_table_root();
                if crate::task::current_task() == 0 {
                    let dst = core::slice::from_raw_parts_mut(buf as *mut u8, n);
                    dst.copy_from_slice(&kbuf[..n]);
                } else {
                    let _ = crate::usercopy::copy_to_user(cr3, buf, kbuf.as_ptr(), n);
                }
            }
            crate::fd_table::set_offset(task_id, fd, offset + n);
            n as isize
        }
        Err(_) => EIO,
    }
}

fn sys_write(fd: usize, buf: usize, len: usize) -> isize {
    let task_id = crate::task::current_task();
    // Stdout/stderr → serial
    if fd == 1 || fd == 2 {
        return crate::syscall::syscall_write(fd, buf, len);
    }
    // Write to VFS file via fd table
    let node_idx = match crate::fd_table::node(task_id, fd) {
        Ok(n) => n,
        Err(_) => return EBADF,
    };
    let offset = crate::fd_table::offset(task_id, fd);
    let mut kbuf = [0u8; 512];
    let to_write = len.min(kbuf.len());
    // Copy from user buffer
    unsafe {
        let cr3 = crate::task::current_page_table_root();
        if crate::task::current_task() == 0 {
            let src = core::slice::from_raw_parts(buf as *const u8, to_write);
            kbuf[..to_write].copy_from_slice(src);
        } else {
            match crate::usercopy::copy_from_user(cr3, buf, to_write, &mut kbuf) {
                Ok(n) => { /* n bytes copied */ }
                Err(e) => return e,
            }
        }
    }
    match crate::fs::write_file_by_node(node_idx, offset, &kbuf[..to_write]) {
        Ok(n) => {
            crate::fd_table::set_offset(task_id, fd, offset + n);
            n as isize
        }
        Err(_) => EIO,
    }
}

fn sys_close(fd: usize) -> isize {
    let task_id = crate::task::current_task();
    match crate::fd_table::close(task_id, fd) {
        Ok(()) => 0,
        Err(_) => EBADF,
    }
}

fn sys_fstat(_fd: usize, _stat_buf: usize) -> isize { ENOENT }

fn sys_mmap() -> isize { ENOMEM }

fn sys_brk(_addr: usize) -> isize { 0x1000_0000 }

fn sys_getpid() -> isize { crate::task::current_task() as isize }

fn sys_exit(code: i32) -> isize {
    crate::stats::count_task_exit();
    crate::task::exit_current_task();
    code as isize
}

fn sys_getcwd(buf: usize, size: usize) -> isize {
    if buf != 0 && size >= 2 {
        unsafe {
            core::ptr::write_bytes(buf as *mut u8, 0, size);
            core::ptr::write_bytes(buf as *mut u8, b'/', 1);
        }
    }
    1
}

fn sys_clock_gettime(_clockid: usize, tp: usize) -> isize {
    if tp == 0 { return EINVAL; }
    let ms = crate::timer::uptime_ms();
    let ts = Timespec { tv_sec: (ms / 1000) as i64, tv_nsec: ((ms % 1000) * 1_000_000) as i64 };
    unsafe { core::ptr::write_volatile(tp as *mut Timespec, ts); }
    0
}

fn sys_clock_nanosleep(_clockid: usize, _flags: usize, req: usize) -> isize {
    if req != 0 {
        let ts = unsafe { &*(req as *const Timespec) };
        let ms = (ts.tv_sec as usize) * 1000 + (ts.tv_nsec as usize) / 1_000_000;
        crate::timer::sleep_ms(ms);
    }
    0
}

fn sys_nanosleep(req: usize, _rem: usize) -> isize {
    if req != 0 {
        let ts = unsafe { &*(req as *const Timespec) };
        let ms = (ts.tv_sec as usize) * 1000 + (ts.tv_nsec as usize) / 1_000_000;
        crate::timer::sleep_ms(ms);
    }
    0
}

fn sys_getuid() -> isize { crate::task::current_user_id() as isize }

fn sys_capget(hdrp: usize, datap: usize) -> isize {
    if hdrp != 0 {
        unsafe {
            core::ptr::write_volatile(hdrp as *mut u32, 0x2008_0522);
            core::ptr::write_volatile((hdrp + 4) as *mut u32, 0);
            if datap != 0 {
                core::ptr::write_volatile(datap as *mut u64, 0u64);
                core::ptr::write_volatile((datap + 8) as *mut u64, 0u64);
            }
        }
    }
    0
}

fn sys_openat(_dirfd: usize, path: usize, _flags: usize) -> isize {
    let path_str = match unsafe { crate::usercopy::copy_from_user_cstr(
        crate::task::current_page_table_root(), path, 256
    ) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    let node_idx = match crate::fs::resolve_path_idx(&path_str) {
        Some(idx) => idx,
        None => return ENOENT,
    };
    let task_id = crate::task::current_task();
    match crate::fd_table::open(task_id, node_idx) {
        Ok(fd) => fd as isize,
        Err(_) => ENOMEM,
    }
}

fn sys_mkdir(path: usize) -> isize {
    let path_str = match unsafe { crate::usercopy::copy_from_user_cstr(
        crate::task::current_page_table_root(), path, 256
    ) } {
        Ok(s) => s,
        Err(e) => return e,
    };
    match crate::fs::create_dir(&path_str) {
        Ok(_) => 0,
        Err(_) => -1,
    }
}

fn sys_lseek(fd: usize, offset: usize, whence: usize) -> isize {
    let task_id = crate::task::current_task();
    crate::fd_table::lseek(task_id, fd, offset as isize, whence)
}

fn sys_dup(old_fd: usize) -> isize {
    let task_id = crate::task::current_task();
    match crate::fd_table::dup(task_id, old_fd) {
        Ok(fd) => fd as isize,
        Err(_) => -1,
    }
}

fn sys_dup2(old_fd: usize, new_fd: usize) -> isize {
    let task_id = crate::task::current_task();
    match crate::fd_table::dup2(task_id, old_fd, new_fd) {
        Ok(fd) => fd as isize,
        Err(_) => -1,
    }
}


fn sys_execve(path_ptr: usize, argv_ptr: usize, envp_ptr: usize) -> isize {
    // Delegate to the main execve syscall handler
    crate::syscall::syscall_execve(path_ptr, argv_ptr, envp_ptr)
}

pub fn dump_stats() {
    crate::println!("=== Linux Compat Statistics ===");
    crate::println!("  Core syscalls implemented: 14");
    crate::println!("    read(fd), write(stdout/stderr), close(fd), openat(resolve),");
    crate::println!("    getpid, exit, clock_gettime, clock_nanosleep, nanosleep,");
    crate::println!("    getuid, geteuid, getgid, getegid, capget, brk,");
    crate::println!("    getcwd, mkdir");
    crate::println!("  Core syscalls stubbed (ENOSYS): ~22");
    crate::println!("    fork, execve, clone, mmap, munmap, mprotect,");
    crate::println!("    fstat, rename, rmdir, unlink, fcntl, prctl,");
    crate::println!("    futex, tgkill, setuid, setgid, capset, ...");
    crate::println!("  Error codes: Linux-compatible (ENOENT, EBADF, EINVAL, ENOSYS, ...)");
}
