// src/process.rs

//! Process creation from ELF binaries.
//!
//! Provides a clean API for loading ELF64 executables into userspace
//! with argc/argv passing, proper stack setup, and process tracking.

use alloc::vec::Vec;
use crate::handles::{GenericHandle, ProcessMarker};

/// A process handle (typed wrapper around task index).
pub type ProcessHandle = GenericHandle<ProcessMarker, usize>;

/// Process information for tracking.
pub struct ProcessInfo {
    pub pid: usize,
    pub entry_point: usize,
    pub args: Vec<&'static str>,
}

/// Load an ELF64 binary and create a userspace process.
///
/// `elf_data` is the raw ELF file bytes.
/// `args` is the argument list (args[0] is typically the program name).
/// `name` is a debug label for the task.
///
/// Returns the process handle (task index) on success.
pub fn create_process(
    elf_data: &[u8],
    args: &[&'static str],
    name: &str,
) -> Result<ProcessHandle, &'static str> {
    // Parse the ELF
    let image = crate::elf::parse_elf(elf_data).map_err(|e| {
        crate::println!("[{}] ELF parse error: {:?}", name, e);
        "ELF parse failed"
    })?;

    if image.segment_count == 0 {
        return Err("no loadable segments");
    }

    // Collect segment info for page table creation
    let mut segments = Vec::new();
    for i in 0..image.segment_count {
        let seg = &image.segments[i];
        let file_offset = seg.data_ptr - elf_data.as_ptr() as usize;
        segments.push((seg.vaddr, file_offset, seg.filesz, seg.memsz, seg.flags));
    }

    // Create per-task page tables
    let (pt_root, user_phys_frames) = crate::paging::create_user_page_tables(&segments);

    // Copy segment data into the allocated frames
    for &(vaddr, filesz, phys_addr) in &user_phys_frames {
        let seg = segments.iter().find(|s| s.0 == vaddr);
        if let Some(&(_svaddr, file_offset, sfilesz, _smemsz, _sflags)) = seg {
            if sfilesz > 0 && filesz > 0 {
                let copy_len = filesz.min(sfilesz);
                let src = &elf_data[file_offset..file_offset + copy_len];
                unsafe {
                    let dst = core::slice::from_raw_parts_mut(phys_addr as *mut u8, copy_len);
                    dst.copy_from_slice(src);
                }
            }
        }
    }

    // Set up the user stack with argc/argv
    let stack_page = crate::paging::USER_BASE_VA + 0x100000; // stack page base
    let stack_top = crate::paging::USER_BASE_VA + 0x100000 + 0x1000; // one page above
    let user_sp = setup_user_stack(stack_page, args);

    // Spawn the task
    let frame_addrs: Vec<usize> = user_phys_frames.iter()
        .map(|&(_, _, pa)| pa)
        .collect();

    let task_idx = crate::task::spawn_userspace(
        image.entry_point,
        user_sp,
        pt_root,
        &frame_addrs,
    )?;

    crate::info!("proc", "created process '{}' pid={} entry={:#x} args={}",
        name, task_idx, image.entry_point, args.len());

    Ok(ProcessHandle::new(task_idx))
}

/// Set up the user stack with argc/argv data.
///
/// Stack layout (bottom of stack page):
///   [rsp+0]  = argc (u64)
///   [rsp+8]  = argv[0] pointer
///   [rsp+16] = argv[1] pointer
///   ...
///   [rsp+8*argc] = NULL (argv terminator)
///   [rsp+8*(argc+1)] = NULL (envp terminator)
///   ... then string data ...
///
/// Returns the initial RSP value (points to argc).
fn setup_user_stack(stack_page: usize, args: &[&str]) -> usize {
    // Calculate space needed for the string data
    let mut str_data_len = 0usize;
    for arg in args {
        str_data_len += arg.len() + 1; // +1 for null terminator
    }

    // Place argc/argv metadata at the top of the stack page,
    // and string data below it. RSP points to argc.
    //
    // We use the bottom of the stack page for everything:
    //   0x500000: argc, argv pointers, envp NULL, string data
    //   RSP = address of argc

    let base = stack_page; // 0x500000
    let header_size = 8 + (args.len() + 1) * 8 + 8; // argc + argv ptrs + NULL + envp NULL
    let str_area = base + header_size;

    unsafe {
        // Write argc
        let mut pos = base;
        core::ptr::write_volatile(pos as *mut u64, args.len() as u64);
        pos += 8;

        // Write argv pointers (pointing to string data in the stack page)
        let mut str_offset = 0usize;
        for arg in args {
            let str_ptr = str_area + str_offset;
            core::ptr::write_volatile(pos as *mut u64, str_ptr as u64);
            // Copy string data
            let dst = core::slice::from_raw_parts_mut(
                (str_area + str_offset) as *mut u8,
                arg.len() + 1,
            );
            dst[..arg.len()].copy_from_slice(arg.as_bytes());
            dst[arg.len()] = 0; // null terminator
            str_offset += arg.len() + 1;
            pos += 8;
        }

        // NULL terminator for argv
        core::ptr::write_volatile(pos as *mut u64, 0u64);
        pos += 8;

        // NULL terminator for envp (empty environment)
        core::ptr::write_volatile(pos as *mut u64, 0u64);
    }

    // RSP points to argc at the base of the stack page
    base
}

/// List all running processes (by scanning task table).
pub fn list_processes() {
    crate::println!("=== Processes ===");
    for i in 1..crate::task::MAX_TASKS {
        let state = crate::task::get_task_state(i);
        if state != crate::task::TaskState::Unused {
            let uid = crate::task::get_task_uid(i);
            crate::println!("  PID {:>3} | UID {:>5} | {:?}",
                i, uid, state);
        }
    }
}
