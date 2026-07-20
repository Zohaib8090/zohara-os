// src/syscall.rs

//! Centralized syscall dispatch table with structured entries.
//!
//! ## Calling convention (FROZEN)
//!
//! - **x86_64:** number in `rax`, args in `rdi`/`rsi`/`rdx`, return in `rax`.
//!   Entry via `int 0x80`, return via `iretq`.
//!
//! ## Syscall number ranges
//!
//!   0–127    Core syscalls (this file)
//!   128–255  Filesystem (future)
//!   256–511  Networking (future)
//!   512–767  Graphics (future)
//!   768–1023 Reserved

use crate::usercopy::{self, MAX_USER_COPY};

// ---- Error codes (POSIX-like) ----

pub const ENOSYS: isize = -38;  // Unknown syscall
pub const EPERM:  isize = -13;  // Permission denied
pub const EINVAL: isize = -22;  // Invalid argument
pub const EFAULT: isize = -14;  // Bad pointer

// ---- Syscall flags ----

pub const FLAG_ROOT_ONLY:     u16 = 0x0001;
pub const FLAG_BLOCKING:      u16 = 0x0002;
pub const FLAG_FASTPATH:      u16 = 0x0004;
pub const FLAG_MAY_SLEEP:     u16 = 0x0008;
pub const FLAG_CAN_PAGEFAULT: u16 = 0x0010;

/// Syscall signature.
pub type SyscallFn = fn(arg0: usize, arg1: usize, arg2: usize) -> isize;

/// Structured syscall entry with metadata.
#[derive(Copy, Clone)]
pub struct SyscallEntry {
    pub name: &'static str,
    pub handler: Option<SyscallFn>,
    pub required_uid: Option<u32>,
    pub arg_count: u8,
    pub flags: u16,
}

impl SyscallEntry {
    const fn new(
        name: &'static str,
        handler: Option<SyscallFn>,
        required_uid: Option<u32>,
        arg_count: u8,
        flags: u16,
    ) -> Self {
        Self { name, handler, required_uid, arg_count, flags }
    }
}

// ---- Syscall numbers ----

#[repr(usize)]
pub enum Syscall {
    Write      = 0,
    Exit       = 1,
    Sleep      = 2,
    Read       = 3,
    GetPid     = 4,
    Yield      = 5,
    GetTime    = 6,
    GetUptime  = 7,
    Version    = 8,
    KernelInfo = 9,
    MemoryInfo = 10,
    TaskInfo   = 11,
    DebugLog   = 12,
    Execve     = 13,
}

// ---- Centralized dispatch table ----
// Slot 0-127: core syscalls, 128-255: filesystem (future), etc.

static SYSCALL_TABLE: [Option<SyscallEntry>; 256] = {
    let mut t: [Option<SyscallEntry>; 256] = [None; 256];

    t[0]  = Some(SyscallEntry::new("Write",      Some(syscall_write),      None, 3, FLAG_CAN_PAGEFAULT));
    t[1]  = Some(SyscallEntry::new("Exit",       Some(syscall_exit),       None, 0, 0));
    t[2]  = Some(SyscallEntry::new("Sleep",      Some(syscall_sleep),      None, 1, FLAG_MAY_SLEEP));
    t[3]  = Some(SyscallEntry::new("Read",       Some(syscall_read),       None, 3, FLAG_CAN_PAGEFAULT));
    t[4]  = Some(SyscallEntry::new("GetPid",     Some(syscall_getpid),     None, 0, FLAG_FASTPATH));
    t[5]  = Some(SyscallEntry::new("Yield",      Some(syscall_yield),      None, 0, 0));
    t[6]  = Some(SyscallEntry::new("GetTime",    Some(syscall_gettime),    None, 0, FLAG_FASTPATH));
    t[7]  = Some(SyscallEntry::new("GetUptime",  Some(syscall_getuptime),  None, 0, FLAG_FASTPATH));
    t[8]  = Some(SyscallEntry::new("Version",    Some(syscall_version),    None, 0, FLAG_FASTPATH));
    t[9]  = Some(SyscallEntry::new("KernelInfo", Some(syscall_kernelinfo), None, 0, FLAG_FASTPATH));
    t[10] = Some(SyscallEntry::new("MemoryInfo", Some(syscall_memoryinfo), None, 1, FLAG_CAN_PAGEFAULT));
    t[11] = Some(SyscallEntry::new("TaskInfo",   Some(syscall_taskinfo),   None, 2, FLAG_CAN_PAGEFAULT));
    t[12] = Some(SyscallEntry::new("DebugLog",   Some(syscall_debuglog),   Some(0), 2, FLAG_CAN_PAGEFAULT));
    t[13] = Some(SyscallEntry::new("Execve",     Some(syscall_execve),     None, 3, FLAG_CAN_PAGEFAULT));

    t
};

