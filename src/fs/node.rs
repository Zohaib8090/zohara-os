// src/fs/node.rs

//! VFS node types — the fundamental building blocks of the filesystem.

use alloc::vec::Vec;

/// Maximum length of a node name.
const NAME_LEN: usize = 60;

/// Type of VFS node.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum VfsNodeType {
    File,
    Directory,
    Symlink,
}

/// A single VFS node (file, directory, or symlink).
pub struct VfsNode {
    /// Node name (up to 60 bytes).
    pub name: [u8; NAME_LEN],
    /// Actual length of the name.
    pub name_len: usize,
    /// Type of this node.
    pub node_type: VfsNodeType,
    /// Inode number (unique identifier).
    pub inode: u64,
    /// Index of parent node in the VFS node table.
    pub parent: usize,
    /// Indices of child nodes (for directories).
    pub children: Vec<usize>,
    /// File content (empty for directories and symlinks).
    pub data: Vec<u8>,
    /// Size in bytes.
    pub size: usize,
    /// Permission bits (rwxrwxrwx).
    pub permissions: u16,
    /// Reference count.
    pub ref_count: u32,
}

impl VfsNode {
    /// Create an empty (deleted) node.
    pub const fn empty() -> Self {
        Self {
            name: [0; NAME_LEN],
            name_len: 0,
            node_type: VfsNodeType::File,
            inode: 0,
            parent: 0,
            children: Vec::new(),
            data: Vec::new(),
            size: 0,
            permissions: 0o644, // rw-r--r--
            ref_count: 0,
        }
    }

    /// Create a new file node.
    pub fn new_file(name: &[u8], parent: usize) -> Self {
        let mut node = Self::empty();
        let len = name.len().min(NAME_LEN);
        node.name[..len].copy_from_slice(&name[..len]);
        node.name_len = len;
        node.node_type = VfsNodeType::File;
        node.parent = parent;
        node.permissions = 0o644;
        node
    }

    /// Create a new directory node.
    pub fn new_dir(name: &[u8], parent: usize) -> Self {
        let mut node = Self::empty();
        let len = name.len().min(NAME_LEN);
        node.name[..len].copy_from_slice(&name[..len]);
        node.name_len = len;
        node.node_type = VfsNodeType::Directory;
        node.parent = parent;
        node.permissions = 0o755; // rwxr-xr-x
        node
    }

    /// Get the node name as a string slice.
    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }

    /// Check if this node is a directory.
    pub fn is_dir(&self) -> bool {
        self.node_type == VfsNodeType::Directory
    }

    /// Check if this node is a file.
    pub fn is_file(&self) -> bool {
        self.node_type == VfsNodeType::File
    }
}
