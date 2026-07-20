// src/fs/mod.rs

//! Virtual File System — generic filesystem abstraction.
//!
//! Provides path resolution, file creation/deletion, and directory listing.
//! The root filesystem is ramfs (in-memory).

pub mod node;
pub mod fd;
pub mod ramfs;
pub mod zohfs;
pub mod bcache;
pub mod gpt;
pub mod fat32;
pub mod ext2;

use alloc::vec::Vec;
use node::{VfsNode, VfsNodeType};

/// Global VFS state using Vec-based storage.
pub struct Vfs {
    nodes: Vec<VfsNode>,
    next_inode: u64,
}

impl Vfs {
    pub fn new() -> Self {
        let mut fs = Self {
            nodes: Vec::new(),
            next_inode: 1,
        };
        // Create root directory
        fs.nodes.push(VfsNode::new_dir(b"/", 0));
        fs.nodes[0].inode = fs.next_inode;
        fs.next_inode += 1;
        fs
    }

    /// Allocate a new VFS node. Returns its index.
    fn alloc_node(&mut self) -> Option<usize> {
        let idx = self.nodes.len();
        self.nodes.push(VfsNode::empty());
        self.nodes[idx].inode = self.next_inode;
        self.next_inode += 1;
        Some(idx)
    }

    /// Create a file in the given parent directory.
    pub fn create_file(&mut self, parent_idx: usize, name: &[u8]) -> Result<usize, &'static str> {
        let child_idx = self.alloc_node().ok_or("VFS node table full")?;
        self.nodes[child_idx] = VfsNode::new_file(name, parent_idx);
        self.nodes[parent_idx].children.push(child_idx);
        Ok(child_idx)
    }

    /// Create a directory in the given parent directory.
    pub fn create_dir(&mut self, parent_idx: usize, name: &[u8]) -> Result<usize, &'static str> {
        let child_idx = self.alloc_node().ok_or("VFS node table full")?;
        self.nodes[child_idx] = VfsNode::new_dir(name, parent_idx);
        self.nodes[parent_idx].children.push(child_idx);
        Ok(child_idx)
    }

    /// Look up a child by name within a parent directory node.
    pub fn lookup_child(&self, parent_idx: usize, name: &[u8]) -> Option<usize> {
        let parent = &self.nodes[parent_idx];
        for &child_idx in &parent.children {
            if child_idx < self.nodes.len() {
                let child = &self.nodes[child_idx];
                if child.name_len == name.len() && &child.name[..child.name_len] == name {
                    return Some(child_idx);
                }
            }
        }
        None
    }

    /// Resolve a path to a VFS node index.
    pub fn resolve_path(&self, path: &str) -> Option<usize> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return Some(0); // root
        }

        let mut current = 0usize;
        for component in path.split('/') {
            if component.is_empty() || component == "." {
                continue;
            }
            if component == ".." {
                let parent = self.nodes[current].parent;
                if parent != 0 && parent < self.nodes.len() {
                    current = parent;
                }
                continue;
            }
            current = self.lookup_child(current, component.as_bytes())?;
        }
        Some(current)
    }

    /// Get a reference to a node by index.
    pub fn get_node(&self, idx: usize) -> Option<&VfsNode> {
        self.nodes.get(idx)
    }

    /// Get a mutable reference to a node by index.
    pub fn get_node_mut(&mut self, idx: usize) -> Option<&mut VfsNode> {
        self.nodes.get_mut(idx)
    }

    /// Delete a node (file or empty directory).
    pub fn delete_node(&mut self, idx: usize) -> Result<(), &'static str> {
        if idx == 0 {
            return Err("cannot delete root");
        }
        if idx >= self.nodes.len() {
            return Err("invalid node");
        }
        if self.nodes[idx].node_type == VfsNodeType::Directory && !self.nodes[idx].children.is_empty() {
            return Err("directory not empty");
        }

        // Remove from parent's children list
        let parent_idx = self.nodes[idx].parent;
        if parent_idx < self.nodes.len() {
            self.nodes[parent_idx].children.retain(|&c| c != idx);
        }

        // Clear the node (keep the slot for now to avoid invalidating indices)
        self.nodes[idx] = VfsNode::empty();
        Ok(())
    }

    /// List directory entries.
    pub fn readdir(&self, idx: usize) -> Vec<(&str, u64, VfsNodeType)> {
        let mut entries = Vec::new();
        if idx >= self.nodes.len() {
            return entries;
        }
        let node = &self.nodes[idx];
        if node.node_type != VfsNodeType::Directory {
            return entries;
        }
        for &child_idx in &node.children {
            if child_idx < self.nodes.len() {
                let child = &self.nodes[child_idx];
                let name = core::str::from_utf8(&child.name[..child.name_len]).unwrap_or("?");
                entries.push((name, child.inode, child.node_type));
            }
        }
        entries
    }

    /// Print VFS statistics.
    pub fn stats(&self) {
        let mut files = 0;
        let mut dirs = 0;
        for node in &self.nodes {
            match node.node_type {
                VfsNodeType::File => files += 1,
                VfsNodeType::Directory => dirs += 1,
                _ => {}
            }
        }
        crate::println!("=== VFS Statistics ===");
        crate::println!("  Total nodes:  {}", self.nodes.len());
        crate::println!("  Files:        {}", files);
        crate::println!("  Directories:  {}", dirs);
    }
}

/// Global VFS instance.
pub static mut VFS: Option<Vfs> = None;

/// Initialize the VFS.
pub fn init() {
    unsafe { VFS = Some(Vfs::new()); }
    crate::info!("vfs", "initialized with root directory");
}

