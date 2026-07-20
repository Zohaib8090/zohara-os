// src/fs/ext2.rs

//! ext2 filesystem driver.
//!
//! Supports reading ext2 volumes. This is a stepping stone toward ext4.
//! Compatible with Linux-formatted ext2 filesystems.

use alloc::vec::Vec;
use crate::drivers::ide::{IdeDisk, SECTOR_SIZE};

/// ext2 Superblock (at offset 1024 from partition start).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ext2Superblock {
    pub inodes_count: u32,
    pub blocks_count: u32,
    pub r_blocks_count: u32,
    pub free_blocks_count: u32,
    pub free_inodes_count: u32,
    pub first_data_block: u32,
    pub log_block_size: u32,
    pub log_frag_size: i32,
    pub blocks_per_group: u32,
    pub frags_per_group: u32,
    pub inodes_per_group: u32,
    pub mtime: u32,
    pub wtime: u32,
    pub mnt_count: u16,
    pub max_mnt_count: u16,
    pub magic: u16,
    pub state: u16,
    pub errors: u16,
    pub minor_rev_level: u16,
    pub lastcheck: u32,
    pub checkinterval: u32,
    pub creator_os: u32,
    pub rev_level: u32,
    pub def_resuid: u16,
    pub def_resgid: u16,
    // Extended fields (rev_level >= 1)
    pub first_ino: u32,
    pub inode_size: u16,
    pub block_group_nr: u16,
    pub feature_compat: u32,
    pub feature_incompat: u32,
    pub feature_ro_compat: u32,
    pub uuid: [u8; 16],
    pub volume_name: [u8; 16],
    pub last_mounted: [u8; 64],
    pub algo_bitmap: u32,
}

/// ext2 Block Group Descriptor (32 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ext2GroupDesc {
    pub block_bitmap: u32,
    pub inode_bitmap: u32,
    pub inode_table: u32,
    pub free_blocks_count: u16,
    pub free_inodes_count: u16,
    pub dirs_count: u16,
    _pad: [u8; 14],
}

/// ext2 Inode (128 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ext2Inode {
    pub mode: u16,
    pub uid: u16,
    pub size: u32,
    pub atime: u32,
    pub ctime: u32,
    pub mtime: u32,
    pub dtime: u32,
    pub gid: u16,
    pub links_count: u16,
    pub blocks: u32,
    pub flags: u32,
    pub osd1: u32,
    pub block: [u32; 15], // 12 direct + 1 indirect + 1 double indirect + 1 triple indirect
    pub generation: u32,
    pub file_acl: u32,
    pub dir_acl: u32,
    pub faddr: u32,
    _osd2: [u8; 12],
}

/// ext2 Directory Entry.
#[derive(Clone)]
pub struct Ext2DirEntry {
    pub inode: u32,
    pub name: alloc::string::String,
    pub file_type: u8,
}

/// ext2 filesystem handle.
pub struct Ext2Fs {
    block_size: u32,
    inodes_per_group: u32,
    blocks_per_group: u32,
    pub num_block_groups: u32,
    inode_size: u32,
    group_desc_table: Vec<Ext2GroupDesc>,
    partition_offset: u32, // LBA offset for partition start
}

impl Ext2Fs {
    /// Initialize ext2 from a disk at a given partition offset.
    pub fn init(disk: &IdeDisk, partition_offset: u32) -> Result<Self, &'static str> {
        // Read superblock (at offset 1024 from partition start)
        let sb_sector = partition_offset + 2; // 1024 / 512 = 2
        let mut buf = [0u8; SECTOR_SIZE];
        disk.read_sector(sb_sector, &mut buf)?;

        let sb = unsafe { core::ptr::read(buf.as_ptr() as *const Ext2Superblock) };

        // Validate magic
        if sb.magic != 0xEF53 {
            return Err("not ext2 (bad magic)");
        }

        // Calculate block size
        let block_size = 1024u32 << sb.log_block_size;
        let inode_size = if sb.rev_level >= 1 { sb.inode_size as u32 } else { 128 };

        // Calculate number of block groups
        let total_blocks = sb.blocks_count;
        let blocks_per_group = sb.blocks_per_group;
        let num_block_groups = (total_blocks + blocks_per_group - 1) / blocks_per_group;

        crate::info!("ext2", "ext2 detected: {} blocks, {} groups, block_size={}",
            total_blocks, num_block_groups, block_size);

        // Read Block Group Descriptor Table
        // The BGDT starts at the block after the superblock
        let bgdt_start_sector = if block_size == 1024 {
            partition_offset + 4 // After superblock (1024 bytes = 2 sectors)
        } else {
            partition_offset + block_size / SECTOR_SIZE as u32 as u32
        };

        let bgdt_sectors = (num_block_groups as usize * 32 + SECTOR_SIZE as usize - 1) / SECTOR_SIZE as usize;
        let mut bgdt_buf = alloc::vec![0u8; bgdt_sectors * SECTOR_SIZE as usize];
        for i in 0..bgdt_sectors {
            let mut sec = [0u8; SECTOR_SIZE];
            disk.read_sector(bgdt_start_sector + i as u32, &mut sec)?;
            let offset = i * SECTOR_SIZE;
            bgdt_buf[offset..offset + SECTOR_SIZE].copy_from_slice(&sec);
        }

        let mut group_descs = Vec::new();
        for i in 0..num_block_groups as usize {
            if i * 32 + 32 <= bgdt_buf.len() {
                let desc = unsafe {
                    core::ptr::read(bgdt_buf.as_ptr().add(i * 32) as *const Ext2GroupDesc)
                };
                group_descs.push(desc);
            }
        }