/// The single shared dispatch entry both architecture stubs call.
pub fn dispatch(num: usize, arg0: usize, arg1: usize, arg2: usize) -> isize {
    // Statistics
    crate::stats::count_syscall(num);

    // Bounds check
    if num >= SYSCALL_TABLE.len() {
        crate::warn!("syscall", "unknown syscall {} (out of range)", num);
        return ENOSYS;
    }

    let entry = match &SYSCALL_TABLE[num] {
        Some(e) => e,
        None => {
            crate::trace!("syscall", "unimplemented syscall {}", num);
            return ENOSYS;
        }
    };

    // Permission check
    if let Some(required_uid) = entry.required_uid {
        let caller_uid = crate::task::current_user_id();
        if caller_uid != required_uid {
            crate::warn!("syscall", "{} EPERM uid={} need={}", entry.name, caller_uid, required_uid);
            return EPERM;
        }
    }

    // Dispatch
    match entry.handler {
        Some(f) => {
            crate::trace!("syscall", "{}({:#x},{:#x},{:#x})", entry.name, arg0, arg1, arg2);
            f(arg0, arg1, arg2)
        }
        None => ENOSYS,
    }
}

// --- syscall handlers -------------------------------------------------------

const MAX_WRITE_LEN: usize = 256;

pub fn syscall_write(buf: usize, len: usize, _unused: usize) -> isize {
    if len > MAX_WRITE_LEN {
        return -1;
    }
    if crate::task::current_task() == 0 {
        let bytes = unsafe { core::slice::from_raw_parts(buf as *const u8, len) };
        for &b in bytes { crate::arch::write_serial(b); }
        return len as isize;
    }
    let cr3 = crate::task::current_page_table_root();
    let mut kbuf = [0u8; MAX_USER_COPY];
    let copied = unsafe {
        match usercopy::copy_from_user(cr3, buf, len, &mut kbuf) {
            Ok(n) => n,
            Err(e) => return e,
        }
    };
    for i in 0..copied { crate::arch::write_serial(kbuf[i]); }
    copied as isize
}

fn syscall_exit(_arg0: usize, _arg1: usize, _arg2: usize) -> isize {
    crate::stats::count_task_exit();
    crate::task::exit_current_task()
}

fn syscall_sleep(duration_ms: usize, _arg1: usize, _arg2: usize) -> isize {
    crate::task::sleep(duration_ms);
    0
}

fn syscall_read(_buf: usize, _len: usize, _arg2: usize) -> isize {
    0 // Stub: no input source for userspace tasks yet
}

fn syscall_getpid(_arg0: usize, _arg1: usize, _arg2: usize) -> isize {
    crate::task::current_task() as isize
}

fn syscall_yield(_arg0: usize, _arg1: usize, _arg2: usize) -> isize {
    crate::task::set_state(crate::task::TaskState::Ready);
    unsafe { core::arch::asm!("sti"); }
    loop {
        unsafe { core::arch::asm!("hlt"); }
        if crate::task::current_state() == crate::task::TaskState::Running {
            break;
        }
    }
    0
}

fn syscall_gettime(_a0: usize, _a1: usize, _a2: usize) -> isize {
    crate::timer::uptime_ms() as isize
}

fn syscall_getuptime(_a0: usize, _a1: usize, _a2: usize) -> isize {
    crate::timer::ticks() as isize
}

fn syscall_version(_a0: usize, _a1: usize, _a2: usize) -> isize {
    "Zohara 0.1.0\0".as_ptr() as isize
}

fn syscall_kernelinfo(_a0: usize, _a1: usize, _a2: usize) -> isize {
    // Pack: major=0, minor=1, arch=x86_64=0, features bitmap
    ((0u64 << 48) | (1u64 << 32) | (0u64 << 16) | 0x07) as isize
}

fn syscall_memoryinfo(buf: usize, _a1: usize, _a2: usize) -> isize {
    if crate::task::current_task() == 0 {
        // Kernel task — write directly
        let total = crate::frame::total_ram() / crate::frame::FRAME_SIZE;
        let free = crate::frame::free_frame_count();
        let data = [total as u64, free as u64, (total - free) as u64];
        unsafe {
            let dst = core::slice::from_raw_parts_mut(buf as *mut u64, 3);
            dst.copy_from_slice(&data);
        }
        return 0;
    }
    // User task — would need copy_to_user, but we don't have it yet.
    // For now, return EFAULT.
    EFAULT
}

