// src/fd_table.rs

//! Global file descriptor table — maps (task_id, fd) → (node_idx, offset).
//!
//! Avoids modifying the Task struct (which caused page faults when made larger).
//! Uses a flat array indexed by task_id * MAX_FDS + fd_number.

use crate::fs::node::VfsNodeType;

/// Maximum file descriptors per task.
pub const MAX_FDS: usize = 32;

/// An fd entry: VFS node index + current offset.
#[derive(Clone, Copy)]
pub struct FdEntry {
    pub node_idx: i32,   // -1 = unused
    pub offset: usize,
}

impl FdEntry {
    pub const fn empty() -> Self {
        Self { node_idx: -1, offset: 0 }
    }
}

/// Global fd table: MAX_TASKS × MAX_FDS entries.
static mut FD_TABLE: [FdEntry; crate::task::MAX_TASKS * MAX_FDS] = {
    // const initialization: all entries empty
    let mut table = [FdEntry::empty(); crate::task::MAX_TASKS * MAX_FDS];
    table
};

/// Get the flat index for (task_id, fd).
fn fd_index(task_id: usize, fd: usize) -> usize {
    task_id * MAX_FDS + fd
}

/// Open a file descriptor for a task. Returns the fd number.
pub fn open(task_id: usize, vfs_node_idx: usize) -> Result<usize, &'static str> {
    if task_id == 0 {
        return Err("kernel task cannot open fds");
    }
    for fd in 0..MAX_FDS {
        let idx = fd_index(task_id, fd);
        unsafe {
            if FD_TABLE[idx].node_idx == -1 {
                FD_TABLE[idx] = FdEntry {
                    node_idx: vfs_node_idx as i32,
                    offset: 0,
                };
                return Ok(fd);
            }
        }
    }
    Err("too many open files")
}

/// Close a file descriptor.
pub fn close(task_id: usize, fd: usize) -> Result<(), &'static str> {
    if fd >= MAX_FDS {
        return Err("bad file descriptor");
    }
    let idx = fd_index(task_id, fd);
    unsafe {
        if FD_TABLE[idx].node_idx == -1 {
            return Err("bad file descriptor");
        }
        FD_TABLE[idx] = FdEntry::empty();
    }
    Ok(())
}

/// Get the VFS node index for an fd.
pub fn node(task_id: usize, fd: usize) -> Result<usize, &'static str> {
    if fd >= MAX_FDS { return Err("bad file descriptor"); }
    let idx = fd_index(task_id, fd);
    unsafe {
        if FD_TABLE[idx].node_idx == -1 {
            return Err("bad file descriptor");
        }
        Ok(FD_TABLE[idx].node_idx as usize)
    }
}

/// Get the current offset for an fd.
pub fn offset(task_id: usize, fd: usize) -> usize {
    if fd >= MAX_FDS { return 0; }
    unsafe { FD_TABLE[fd_index(task_id, fd)].offset }
}

/// Set the offset for an fd.
pub fn set_offset(task_id: usize, fd: usize, new_offset: usize) {
    if fd < MAX_FDS {
        unsafe { FD_TABLE[fd_index(task_id, fd)].offset = new_offset; }
    }
}

/// Get the number of open fds for a task.
pub fn open_count(task_id: usize) -> usize {
    let mut count = 0;
    for fd in 0..MAX_FDS {
        unsafe {
            if FD_TABLE[fd_index(task_id, fd)].node_idx != -1 {
                count += 1;
            }
        }
    }
    count
}

/// Print fd table for a task.
pub fn dump(task_id: usize) {
    crate::println!("  FD table for task {}:", task_id);
    for fd in 0..MAX_FDS {
        unsafe {
            let entry = &FD_TABLE[fd_index(task_id, fd)];
            if entry.node_idx != -1 {
                crate::println!("    fd {}: node={}, offset={}", fd, entry.node_idx, entry.offset);
            }
        }
    }
}

/// Duplicate an fd: find the lowest unused fd and point it at the same node/offset.
pub fn dup(task_id: usize, old_fd: usize) -> Result<usize, &'static str> {
    if old_fd >= MAX_FDS { return Err("bad file descriptor"); }
    let old_idx = fd_index(task_id, old_fd);
    unsafe {
        if FD_TABLE[old_idx].node_idx == -1 {
            return Err("bad file descriptor");
        }
        let old_entry = FD_TABLE[old_idx];
        for fd in 0..MAX_FDS {
            let idx = fd_index(task_id, fd);
            if FD_TABLE[idx].node_idx == -1 {
                FD_TABLE[idx] = old_entry;
                return Ok(fd);
            }
        }
    }
    Err("too many open files")
}

/// Duplicate old_fd to new_fd. Closes new_fd first if it was open.
pub fn dup2(task_id: usize, old_fd: usize, new_fd: usize) -> Result<usize, &'static str> {
    if old_fd >= MAX_FDS || new_fd >= MAX_FDS {
        return Err("bad file descriptor");
    }
    if old_fd == new_fd { return Ok(new_fd); }
    let old_idx = fd_index(task_id, old_fd);
    unsafe {
        if FD_TABLE[old_idx].node_idx == -1 {
            return Err("bad file descriptor");
        }
        let old_entry = FD_TABLE[old_idx];
        let new_idx = fd_index(task_id, new_fd);
        FD_TABLE[new_idx] = old_entry;
    }
    Ok(new_fd)
}

/// Change the offset of an fd. Returns the new offset.
pub fn lseek(task_id: usize, fd: usize, offset: isize, whence: usize) -> isize {
    if fd >= MAX_FDS { return -1; }
    let idx = fd_index(task_id, fd);
    unsafe {
        if FD_TABLE[idx].node_idx == -1 { return -1; }
        let node_idx = FD_TABLE[idx].node_idx as usize;
        let file_size = crate::fs::get_file_size(node_idx);
        let new_offset = match whence {
            0 => offset as usize,
            1 => FD_TABLE[idx].offset.wrapping_add(offset as usize),
            2 => file_size.wrapping_add(offset as usize),
            _ => return -1,
        };
        FD_TABLE[idx].offset = new_offset;
        new_offset as isize
    }
}
