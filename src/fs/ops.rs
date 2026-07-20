// src/fs/ops.rs

//! Filesystem operations trait — the interface every filesystem must implement.

/// Trait that all filesystems must implement.
pub trait FileSystem {
    /// Get the filesystem name.
    fn name(&self) -> &str;

    /// Read data from an inode at a given offset.
    fn read(&self, inode: u64, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str>;

    /// Write data to an inode at a given offset.
    fn write(&mut self, inode: u64, offset: usize, data: &[u8]) -> Result<usize, &'static str>;

    /// Create a new file.
    fn create_file(&mut self, parent_inode: u64, name: &[u8]) -> Result<u64, &'static str>;

    /// Create a new directory.
    fn create_dir(&mut self, parent_inode: u64, name: &[u8]) -> Result<u64, &'static str>;

    /// Look up a child by name within a directory.
    fn lookup(&self, parent_inode: u64, name: &[u8]) -> Option<u64>;

    /// Delete a file or empty directory.
    fn delete(&mut self, inode: u64) -> Result<(), &'static str>;

    /// List directory entries.
    fn readdir(&self, inode: u64) -> alloc::vec::Vec<DirEntry>;

    /// Get file metadata.
    fn stat(&self, inode: u64) -> Result<FileStat, &'static str>;
}

/// A single directory entry.
pub struct DirEntry {
    pub name: alloc::string::String,
    pub inode: u64,
    pub is_dir: bool,
}

/// File metadata (stat result).
pub struct FileStat {
    pub inode: u64,
    pub size: usize,
    pub is_dir: bool,
    pub permissions: u16,
}
