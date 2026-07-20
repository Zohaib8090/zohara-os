// src/fs/zohfs.rs

//! ZohFS — simple on-disk filesystem for Zohara OS.
//!
//! Layout on a 1 MB disk (2048 sectors × 512 bytes):
//!   Sector 0:       Superblock (128 bytes)
//!   Sectors 1-7:    Block bitmap (3584 bytes → covers all blocks)
//!   Sectors 8-39:   Inode table (32 KB → 32 inodes × 1024 bytes each)
//!   Sectors 40+:    Data blocks
//!
//! Block size = 4096 bytes = 8 sectors.
//! Inode size = 128 bytes.

use crate::drivers::ide::{IdeDisk, SECTOR_SIZE};

pub const ZOHFS_MAGIC: u32 = 0x5A4F_4853;
pub const ZOHFS_VERSION: u32 = 1;
pub const BLOCK_SIZE: usize = 4096;
pub const SECTORS_PER_BLOCK: u32 = BLOCK_SIZE as u32 / SECTOR_SIZE as u32;
pub const MAX_INODES: u32 = 32;
pub const DIRECT_BLOCKS: usize = 10;
pub const ROOT_INODE: u32 = 1;

// ---- On-disk structures (packed to avoid alignment issues) ----

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Superblock {
    pub magic: u32,
    pub version: u32,
    pub block_size: u32,
    pub total_blocks: u32,
    pub inode_count: u32,
    pub free_blocks: u32,
    pub root_inode: u32,
    pub bitmap_start_sector: u32,
    pub inode_start_sector: u32,
    pub data_start_sector: u32,
    pub _reserved: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Inode {
    pub mode: u16,
    pub size: u32,
    pub block_count: u32,
    pub direct_blocks: [u32; DIRECT_BLOCKS],
    pub indirect_block: u32,
    pub _reserved: [u32; 2],
}

impl Inode {
    pub const fn empty() -> Self {
        Self { mode: 0, size: 0, block_count: 0, direct_blocks: [0; DIRECT_BLOCKS], indirect_block: 0, _reserved: [0; 2] }
    }
    pub fn is_dir(&self) -> bool { self.mode & 0o40000 != 0 }
    pub fn is_file(&self) -> bool { self.mode & 0o100000 != 0 }
    pub fn set_dir(&mut self) { self.mode = 0o40755; }
    pub fn set_file(&mut self) { self.mode = 0o100644; }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DirEntry {
    pub inode: u32,
    pub name: [u8; 56],
    pub entry_type: u16,
    pub _reserved: u16,
}

// ---- Global state ----

static mut FS_SUPER: Superblock = Superblock {
    magic: 0, version: 0, block_size: 0, total_blocks: 0,
    inode_count: 0, free_blocks: 0, root_inode: 0,
    bitmap_start_sector: 0, inode_start_sector: 0, data_start_sector: 0,
    _reserved: [0; 3],
};
static mut FS_DISK: IdeDisk = IdeDisk { base: 0x1F0, slave: false, total_sectors: 0 };
static mut FS_MOUNTED: bool = false;

const BITMAP_WORDS: usize = 64;
static mut BITMAP: [u64; BITMAP_WORDS] = [0; BITMAP_WORDS];
static mut INODES: [Inode; MAX_INODES as usize] = [Inode::empty(); MAX_INODES as usize];
static mut ROOT_DIR: [DirEntry; 64] = [DirEntry { inode: 0, name: [0; 56], entry_type: 0, _reserved: 0 }; 64];
static mut ROOT_DIR_COUNT: usize = 0;

// ---- Helper to safely read packed struct fields ----

fn sb_field<F: Copy>(f: impl FnOnce() -> F) -> F { f() }

// ---- Public API ----

/// Initialize ZohFS from a disk. Reads superblock, bitmap, inodes.
pub fn init(disk: &mut IdeDisk) -> Result<(), &'static str> {
    if !disk.is_present() {
        crate::warn!("zohfs", "no disk present, ZohFS not mounted");
        return Ok(());
    }

    let mut buf = [0u8; SECTOR_SIZE];
    crate::fs::bcache::cached_read(disk, 0, &mut buf)?;

    let sb = unsafe { core::ptr::read(buf.as_ptr() as *const Superblock) };

    let magic = sb_field(|| sb.magic);
    if magic != ZOHFS_MAGIC {
        crate::warn!("zohfs", "no ZohFS found (magic={:#x}), run 'mkfs'", magic);
        return Ok(());
    }

    unsafe {
        FS_SUPER = sb;
        FS_DISK = *disk;
        FS_MOUNTED = true;
    }

    // Read bitmap
    let mut bitmap_buf = [0u8; 3584];
    crate::fs::bcache::cached_read_sectors(disk, 1, 7, &mut bitmap_buf)?;
    unsafe {
        for i in 0..BITMAP_WORDS {
            BITMAP[i] = core::ptr::read(bitmap_buf.as_ptr().add(i * 8) as *const u64);
        }
    }

    // Read inodes
    let mut inode_buf = [0u8; 32 * 1024];
    crate::fs::bcache::cached_read_sectors(disk, 8, 64, &mut inode_buf)?;
    unsafe {
        for i in 0..MAX_INODES as usize {
            INODES[i] = core::ptr::read(inode_buf.as_ptr().add(i * 1024) as *const Inode);
        }
    }

    // Read root directory
    let root_bc = unsafe { INODES[ROOT_INODE as usize].block_count };
    if root_bc > 0 {
        let block = unsafe { INODES[ROOT_INODE as usize].direct_blocks[0] };
        if block > 0 {
            let mut dir_buf = [0u8; SECTOR_SIZE];
            let ds = unsafe { FS_SUPER.data_start_sector };
            let sector = ds + (block - 1) * SECTORS_PER_BLOCK;
            crate::fs::bcache::cached_read(disk, sector, &mut dir_buf)?;
            unsafe {
                let entry_count = (INODES[ROOT_INODE as usize].size as usize) / 64;
                ROOT_DIR_COUNT = entry_count.min(64);
                for i in 0..ROOT_DIR_COUNT {
                    ROOT_DIR[i] = core::ptr::read(dir_buf.as_ptr().add(i * 64) as *const DirEntry);
                }
            }
        }
    }

    let tb = sb_field(|| sb.total_blocks);
    let fb = sb_field(|| sb.free_blocks);
    crate::info!("zohfs", "mounted: {} blocks, {} free", tb, fb);
    Ok(())
}

/// Format the disk with ZohFS.
pub fn mkfs(disk: &mut IdeDisk) -> Result<(), &'static str> {
    if !disk.is_present() { return Err("no disk present"); }

    let total_sectors = disk.total_sectors();
    let data_start: u32 = 72; // sector 72
    let total_blocks = (total_sectors.saturating_sub(data_start)) / SECTORS_PER_BLOCK;

    let sb = Superblock {
        magic: ZOHFS_MAGIC,
        version: ZOHFS_VERSION,
        block_size: BLOCK_SIZE as u32,
        total_blocks,
        inode_count: MAX_INODES,
        free_blocks: total_blocks - (data_start / SECTORS_PER_BLOCK),
        root_inode: ROOT_INODE,
        bitmap_start_sector: 1,
        inode_start_sector: 8,
        data_start_sector: data_start,
        _reserved: [0; 3],
    };

    unsafe {
        for i in 0..BITMAP_WORDS { BITMAP[i] = 0; }
        let system_4k_blocks = 10usize; // superblock + bitmap + inodes + root block
        for i in 0..system_4k_blocks.min(BITMAP_WORDS * 64) {
            BITMAP[i / 64] |= 1u64 << (i % 64);
        }
        for i in 0..MAX_INODES as usize {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(INODES[i]), Inode::empty());
        }
        let mut root = Inode::empty();
        root.set_dir();
        root.block_count = 1;
        root.direct_blocks[0] = 1;
        root.size = 0;
        core::ptr::write_volatile(core::ptr::addr_of_mut!(INODES[ROOT_INODE as usize]), root);
        BITMAP[0] |= 1u64 << 1; // mark root data block used
        // Also reset in-memory root directory
        for i in 0..ROOT_DIR.len() {
            core::ptr::write_volatile(core::ptr::addr_of_mut!(ROOT_DIR[i]), DirEntry { inode: 0, name: [0; 56], entry_type: 0, _reserved: 0 });
        }
        ROOT_DIR_COUNT = 0;
    }

    // Write superblock
    let mut sb_buf = [0u8; SECTOR_SIZE];
    unsafe { core::ptr::copy_nonoverlapping(&sb as *const Superblock as *const u8, sb_buf.as_mut_ptr(), 128); }
    crate::fs::bcache::cached_write(disk, 0, &sb_buf)?;

    // Write bitmap
    let mut bitmap_buf = [0u8; 3584];
    unsafe {
        for i in 0..BITMAP_WORDS {
            core::ptr::copy_nonoverlapping(core::ptr::addr_of!(BITMAP[i]), bitmap_buf.as_mut_ptr().add(i * 8) as *mut u64, 1);
        }
    }
    crate::fs::bcache::cached_write_sectors(disk, 1, 7, &bitmap_buf)?;

    // Write inodes
    let mut inode_buf = [0u8; 32 * 1024];
    unsafe {
        for i in 0..MAX_INODES as usize {
            core::ptr::copy_nonoverlapping(core::ptr::addr_of!(INODES[i]), inode_buf.as_mut_ptr().add(i * 1024) as *mut Inode, 1);
        }
    }
    crate::fs::bcache::cached_write_sectors(disk, 8, 64, &inode_buf)?;

    // Clear root dir block
    let root_sector = data_start + (1 - 1) * SECTORS_PER_BLOCK;
    let empty = [0u8; SECTOR_SIZE];
    disk.write_sector(root_sector, &empty)?;

    let fb = sb_field(|| sb.free_blocks);
    crate::info!("zohfs", "formatted: {} blocks, {} free", total_blocks, fb);
    Ok(())
}