        crate::info!("ext2", "read {} block group descriptors", group_descs.len());

        Ok(Self {
            block_size,
            inodes_per_group: sb.inodes_per_group,
            blocks_per_group,
            num_block_groups,
            inode_size,
            group_desc_table: group_descs,
            partition_offset,
        })
    }

    /// Read a block from disk.
    pub fn read_block(&self, disk: &IdeDisk, block: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        let sectors_per_block = self.block_size / SECTOR_SIZE as u32 as u32;
        let start_sector = self.partition_offset + block * sectors_per_block;

        for s in 0..sectors_per_block {
            let mut sector = [0u8; SECTOR_SIZE];
            disk.read_sector(start_sector + s, &mut sector)?;
            let offset = s as usize * SECTOR_SIZE;
            if offset + SECTOR_SIZE <= buf.len() {
                buf[offset..offset + SECTOR_SIZE].copy_from_slice(&sector);
            }
        }
        Ok(())
    }

    /// Read an inode by number.
    pub fn read_inode(&self, disk: &IdeDisk, inode_num: u32) -> Result<Ext2Inode, &'static str> {
        if inode_num == 0 { return Err("invalid inode 0"); }

        let inode_index = inode_num - 1; // ext2 inodes are 1-based
        let group = inode_index / self.inodes_per_group;
        let index_in_group = inode_index % self.inodes_per_group;

        if group as usize >= self.group_desc_table.len() {
            return Err("inode group out of range");
        }

        let desc = &self.group_desc_table[group as usize];
        let inode_table_sector = self.partition_offset + desc.inode_table * (self.block_size / SECTOR_SIZE as u32 as u32);
        let byte_offset = index_in_group * self.inode_size;
        let sector_offset = byte_offset / SECTOR_SIZE as u32;
        let byte_in_sector = ((byte_offset % SECTOR_SIZE as u32)) as usize;

        let mut buf = [0u8; SECTOR_SIZE];
        disk.read_sector(inode_table_sector + sector_offset as u32, &mut buf)?;

        let inode = unsafe { core::ptr::read(buf.as_ptr().add(byte_in_sector) as *const Ext2Inode) };
        Ok(inode)
    }

    /// Read the root inode (inode 2).
    pub fn root_inode(&self, disk: &IdeDisk) -> Result<Ext2Inode, &'static str> {
        self.read_inode(disk, 2)
    }

    /// Read a file's data by following direct block pointers.
    pub fn read_file_data(&self, disk: &IdeDisk, inode: &Ext2Inode, size: u32, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut offset = 0usize;
        let mut block_buf = alloc::vec![0u8; self.block_size as usize];

        // Direct blocks (0-11)
        for i in 0..12 {
            if offset >= buf.len() || offset >= size as usize { break; }
            let block = inode.block[i];
            if block == 0 { continue; }

            self.read_block(disk, block, &mut block_buf)?;
            let to_copy = (buf.len() - offset).min((size as usize - offset).min(self.block_size as usize));
            buf[offset..offset + to_copy].copy_from_slice(&block_buf[..to_copy]);
            offset += to_copy;
        }

        // Indirect block (12)
        if offset < buf.len() && offset < size as usize && inode.block[12] != 0 {
            let mut indirect_buf = alloc::vec![0u8; self.block_size as usize];
            self.read_block(disk, inode.block[12], &mut indirect_buf)?;
            let entries_per_block = self.block_size / 4;

            for i in 0..entries_per_block {
                if offset >= buf.len() || offset >= size as usize { break; }
                let block = u32::from_le_bytes([
                    indirect_buf[((i * 4) + 0) as usize],
                    indirect_buf[((i * 4) + 1) as usize],
                    indirect_buf[((i * 4) + 2) as usize],
                    indirect_buf[((i * 4) + 3) as usize],
                ]);
                if block == 0 { continue; }

                self.read_block(disk, block, &mut block_buf)?;
                let to_copy = (buf.len() - offset).min((size as usize - offset).min(self.block_size as usize));
                buf[offset..offset + to_copy].copy_from_slice(&block_buf[..to_copy]);
                offset += to_copy;
            }
        }

        Ok(offset)
    }

    /// List directory entries for an inode.
    pub fn readdir(&self, disk: &IdeDisk, inode: &Ext2Inode) -> Result<Vec<Ext2DirEntry>, &'static str> {
        let mut entries = Vec::new();
        let dir_size = inode.size;
        let mut dir_buf = alloc::vec![0u8; dir_size as usize];

        self.read_file_data(disk, inode, dir_size, &mut dir_buf)?;

        let mut pos = 0;
        while pos < dir_size as usize {
            if pos + 8 > dir_buf.len() { break; }

            let entry_inode = u32::from_le_bytes([
                dir_buf[pos], dir_buf[pos + 1], dir_buf[pos + 2], dir_buf[pos + 3]
            ]);
            let entry_len = u16::from_le_bytes([dir_buf[pos + 4], dir_buf[pos + 5]]) as usize;
            let name_len = dir_buf[pos + 6] as usize;
            let file_type = dir_buf[pos + 7];

            if entry_inode == 0 || entry_len == 0 {
                break;
            }

            if name_len > 0 && name_len <= 255 {
                let name_bytes = &dir_buf[pos + 8..pos + 8 + name_len];
                let name = alloc::string::String::from(core::str::from_utf8(name_bytes).unwrap_or("?"));
                entries.push(Ext2DirEntry {
                    inode: entry_inode,
                    name,
                    file_type,
                });
            }

            pos += entry_len;
        }

        Ok(entries)
    }

    /// Get block size.
    pub fn block_size(&self) -> u32 {
        self.block_size
    }
}