/// Helper to access VFS.
fn with_vfs<F, R>(f: F) -> R
where
    F: FnOnce(&Vfs) -> R,
{
    unsafe {
        let vfs = VFS.as_ref().expect("VFS not initialized");
        f(vfs)
    }
}

fn with_vfs_mut<F, R>(f: F) -> R
where
    F: FnOnce(&mut Vfs) -> R,
{
    unsafe {
        let vfs = VFS.as_mut().expect("VFS not initialized");
        f(vfs)
    }
}

/// Create a file at the given path.
pub fn create_file(path: &str) -> Result<usize, &'static str> {
    let (parent_path, name) = split_path(path)?;
    with_vfs_mut(|vfs| {
        let parent_idx = vfs.resolve_path(parent_path).ok_or("parent not found")?;
        vfs.create_file(parent_idx, name.as_bytes())
    })
}

/// Create a directory at the given path.
pub fn create_dir(path: &str) -> Result<usize, &'static str> {
    let (parent_path, name) = split_path(path)?;
    with_vfs_mut(|vfs| {
        let parent_idx = vfs.resolve_path(parent_path).ok_or("parent not found")?;
        vfs.create_dir(parent_idx, name.as_bytes())
    })
}

/// Read data from a file at the given offset.
pub fn read_file(path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    with_vfs(|vfs| {
        let idx = vfs.resolve_path(path).ok_or("file not found")?;
        let node = &vfs.get_node(idx).ok_or("invalid node")?;
        if node.node_type != VfsNodeType::File {
            return Err("not a file");
        }
        let data = &node.data;
        if offset >= data.len() {
            return Ok(0);
        }
        let available = data.len() - offset;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&data[offset..offset + to_read]);
        Ok(to_read)
    })
}

/// Write data to a file at the given offset.
pub fn write_file(path: &str, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
    with_vfs_mut(|vfs| {
        let idx = vfs.resolve_path(path).ok_or("file not found")?;
        let node = vfs.get_node_mut(idx).ok_or("invalid node")?;
        if node.node_type != VfsNodeType::File {
            return Err("not a file");
        }
        let end = offset + data.len();
        if end > node.data.len() {
            node.data.resize(end, 0);
        }
        node.data[offset..offset + data.len()].copy_from_slice(data);
        node.size = node.data.len();
        Ok(data.len())
    })
}

/// Read data from a VFS node by index (for fd-based I/O).
pub fn read_file_by_node(node_idx: usize, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    with_vfs(|vfs| {
        let node = vfs.get_node(node_idx).ok_or("invalid node")?;
        if node.node_type != VfsNodeType::File {
            return Err("not a file");
        }
        let data = &node.data;
        if offset >= data.len() {
            return Ok(0);
        }
        let available = data.len() - offset;
        let to_read = buf.len().min(available);
        buf[..to_read].copy_from_slice(&data[offset..offset + to_read]);
        Ok(to_read)
    })
}

/// Write data to a VFS node by index (for fd-based I/O).
pub fn write_file_by_node(node_idx: usize, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
    with_vfs_mut(|vfs| {
        let node = vfs.get_node_mut(node_idx).ok_or("invalid node")?;
        if node.node_type != VfsNodeType::File {
            return Err("not a file");
        }
        let end = offset + data.len();
        if end > node.data.len() {
            node.data.resize(end, 0);
        }
        node.data[offset..offset + data.len()].copy_from_slice(data);
        node.size = node.data.len();
        Ok(data.len())
    })
}

/// Resolve a path to a VFS node index (public wrapper).
pub fn resolve_path_idx(path: &str) -> Option<usize> {
    with_vfs(|vfs| vfs.resolve_path(path))
}

/// Delete a file or empty directory at the given path.
pub fn delete(path: &str) -> Result<(), &'static str> {
    let (parent_path, name) = split_path(path)?;
    with_vfs_mut(|vfs| {
        let parent_idx = vfs.resolve_path(parent_path).ok_or("parent not found")?;
        let child_idx = vfs.lookup_child(parent_idx, name.as_bytes()).ok_or("not found")?;
        vfs.delete_node(child_idx)
    })
}

/// List directory entries (returns owned data).
pub fn readdir(path: &str) -> Vec<(alloc::string::String, u64, VfsNodeType)> {
    unsafe {
        let vfs = match VFS.as_ref() {
            Some(v) => v,
            None => return Vec::new(),
        };
        let mut entries = Vec::new();
        if let Some(idx) = vfs.resolve_path(path) {
            if let Some(node) = vfs.get_node(idx) {
                if node.node_type == VfsNodeType::Directory {
                    for &child_idx in &node.children {
                        if let Some(child) = vfs.get_node(child_idx) {
                            if child.name_len > 0 {
                                let name = alloc::string::String::from(
                                    core::str::from_utf8(&child.name[..child.name_len])
                                    .unwrap_or("?")
                                );
                                entries.push((name, child.inode, child.node_type));
                            }
                        }
                    }
                }
            }
        }
        entries
    }
}

/// Split a path into (parent, name).
fn split_path(path: &str) -> Result<(&str, &str), &'static str> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err("empty path");
    }
    match path.rfind('/') {
        Some(pos) => {
            let parent = if pos == 0 { "/" } else { &path[..pos] };
            let name = &path[pos + 1..];
            if name.is_empty() {
                return Err("empty name");
            }
            Ok((parent, name))
        }
        None => Ok(("/", path)),
    }
}

/// Get the size of a file by node index.
pub fn get_file_size(node_idx: usize) -> usize {
    with_vfs(|vfs| {
        match vfs.get_node(node_idx) {
            Some(node) => node.size,
            None => 0,
        }
    })
}
