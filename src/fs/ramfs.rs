// src/fs/ramfs.rs

//! RAM-backed filesystem — the initial in-memory filesystem.
//!
//! Provides a simple, heap-allocated filesystem that stores all data
//! in memory. Used as the root filesystem and for testing VFS operations.

use alloc::vec::Vec;

/// RAM filesystem node (mirrors VfsNode but is self-contained).
pub struct RamFsNode {
    pub name: [u8; 60],
    pub name_len: usize,
    pub is_dir: bool,
    pub children: Vec<usize>,
    pub data: Vec<u8>,
    pub inode: u64,
}

impl RamFsNode {
    pub fn new_file(name: &[u8], inode: u64) -> Self {
        let mut node = Self {
            name: [0; 60],
            name_len: 0,
            is_dir: false,
            children: Vec::new(),
            data: Vec::new(),
            inode,
        };
        let len = name.len().min(60);
        node.name[..len].copy_from_slice(&name[..len]);
        node.name_len = len;
        node
    }

    pub fn new_dir(name: &[u8], inode: u64) -> Self {
        let mut node = Self {
            name: [0; 60],
            name_len: 0,
            is_dir: true,
            children: Vec::new(),
            data: Vec::new(),
            inode,
        };
        let len = name.len().min(60);
        node.name[..len].copy_from_slice(&name[..len]);
        node.name_len = len;
        node
    }

    pub fn name_str(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("")
    }
}

/// RAM filesystem instance.
pub struct RamFs {
    nodes: Vec<RamFsNode>,
    next_inode: u64,
}

impl RamFs {
    pub fn new() -> Self {
        let mut fs = Self {
            nodes: Vec::new(),
            next_inode: 1,
        };
        // Create root directory
        fs.nodes.push(RamFsNode::new_dir(b"/", fs.next_inode));
        fs.next_inode += 1;
        fs
    }

    /// Allocate a new inode number.
    fn alloc_inode(&mut self) -> u64 {
        let inode = self.next_inode;
        self.next_inode += 1;
        inode
    }

    /// Create a file in the given parent directory.
    pub fn create_file(&mut self, parent_idx: usize, name: &[u8]) -> Result<usize, &'static str> {
        let inode = self.alloc_inode();
        let child_idx = self.nodes.len();
        self.nodes.push(RamFsNode::new_file(name, inode));
        self.nodes[parent_idx].children.push(child_idx);
        Ok(child_idx)
    }

    /// Create a directory in the given parent directory.
    pub fn create_dir(&mut self, parent_idx: usize, name: &[u8]) -> Result<usize, &'static str> {
        let inode = self.alloc_inode();
        let child_idx = self.nodes.len();
        self.nodes.push(RamFsNode::new_dir(name, inode));
        self.nodes[parent_idx].children.push(child_idx);
        Ok(child_idx)
    }

    /// Look up a child by name.
    pub fn lookup(&self, parent_idx: usize, name: &[u8]) -> Option<usize> {
        for &child_idx in &self.nodes[parent_idx].children {
            let child = &self.nodes[child_idx];
            if child.name_len == name.len() && &child.name[..child.name_len] == name {
                return Some(child_idx);
            }
        }
        None
    }

    /// Resolve a path to a node index.
    pub fn resolve(&self, path: &str) -> Option<usize> {
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
                // For simplicity, stay at current (root has no parent)
                continue;
            }
            current = self.lookup(current, component.as_bytes())?;
        }
        Some(current)
    }

    /// Read data from a node.
    pub fn read(&self, idx: usize, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if idx >= self.nodes.len() {
            return Err("invalid node");
        }
        let node = &self.nodes[idx];
        if node.is_dir {
            return Err("is a directory");
        }
        if offset >= node.data.len() {
            return Ok(0);
        }
        let avail = node.data.len() - offset;
        let n = buf.len().min(avail);
        buf[..n].copy_from_slice(&node.data[offset..offset + n]);
        Ok(n)
    }

    /// Write data to a node.
    pub fn write(&mut self, idx: usize, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
        if idx >= self.nodes.len() {
            return Err("invalid node");
        }
        let node = &mut self.nodes[idx];
        if node.is_dir {
            return Err("is a directory");
        }
        let end = offset + data.len();
        if end > node.data.len() {
            node.data.resize(end, 0);
        }
        node.data[offset..offset + data.len()].copy_from_slice(data);
        Ok(data.len())
    }

    /// Delete a node (must be empty directory or file).
    pub fn delete(&mut self, idx: usize) -> Result<(), &'static str> {
        if idx == 0 {
            return Err("cannot delete root");
        }
        if self.nodes[idx].is_dir && !self.nodes[idx].children.is_empty() {
            return Err("directory not empty");
        }

        // Remove from parent's children
        // We need to find the parent and remove idx from its children list
        // without borrowing self.nodes twice.
        let parent_inode = self.nodes[idx].inode;
        // Find parent by scanning — for simplicity, just clear the node
        self.nodes[idx].name_len = 0;
        self.nodes[idx].data.clear();
        self.nodes[idx].children.clear();
        self.nodes[idx].is_dir = false;
        Ok(())
    }

    /// List directory entries.
    pub fn readdir(&self, idx: usize) -> Vec<(&str, u64, bool)> {
        let mut entries = Vec::new();
        if idx >= self.nodes.len() {
            return entries;
        }
        let node = &self.nodes[idx];
        if !node.is_dir {
            return entries;
        }
        for &child_idx in &node.children {
            if child_idx < self.nodes.len() {
                let child = &self.nodes[child_idx];
                if child.name_len > 0 {
                    entries.push((child.name_str(), child.inode, child.is_dir));
                }
            }
        }
        entries
    }

    /// Get node count.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}
