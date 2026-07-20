// src/fs/fd.rs

//! File descriptor table — per-process open file tracking.

/// Maximum file descriptors per process.
const MAX_FDS: usize = 64;

/// Open mode for a file descriptor.
#[derive(Clone, Copy, PartialEq)]
pub enum OpenMode {
    Read,
    Write,
    ReadWrite,
    Append,
}

/// A file descriptor entry.
#[derive(Clone, Copy)]
pub struct FdEntry {
    /// VFS node index (-1 = unused).
    pub node_idx: i32,
    /// Current offset in the file.
    pub offset: usize,
    /// Open mode.
    pub mode: OpenMode,
    /// Whether this FD is in use.
    pub in_use: bool,
}

impl FdEntry {
    const fn empty() -> Self {
        Self {
            node_idx: -1,
            offset: 0,
            mode: OpenMode::Read,
            in_use: false,
        }
    }
}

/// Per-process file descriptor table.
pub struct FdTable {
    entries: [FdEntry; MAX_FDS],
}

impl FdTable {
    pub const fn new() -> Self {
        Self {
            entries: [FdEntry::empty(); MAX_FDS],
        }
    }

    /// Open a file descriptor for a VFS node.
    pub fn open(&mut self, node_idx: usize, mode: OpenMode) -> Result<usize, &'static str> {
        for i in 0..MAX_FDS {
            if !self.entries[i].in_use {
                self.entries[i] = FdEntry {
                    node_idx: node_idx as i32,
                    offset: 0,
                    mode,
                    in_use: true,
                };
                return Ok(i);
            }
        }
        Err("too many open files")
    }

    /// Close a file descriptor.
    pub fn close(&mut self, fd: usize) -> Result<(), &'static str> {
        if fd >= MAX_FDS || !self.entries[fd].in_use {
            return Err("bad file descriptor");
        }
        self.entries[fd] = FdEntry::empty();
        Ok(())
    }

    /// Get a reference to a file descriptor entry.
    pub fn get(&self, fd: usize) -> Option<&FdEntry> {
        if fd < MAX_FDS && self.entries[fd].in_use {
            Some(&self.entries[fd])
        } else {
            None
        }
    }

    /// Get a mutable reference to a file descriptor entry.
    pub fn get_mut(&mut self, fd: usize) -> Option<&mut FdEntry> {
        if fd < MAX_FDS && self.entries[fd].in_use {
            Some(&mut self.entries[fd])
        } else {
            None
        }
    }

    /// Check if a file descriptor is valid.
    pub fn is_valid(&self, fd: usize) -> bool {
        fd < MAX_FDS && self.entries[fd].in_use
    }

    /// Get the number of open file descriptors.
    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|e| e.in_use).count()
    }
}