impl IdeDisk {
    fn write_sectors(&self, start: u32, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        for i in 0..count {
            let offset = (i as usize) * SECTOR_SIZE;
            if offset + SECTOR_SIZE > buf.len() { return Err("buffer too small"); }
            let mut sector = [0u8; SECTOR_SIZE];
            sector.copy_from_slice(&buf[offset..offset + SECTOR_SIZE]);
            self.write_sector(start + i, &sector)?;
        }
        Ok(())
    }
}

/// Check if ZohFS is currently mounted.
pub fn is_mounted() -> bool {
    unsafe { FS_MOUNTED }
}

/// Find a file/dir by name in root. Returns inode number.
pub fn find_file(name: &str) -> Option<u32> {
    let idx = find_in_root(name)?;
    unsafe { Some(ROOT_DIR[idx].inode) }
}

// ---- File operations ----

pub fn create_file(name: &str) -> Result<u32, &'static str> {
    if !unsafe { FS_MOUNTED } { return Err("filesystem not mounted"); }
    if find_in_root(name).is_some() { return Err("file exists"); }
    let inode_num = alloc_inode()?;
    unsafe {
        let mut node = core::ptr::read_volatile(core::ptr::addr_of!(INODES[inode_num as usize]));
        node.set_file();
        node.size = 0;
        core::ptr::write_volatile(core::ptr::addr_of_mut!(INODES[inode_num as usize]), node);
    }
    add_to_root(name, inode_num, false)
}