fn syscall_taskinfo(pid: usize, buf: usize, _a2: usize) -> isize {
    if pid >= crate::task::MAX_TASKS {
        return EINVAL;
    }
    if crate::task::current_task() == 0 {
        // Kernel task — write task info directly
        let state_val = match crate::task::get_task_state(pid) {
            crate::task::TaskState::Unused  => 0u64,
            crate::task::TaskState::Ready   => 1,
            crate::task::TaskState::Running  => 2,
            crate::task::TaskState::Sleeping => 3,
        };
        let data = [pid as u64, state_val, crate::task::get_task_uid(pid) as u64];
        unsafe {
            let dst = core::slice::from_raw_parts_mut(buf as *mut u64, 3);
            dst.copy_from_slice(&data);
        }
        return 0;
    }
    EFAULT
}

fn syscall_debuglog(buf: usize, len: usize, _a2: usize) -> isize {
    if len > 256 { return EINVAL; }
    if crate::task::current_task() == 0 {
        let bytes = unsafe { core::slice::from_raw_parts(buf as *const u8, len) };
        for &b in bytes { crate::arch::write_serial(b); }
        return len as isize;
    }
    let cr3 = crate::task::current_page_table_root();
    let mut kbuf = [0u8; MAX_USER_COPY];
    let copied = unsafe {
        match usercopy::copy_from_user(cr3, buf, len, &mut kbuf) {
            Ok(n) => n,
            Err(e) => return e,
        }
    };
    for i in 0..copied { crate::arch::write_serial(kbuf[i]); }
    copied as isize
}


pub fn syscall_execve(path_ptr: usize, _argv_ptr: usize, _envp_ptr: usize) -> isize {
    // Step 1: Copy path from userspace BEFORE destroying address space
    let path = if crate::task::current_task() == 0 {
        // Kernel task: path is a direct pointer
        unsafe {
            let slice = core::slice::from_raw_parts(path_ptr as *const u8, 256);
            let end = slice.iter().position(|&b| b == 0).unwrap_or(256);
            match core::str::from_utf8(&slice[..end]) {
                Ok(s) => alloc::string::String::from(s),
                Err(_) => return -8, // ENOEXEC
            }
        }
    } else {
        let cr3 = crate::task::current_page_table_root();
        match unsafe { crate::usercopy::copy_from_user_cstr(cr3, path_ptr, 256) } {
            Ok(s) => s,
            Err(e) => return e,
        }
    };

    crate::println!("[execve] loading: {}", path);

    // Step 2: Read file from FAT32
    if !crate::fs::fat32::is_mounted() {
        crate::println!("[execve] error: no filesystem mounted");
        return -2; // ENOENT
    }
    let disk = crate::fs::fat32::disk();
    let fs = crate::fs::fat32::fs().unwrap();

    // Find the file in root directory
    let entries = match fs.readdir(&disk, fs.root_cluster()) {
        Ok(e) => e,
        Err(_) => return -2,
    };
    let entry = match entries.iter().find(|e| e.name.eq_ignore_ascii_case(&path)) {
        Some(e) if !e.is_dir && e.cluster >= 2 => e.clone(),
        _ => {
            crate::println!("[execve] file not found: {}", path);
            return -2; // ENOENT
        }
    };

    // Read file data into kernel buffer
    let file_size = entry.size as usize;
    if file_size == 0 || file_size > 1024 * 1024 {
        crate::println!("[execve] invalid file size: {}", file_size);
        return -8; // ENOEXEC
    }
    let mut elf_buf = alloc::vec![0u8; file_size];
    match fs.read_file_data(&disk, entry.cluster, entry.size, &mut elf_buf) {
        Ok(n) => {
            if n != file_size {
                crate::println!("[execve] short read: {} vs {}", n, file_size);
                return -8;
            }
        }
        Err(e) => {
            crate::println!("[execve] read failed: {}", e);
            return -8;
        }
    };

    // Step 3: Validate ELF
    let image = match crate::elf::parse_elf(&elf_buf) {
        Ok(img) => img,
        Err(e) => {
            crate::println!("[execve] ELF parse error: {:?}", e);
            return -8; // ENOEXEC
        }
    };

    crate::println!("[execve] entry={:#x}, {} segments", image.entry_point, image.segment_count);

    // Step 4: Replace the current task's address space
    let task_idx = crate::task::current_task();
    match crate::task::replace_address_space(task_idx, &image, &elf_buf) {
        Ok(stack_ptr) => {
            crate::println!("[execve] success, new RSP={:#x}, entry={:#x}", stack_ptr, image.entry_point);
            0
        }
        Err(e) => {
            crate::println!("[execve] failed: {}", e);
            -12 // ENOMEM
        }
    }
}

// --- init -------------------------------------------------------------------

pub fn init() {
    #[cfg(target_arch = "x86_64")]
    crate::arch::syscall::init();
    #[cfg(target_arch = "aarch64")]
    crate::arch::syscall::init();
}