pub fn create_dir(name: &str) -> Result<u32, &'static str> {
    if !unsafe { FS_MOUNTED } { return Err("filesystem not mounted"); }
    if find_in_root(name).is_some() { return Err("directory exists"); }
    let inode_num = alloc_inode()?;
    unsafe {
        let mut node = core::ptr::read_volatile(core::ptr::addr_of!(INODES[inode_num as usize]));
        node.set_dir();
        node.size = 0;
        core::ptr::write_volatile(core::ptr::addr_of_mut!(INODES[inode_num as usize]), node);
    }
    add_to_root(name, inode_num, true)
}

pub fn read_file_data(inode_num: u32, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    if !unsafe { FS_MOUNTED } { return Err("filesystem not mounted"); }
    if inode_num == 0 || inode_num >= MAX_INODES { return Err("invalid inode"); }
    let disk = unsafe { &FS_DISK };
    let inode = unsafe { INODES[inode_num as usize] };
    if !inode.is_file() { return Err("not a file"); }
    let file_size = inode.size as usize;
    if offset >= file_size { return Ok(0); }
    let available = file_size - offset;
    let to_read = buf.len().min(available);
    let mut bytes_read = 0;
    let mut pos = offset;
    while bytes_read < to_read {
        let block_idx = pos / BLOCK_SIZE;
        if block_idx >= DIRECT_BLOCKS { break; }
        let block_num = inode.direct_blocks[block_idx];
        if block_num == 0 { break; }
        let block_offset = pos % BLOCK_SIZE;
        let ds = unsafe { FS_SUPER.data_start_sector };
        let sector = ds + (block_num - 1) * SECTORS_PER_BLOCK;
        let mut block_buf = [0u8; BLOCK_SIZE];
        for s in 0..SECTORS_PER_BLOCK {
            let mut sec = [0u8; SECTOR_SIZE];
            let target_sector = sector + s;
            crate::fs::bcache::cached_read(disk, target_sector, &mut sec)?;
            if s == 0 {
                crate::println!("[zohfs READ] inode={} blk={} sector={} data[0..4]={:02x?}", inode_num, block_num, target_sector, &sec[0..4]);
            }
            let start = s as usize * SECTOR_SIZE;
            block_buf[start..start + SECTOR_SIZE].copy_from_slice(&sec);
        }
        let n = (to_read - bytes_read).min(BLOCK_SIZE - block_offset);
        buf[bytes_read..bytes_read + n].copy_from_slice(&block_buf[block_offset..block_offset + n]);
        crate::println!("[zohfs] read blk={} sector={} data[0..8]={:?}", block_num, sector, &block_buf[0..8.min(BLOCK_SIZE)]);
        bytes_read += n;
        pos += n;
    }
    Ok(bytes_read)
}

pub fn write_file_data(inode_num: u32, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
    if !unsafe { FS_MOUNTED } { return Err("filesystem not mounted"); }
    if inode_num == 0 || inode_num >= MAX_INODES { return Err("invalid inode"); }
    let disk = unsafe { &FS_DISK };
    // Safety: copy inode out, mutate, write back via write_volatile to avoid UB
    // from &mut references to static mut.
    let mut inode = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(INODES[inode_num as usize])) };
    if !inode.is_file() { return Err("not a file"); }
    let mut pos = offset;
    let mut bytes_written = 0;
    while bytes_written < data.len() {
        let block_idx = pos / BLOCK_SIZE;
        if block_idx >= DIRECT_BLOCKS { break; }
        if inode.direct_blocks[block_idx] == 0 {
            let block_num = alloc_block()?;
            inode.direct_blocks[block_idx] = block_num;
            inode.block_count = block_idx as u32 + 1;
        }
        let block_num = inode.direct_blocks[block_idx];
        let block_offset = pos % BLOCK_SIZE;
        let ds = unsafe { FS_SUPER.data_start_sector };
        let sector = ds + (block_num - 1) * SECTORS_PER_BLOCK;
        let mut block_buf = [0u8; BLOCK_SIZE];
        for s in 0..SECTORS_PER_BLOCK {
            let mut sec = [0u8; SECTOR_SIZE];
            crate::fs::bcache::cached_read(disk, sector + s, &mut sec)?;
            let start = s as usize * SECTOR_SIZE;
            block_buf[start..start + SECTOR_SIZE].copy_from_slice(&sec);
        }
        let n = (data.len() - bytes_written).min(BLOCK_SIZE - block_offset);
        block_buf[block_offset..block_offset + n].copy_from_slice(&data[bytes_written..bytes_written + n]);
        crate::println!("[zohfs WRITE] inode={} blk={} sector={} data[0..4]={:02x?}", inode_num, block_num, sector, &block_buf[0..4.min(BLOCK_SIZE)]);
        for s in 0..SECTORS_PER_BLOCK {
            let mut sec = [0u8; SECTOR_SIZE];
            let start = s as usize * SECTOR_SIZE;
            sec.copy_from_slice(&block_buf[start..start + SECTOR_SIZE]);
            crate::fs::bcache::cached_write(disk, sector + s, &sec)?;
        }
        bytes_written += n;
        pos += n;
    }
    let new_size = offset + bytes_written;
    if new_size > inode.size as usize { inode.size = new_size as u32; }
    // Write the modified inode back to the static array
    unsafe {
        let dst = core::ptr::addr_of_mut!(INODES[inode_num as usize]);
        core::ptr::write_volatile(dst, inode);
    }
    Ok(bytes_written)
}

pub fn delete_file(name: &str) -> Result<(), &'static str> {
    if !unsafe { FS_MOUNTED } { return Err("filesystem not mounted"); }
    let entry_idx = find_in_root(name).ok_or("file not found")?;
    let inode_num;
    unsafe {
        inode_num = ROOT_DIR[entry_idx].inode;
        ROOT_DIR[entry_idx] = DirEntry { inode: 0, name: [0; 56], entry_type: 0, _reserved: 0 };
        ROOT_DIR_COUNT -= 1;
        INODES[ROOT_INODE as usize].size = (ROOT_DIR_COUNT * 64) as u32;
    }
    free_inode_blocks(inode_num)?;
    free_inode(inode_num)?;
    flush_root_dir()
}

pub fn readdir_root() -> alloc::vec::Vec<(alloc::string::String, u32, bool)> {
    let mut entries = alloc::vec::Vec::new();
    unsafe {
        for i in 0..ROOT_DIR_COUNT {
            let inode = ROOT_DIR[i].inode;
            if inode != 0 {
                let name_bytes = unsafe { &core::ptr::read_volatile(core::ptr::addr_of!(ROOT_DIR[i])).name };
                let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(56);
                let name = alloc::string::String::from(core::str::from_utf8(&name_bytes[..end]).unwrap_or("?"));
                let is_dir = ROOT_DIR[i].entry_type == 1;
                entries.push((name, inode, is_dir));
            }
        }
    }
    entries
}

// ---- Internal helpers ----

fn alloc_inode() -> Result<u32, &'static str> {
    unsafe {
        for i in 1..MAX_INODES as usize {
            if INODES[i].mode == 0 { return Ok(i as u32); }
        }
    }
    Err("no free inodes")
}

fn free_inode(inode: u32) -> Result<(), &'static str> {
    if inode == 0 || inode >= MAX_INODES { return Err("invalid inode"); }
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(INODES[inode as usize]), Inode::empty()); }
    Ok(())
}

fn alloc_block() -> Result<u32, &'static str> {
    unsafe {
        let total = FS_SUPER.total_blocks;
        for i in 0..total as usize {
            if BITMAP[i / 64] & (1u64 << (i % 64)) == 0 {
                BITMAP[i / 64] |= 1u64 << (i % 64);
                FS_SUPER.free_blocks = FS_SUPER.free_blocks.saturating_sub(1);
                return Ok(i as u32);
            }
        }
    }
    Err("no free blocks")
}

fn free_inode_blocks(inode_num: u32) -> Result<(), &'static str> {
    if inode_num == 0 || inode_num >= MAX_INODES { return Err("invalid inode"); }
    unsafe {
        let inode = INODES[inode_num as usize];
        for i in 0..inode.block_count.min(DIRECT_BLOCKS as u32) as usize {
            let block = inode.direct_blocks[i];
            if block > 0 {
                BITMAP[block as usize / 64] &= !(1u64 << (block as usize % 64));
                FS_SUPER.free_blocks += 1;
            }
        }
    }
    Ok(())
}

fn find_in_root(name: &str) -> Option<usize> {
    unsafe {
        for i in 0..ROOT_DIR_COUNT {
            let inode = ROOT_DIR[i].inode;
            if inode != 0 {
                let nb = unsafe { &core::ptr::read_volatile(core::ptr::addr_of!(ROOT_DIR[i])).name };
                let end = nb.iter().position(|&b| b == 0).unwrap_or(56);
                let entry_name = core::str::from_utf8(&nb[..end]).unwrap_or("");
                if entry_name == name { return Some(i); }
            }
        }
    }
    None
}

fn add_to_root(name: &str, inode_num: u32, is_dir: bool) -> Result<u32, &'static str> {
    unsafe {
        if ROOT_DIR_COUNT >= 64 { return Err("root directory full"); }
        let mut entry = core::ptr::read_volatile(core::ptr::addr_of!(ROOT_DIR[ROOT_DIR_COUNT]));
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(55);
        entry.name[..len].copy_from_slice(&name_bytes[..len]);
        entry.name[len] = 0;
        entry.inode = inode_num;
        entry.entry_type = if is_dir { 1 } else { 0 };
        entry._reserved = 0;
        core::ptr::write_volatile(core::ptr::addr_of_mut!(ROOT_DIR[ROOT_DIR_COUNT]), entry);
        ROOT_DIR_COUNT += 1;
        // Update root inode size via volatile
        let mut root_inode = core::ptr::read_volatile(core::ptr::addr_of!(INODES[ROOT_INODE as usize]));
        root_inode.size = (ROOT_DIR_COUNT * 64) as u32;
        core::ptr::write_volatile(core::ptr::addr_of_mut!(INODES[ROOT_INODE as usize]), root_inode);
    }
    flush_root_dir()?;
    Ok(inode_num)
}

fn flush_root_dir() -> Result<(), &'static str> {
    let disk = unsafe { &FS_DISK };
    let block = unsafe { INODES[ROOT_INODE as usize].direct_blocks[0] };
    if block == 0 { return Ok(()); }
    let ds = unsafe { FS_SUPER.data_start_sector };
    let sector = ds + (block - 1) * SECTORS_PER_BLOCK;
    let mut dir_buf = [0u8; SECTOR_SIZE];
    unsafe {
        let count = ROOT_DIR_COUNT.min(8);
        for i in 0..count {
            core::ptr::copy_nonoverlapping(core::ptr::addr_of!(ROOT_DIR[i]), dir_buf.as_mut_ptr().add(i * 64) as *mut DirEntry, 1);
        }
    }
    crate::fs::bcache::cached_write(disk, sector, &dir_buf)?;
    // Flush superblock
    let mut sb_buf = [0u8; SECTOR_SIZE];
    unsafe { core::ptr::copy_nonoverlapping(core::ptr::addr_of!(FS_SUPER) as *const u8, sb_buf.as_mut_ptr(), core::mem::size_of::<Superblock>()); }
    crate::fs::bcache::cached_write(disk, 0, &sb_buf)?;
    // Flush bitmap
    let mut bitmap_buf = [0u8; 3584];
    unsafe {
        for i in 0..BITMAP_WORDS {
            core::ptr::copy_nonoverlapping(core::ptr::addr_of!(BITMAP[i]), bitmap_buf.as_mut_ptr().add(i * 8) as *mut u64, 1);
        }
    }
    crate::fs::bcache::cached_write_sectors(disk, 1, 7, &bitmap_buf)?;
    // Flush inodes
    let mut inode_buf = [0u8; 32 * 1024];
    unsafe {
        for i in 0..MAX_INODES as usize {
            core::ptr::copy_nonoverlapping(core::ptr::addr_of!(INODES[i]), inode_buf.as_mut_ptr().add(i * 1024) as *mut Inode, 1);
        }
    }
    crate::fs::bcache::cached_write_sectors(disk, 8, 64, &inode_buf)?;
    Ok(())
}

pub fn dump_stats() {
    if !unsafe { FS_MOUNTED } {
            return;
    }
    unsafe {
        let tb = FS_SUPER.total_blocks;
        let fb = FS_SUPER.free_blocks;
        let ic = FS_SUPER.inode_count;
        let rc = ROOT_DIR_COUNT;
                        }
}
