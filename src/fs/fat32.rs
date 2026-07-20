// src/fs/fat32.rs

//! FAT32 filesystem driver.
//!
//! Supports reading FAT32 volumes. Basic write support for new files.
//! Compatible with Windows, Linux, and macOS formatted FAT32 drives.

use alloc::vec::Vec;
use crate::drivers::ide::{IdeDisk, SECTOR_SIZE};

/// FAT32 Boot Sector (BPB) - first 512 bytes.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Fat32Bpb {
    _jmp_boot: [u8; 3],
    _oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media_type: u8,
    pub fat_size_16: u16,
    pub sectors_per_track: u16,
    pub num_heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,
    pub fat_size_32: u32,
    pub ext_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fs_info_sector: u16,
    pub backup_boot_sector: u16,
    _reserved: [u8; 12],
    pub drive_number: u8,
    _reserved1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],
}

/// FAT32 Directory Entry (32 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Fat32DirEntry {
    pub name: [u8; 8],
    pub ext: [u8; 3],
    pub attributes: u8,
    _reserved: u8,
    pub create_time_tenth: u8,
    pub create_time: u16,
    pub create_date: u16,
    pub access_date: u16,
    pub cluster_high: u16,
    pub modify_time: u16,
    pub modify_date: u16,
    pub cluster_low: u16,
    pub file_size: u32,
}

/// FAT32 Long Directory Entry (32 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Fat32LongEntry {
    pub order: u8,
    pub name1: [u8; 10], // UTF-16LE chars 1-5
    pub attributes: u8,  // 0x0F for long entry
    _type: u8,
    pub checksum: u8,
    pub name2: [u8; 12], // UTF-16LE chars 6-11
    _zero: u16,
    pub name3: [u8; 4],  // UTF-16LE chars 12-13
}

/// FAT32 filesystem handle.
#[derive(Copy, Clone)]
pub struct Fat32Fs {
    pub bytes_per_sector: u32,
    pub sectors_per_cluster: u32,
    pub reserved_sectors: u32,
    pub num_fats: u32,
    pub fat_size: u32,
    pub root_cluster: u32,
    pub data_start: u32,
    pub total_clusters: u32,
}


/// Global FAT32 mount state.
static mut FAT32_FS: Option<Fat32Fs> = None;
static mut FAT32_DISK: IdeDisk = IdeDisk { base: 0x1F0, slave: false, total_sectors: 0 };

/// Check if FAT32 is mounted.
pub fn is_mounted() -> bool {
    unsafe { FAT32_FS.is_some() }
}

/// Store mounted FAT32 filesystem and disk handle.
pub fn set_mounted(fs: Fat32Fs, disk: IdeDisk) {
    unsafe {
        FAT32_FS = Some(fs);
        FAT32_DISK = disk;
    }
}

/// Get reference to mounted FAT32 filesystem.
pub fn fs() -> Option<Fat32Fs> {
    unsafe { FAT32_FS }
}

/// Get reference to mounted disk.
pub fn disk() -> IdeDisk {
    unsafe { FAT32_DISK }
}

impl Fat32Fs {
    /// Initialize FAT32 from a disk.
    pub fn init(disk: &IdeDisk) -> Result<Self, &'static str> {
        let mut buf = [0u8; SECTOR_SIZE];
        disk.read_sector(0, &mut buf)?;

        let bpb = unsafe { core::ptr::read(buf.as_ptr() as *const Fat32Bpb) };

        // Validate FAT32
        if bpb.fat_size_16 != 0 {
            return Err("not FAT32 (FAT12/16 detected)");
        }
        if bpb.fat_size_32 == 0 {
            return Err("invalid FAT32: fat_size_32 is 0");
        }

        let bytes_per_sector = bpb.bytes_per_sector as u32;
        let sectors_per_cluster = bpb.sectors_per_cluster as u32;
        let reserved_sectors = bpb.reserved_sectors as u32;
        let num_fats = bpb.num_fats as u32;
        let fat_size = bpb.fat_size_32;
        let root_cluster = bpb.root_cluster;

        // Data start = reserved_sectors + (num_fats * fat_size)
        let data_start = reserved_sectors + (num_fats * fat_size);

        let total_sectors = if bpb.total_sectors_32 != 0 {
            bpb.total_sectors_32
        } else {
            bpb.total_sectors_16 as u32
        };

        let total_clusters = (total_sectors - data_start) / sectors_per_cluster;

        let fs = Self {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            fat_size,
            root_cluster,
            data_start,
            total_clusters,
        };

        crate::info!("fat32", "FAT32 detected: {} clusters, cluster={} sectors, root={}",
            total_clusters, sectors_per_cluster, root_cluster);

        Ok(fs)
    }

    /// Read a cluster from disk.
    pub fn read_cluster(&self, disk: &IdeDisk, cluster: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        if cluster < 2 { return Err("invalid cluster"); }
        let cluster_size = self.sectors_per_sector() * SECTOR_SIZE as u32;
        if buf.len() < cluster_size as usize {
            return Err("buffer too small for cluster");
        }

        let first_sector = self.data_start + (cluster - 2) * self.sectors_per_cluster;
        for s in 0..self.sectors_per_cluster {
            let mut sector = [0u8; SECTOR_SIZE];
            disk.read_sector(first_sector + s as u32, &mut sector)?;
            let offset = s as usize * SECTOR_SIZE;
            buf[offset..offset + SECTOR_SIZE].copy_from_slice(&sector);
        }
        Ok(())
    }

    /// Get the next cluster in the FAT chain.
    pub fn next_cluster(&self, disk: &IdeDisk, cluster: u32) -> Result<u32, &'static str> {
        // FAT32 FAT entry is 4 bytes (only lower 28 bits used)
        let fat_offset = self.reserved_sectors * self.bytes_per_sector + cluster * 4;
        let fat_sector = fat_offset / self.bytes_per_sector;
        let byte_in_sector = (fat_offset % self.bytes_per_sector) as usize;

        let mut buf = [0u8; SECTOR_SIZE];
        disk.read_sector(fat_sector as u32, &mut buf)?;

        let entry = u32::from_le_bytes([
            buf[byte_in_sector],
            buf[byte_in_sector + 1],
            buf[byte_in_sector + 2],
            buf[byte_in_sector + 3],
        ]);

        // FAT32 cluster values: 0x0FFFFFF7 = bad, >= 0x0FFFFFF8 = end of chain
        if entry >= 0x0FFFFFF8 {
            Ok(0) // End of chain
        } else if entry == 0x0FFFFFF7 {
            Err("bad cluster")
        } else {
            Ok(entry & 0x0FFFFFFF)
        }
    }

    /// Read a file's data by following the cluster chain.
    pub fn read_file_data(&self, disk: &IdeDisk, start_cluster: u32, size: u32, buf: &mut [u8]) -> Result<usize, &'static str> {
        let cluster_size = self.sectors_per_sector() * SECTOR_SIZE as u32;
        let mut cluster = start_cluster;
        let mut offset = 0usize;
        let mut cluster_buf = alloc::vec![0u8; cluster_size as usize];

        while offset < buf.len() && offset < size as usize {
            self.read_cluster(disk, cluster, &mut cluster_buf)?;
            let remaining = (size as usize - offset).min(cluster_buf.len());
            let to_copy = remaining.min(buf.len() - offset);
            buf[offset..offset + to_copy].copy_from_slice(&cluster_buf[..to_copy]);
            offset += to_copy;

            // Get next cluster
            cluster = self.next_cluster(disk, cluster)?;
            if cluster == 0 {
                break; // End of chain
            }
        }

        Ok(offset)
    }

    /// List files in a directory (by cluster).
    pub fn readdir(&self, disk: &IdeDisk, dir_cluster: u32) -> Result<Vec<DirEntry32>, &'static str> {
        let cluster_size = self.sectors_per_sector() * SECTOR_SIZE as u32;
        let mut cluster_buf = alloc::vec![0u8; cluster_size as usize];
        let mut entries = Vec::new();
        let mut cluster = dir_cluster;
        let mut depth = 0u32;

        loop {
            if cluster < 2 || depth > 1024 { break; }
            self.read_cluster(disk, cluster, &mut cluster_buf)?;
            depth += 1;

            // Parse directory entries (32 bytes each)
            let num_entries = cluster_size as usize / 32;
            let mut long_name = alloc::string::String::new();

            for i in 0..num_entries {
                let entry = unsafe {
                    core::ptr::read(cluster_buf.as_ptr().add(i * 32) as *const Fat32DirEntry)
                };

                // End of directory
                if entry.name[0] == 0x00 {
                    break;
                }

                // Deleted entry
                if entry.name[0] == 0xE5 {
                    long_name.clear();
                    continue;
                }

                // Long filename entry
                if entry.attributes == 0x0F {
                    // Extract long filename chars (simplified - just get ASCII chars)
                    let long_entry = unsafe {
                        core::ptr::read(cluster_buf.as_ptr().add(i * 32) as *const Fat32LongEntry)
                    };
                    // UTF-16LE chars 1-5 from name1
                    for j in 0..5 {
                        let c = u16::from_le_bytes([long_entry.name1[j * 2], long_entry.name1[j * 2 + 1]]);
                        if c == 0xFFFF || c == 0 { break; }
                        if c < 0x80 {
                            long_name.push(c as u8 as char);
                        }
                    }
                    continue;
                }

                // Regular entry
                let name = if !long_name.is_empty() {
                    let n = long_name.clone();
                    long_name.clear();
                    n
                } else {
                    // Short filename: 8.3 format
                    let mut n = alloc::string::String::new();
                    for c in entry.name.iter() {
                        if *c == b' ' { break; }
                        n.push(*c as char);
                    }
                    if entry.ext[0] != b' ' {
                        n.push('.');
                        for c in entry.ext.iter() {
                            if *c == b' ' { break; }
                            n.push(*c as char);
                        }
                    }
                    n
                };

                let cluster = ((entry.cluster_high as u32) << 16) | (entry.cluster_low as u32);
                let is_dir = entry.attributes & 0x10 != 0;
                let is_hidden = entry.attributes & 0x02 != 0;
                let is_system = entry.attributes & 0x04 != 0;

                // Skip hidden/system entries
                if is_hidden || is_system { continue; }

                entries.push(DirEntry32 {
                    name,
                    cluster,
                    size: entry.file_size,
                    is_dir,
                });
            }

            // Follow cluster chain
            cluster = self.next_cluster(disk, cluster)?;
            if cluster == 0 || cluster < 2 {
                break;
            }
        }

        Ok(entries)
    }

    /// Get cluster size in bytes.
    fn sectors_per_sector(&self) -> u32 {
        self.sectors_per_cluster
    }

    /// Get root directory cluster.
    /// Get total clusters.
    pub fn total_clusters(&self) -> u32 {
        self.total_clusters
    }
    pub fn root_cluster(&self) -> u32 {
        self.root_cluster
    }

    /// Cluster size in bytes.
    pub fn cluster_size(&self) -> u32 {
        self.sectors_per_cluster * self.bytes_per_sector
    }

    /// Scan FAT for a free cluster (entry == 0). Returns cluster number.
    pub fn alloc_cluster(&self, disk: &IdeDisk) -> Result<u32, &'static str> {
        let fat_start = self.reserved_sectors;
        let fat_sectors = self.fat_size;
        let entries_per_sector = self.bytes_per_sector / 4;
        let mut sector_buf = [0u8; SECTOR_SIZE];

        for sec in 0..fat_sectors {
            let sector = fat_start + sec;
            disk.read_sector(sector, &mut sector_buf)?;
            for i in 0..entries_per_sector as usize {
                let offset = i * 4;
                let entry = u32::from_le_bytes([
                    sector_buf[offset], sector_buf[offset+1],
                    sector_buf[offset+2], sector_buf[offset+3],
                ]) & 0x0FFFFFFF;
                if entry == 0 {
                    let cluster = sec * entries_per_sector + i as u32;
                    if cluster >= 2 && cluster < self.total_clusters {
                        return Ok(cluster);
                    }
                }
            }
        }
        Err("no free clusters")
    }

    /// Write a FAT entry for a given cluster.
    pub fn set_fat_entry(&self, disk: &IdeDisk, cluster: u32, value: u32) -> Result<(), &'static str> {
        let fat_start = self.reserved_sectors;
        let fat_offset = cluster * 4;
        let sector = fat_start + (fat_offset / self.bytes_per_sector);
        let byte_in_sector = (fat_offset % self.bytes_per_sector) as usize;

        let mut buf = [0u8; SECTOR_SIZE];
        disk.read_sector(sector, &mut buf)?;

        let val = value & 0x0FFFFFFF;
        buf[byte_in_sector..byte_in_sector+4].copy_from_slice(&val.to_le_bytes());

        // Write to all FAT copies
        for f in 0..self.num_fats {
            disk.write_sector(sector + f * self.fat_size, &buf)?;
        }
        Ok(())
    }

    /// Mark a cluster as end-of-chain.
    pub fn mark_eoc(&self, disk: &IdeDisk, cluster: u32) -> Result<(), &'static str> {
        self.set_fat_entry(disk, cluster, 0x0FFFFFF8)
    }

    /// Free a single cluster (set FAT entry to 0).
    pub fn free_cluster(&self, disk: &IdeDisk, cluster: u32) -> Result<(), &'static str> {
        self.set_fat_entry(disk, cluster, 0)
    }

    /// Free an entire cluster chain starting from start_cluster.
    pub fn free_cluster_chain(&self, disk: &IdeDisk, start_cluster: u32) -> Result<(), &'static str> {
        let mut cluster = start_cluster;
        loop {
            if cluster < 2 || cluster >= self.total_clusters { break; }
            let next = self.next_cluster(disk, cluster)?;
            self.free_cluster(disk, cluster)?;
            if next == 0 || next >= 0x0FFFFFF8 { break; }
            cluster = next;
        }
        Ok(())
    }

    /// Get the last cluster in a chain (the one pointing to EOC).
    pub fn last_cluster(&self, disk: &IdeDisk, start_cluster: u32) -> Result<u32, &'static str> {
        let mut cluster = start_cluster;
        loop {
            let next = self.next_cluster(disk, cluster)?;
            if next == 0 { return Ok(cluster); }
            cluster = next;
        }
    }

    /// Write data to a cluster on disk.
    pub fn write_cluster(&self, disk: &IdeDisk, cluster: u32, data: &[u8]) -> Result<(), &'static str> {
        let first_sector = self.data_start + (cluster - 2) * self.sectors_per_cluster;
        let cluster_bytes = self.cluster_size() as usize;
        let to_write = data.len().min(cluster_bytes);

        for s in 0..self.sectors_per_cluster {
            let mut sector = [0u8; SECTOR_SIZE];
            let offset = s as usize * SECTOR_SIZE;
            if offset < to_write {
                let end = (offset + SECTOR_SIZE).min(to_write);
                sector[..end - offset].copy_from_slice(&data[offset..end]);
            }
            disk.write_sector(first_sector + s, &sector)?;
        }
        Ok(())
    }

    /// Write file data, extending the cluster chain as needed.
    /// Returns bytes written.
    pub fn write_file_data(&self, disk: &IdeDisk, start_cluster: u32, offset: usize, data: &[u8]) -> Result<usize, &'static str> {
        let cs = self.cluster_size() as usize;
        let mut cluster = start_cluster;
        let mut pos = offset;
        let mut written = 0;

        // Walk to the cluster containing the offset
        while pos >= cs {
            let next = self.next_cluster(disk, cluster)?;
            if next == 0 { return Err("offset beyond file"); }
            cluster = next;
            pos -= cs;
        }

        while written < data.len() {
            // If this is not the first write to this cluster, read-modify-write
            let mut cluster_buf = alloc::vec![0u8; cs];
            self.read_cluster(disk, cluster, &mut cluster_buf)?;

            let n = (data.len() - written).min(cs - pos);
            cluster_buf[pos..pos + n].copy_from_slice(&data[written..written + n]);
            self.write_cluster(disk, cluster, &cluster_buf)?;

            written += n;
            pos = 0;

            if written < data.len() {
                // Need more clusters
                let next = self.next_cluster(disk, cluster)?;
                if next == 0 {
                    // Allocate new cluster and chain it
                    let newCluster = self.alloc_cluster(disk)?;
                    self.set_fat_entry(disk, cluster, newCluster)?;
                    self.mark_eoc(disk, newCluster)?;
                    cluster = newCluster;
                } else {
                    cluster = next;
                }
            }
        }
        Ok(written)
    }

    /// Create a short 8.3 directory entry in a directory.
    /// Returns the cluster assigned to the new file.
    pub fn create_file_entry(&self, disk: &IdeDisk, dir_cluster: u32, name: &str, is_dir: bool) -> Result<u32, &'static str> {
        // Allocate a cluster for the new file
        let file_cluster = self.alloc_cluster(disk)?;
        self.mark_eoc(disk, file_cluster)?;

        // Build 8.3 name
        let (short_name, short_ext) = to_8_3(name);

        let cluster_size = self.cluster_size() as usize;
        let mut cluster_buf = alloc::vec![0u8; cluster_size];
        let mut cluster = dir_cluster;

        loop {
            self.read_cluster(disk, cluster, &mut cluster_buf)?;
            let num_entries = cluster_size / 32;

            for i in 0..num_entries {
                let off = i * 32;
                if cluster_buf[off] == 0x00 || cluster_buf[off] == 0xE5 {
                    // Found free slot — write entry
                    let entry = Fat32DirEntry {
                        name: short_name,
                        ext: short_ext,
                        attributes: if is_dir { 0x10 } else { 0x20 },
                        _reserved: 0,
                        create_time_tenth: 0,
                        create_time: 0,
                        create_date: 0,
                        access_date: 0,
                        cluster_high: (file_cluster >> 16) as u16,
                        modify_time: 0,
                        modify_date: 0,
                        cluster_low: (file_cluster & 0xFFFF) as u16,
                        file_size: 0,
                    };
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            &entry as *const Fat32DirEntry as *const u8,
                            cluster_buf[off..].as_mut_ptr(),
                            32,
                        );
                    }
                    // Write the modified cluster back
                    self.write_cluster(disk, cluster, &cluster_buf)?;
                    return Ok(file_cluster);
                }
            }

            let next = self.next_cluster(disk, cluster)?;
            if next == 0 {
                // Directory full — allocate new cluster and chain it
                let newCluster = self.alloc_cluster(disk)?;
                self.set_fat_entry(disk, cluster, newCluster)?;
                self.mark_eoc(disk, newCluster)?;
                // Zero out the new cluster (empty directory)
                let empty = alloc::vec![0u8; cluster_size];
                self.write_cluster(disk, newCluster, &empty)?;
                // Write entry at first slot of new cluster
                let entry = Fat32DirEntry {
                    name: short_name,
                    ext: short_ext,
                    attributes: if is_dir { 0x10 } else { 0x20 },
                    _reserved: 0,
                    create_time_tenth: 0,
                    create_time: 0,
                    create_date: 0,
                    access_date: 0,
                    cluster_high: (file_cluster >> 16) as u16,
                    modify_time: 0,
                    modify_date: 0,
                    cluster_low: (file_cluster & 0xFFFF) as u16,
                    file_size: 0,
                };
                let mut new_buf = alloc::vec![0u8; cluster_size];
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &entry as *const Fat32DirEntry as *const u8,
                        new_buf.as_mut_ptr(),
                        32,
                    );
                }
                self.write_cluster(disk, newCluster, &new_buf)?;
                return Ok(file_cluster);
            }
            cluster = next;
        }
    }

    /// Delete a file entry from a directory (mark as deleted).
    pub fn delete_file_entry(&self, disk: &IdeDisk, dir_cluster: u32, name: &str) -> Result<(), &'static str> {
        let (short_name, short_ext) = to_8_3(name);
        let cluster_size = self.cluster_size() as usize;
        let mut cluster_buf = alloc::vec![0u8; cluster_size];
        let mut cluster = dir_cluster;

        loop {
            self.read_cluster(disk, cluster, &mut cluster_buf)?;
            let num_entries = cluster_size / 32;

            for i in 0..num_entries {
                let off = i * 32;
                if cluster_buf[off] == 0x00 { return Err("file not found"); }
                if cluster_buf[off] == 0xE5 { continue; }

                let entry_name = &cluster_buf[off..off+8];
                let entry_ext = &cluster_buf[off+8..off+11];

                if entry_name == &short_name && entry_ext == &short_ext {
                    // Free the cluster chain
                    let cl = ((cluster_buf[off+20] as u32) << 16) | (cluster_buf[off+26] as u32);
                    if cl >= 2 {
                        self.free_cluster_chain(disk, cl)?;
                    }
                    // Mark entry as deleted
                    cluster_buf[off] = 0xE5;
                    self.write_cluster(disk, cluster, &cluster_buf)?;
                    return Ok(());
                }
            }

            let next = self.next_cluster(disk, cluster)?;
            if next == 0 { return Err("file not found"); }
            cluster = next;
        }
    }

}

/// Convert a filename to 8.3 format.
fn to_8_3(name: &str) -> ([u8; 8], [u8; 3]) {
    let mut short_name = [0x20u8; 8];
    let mut short_ext = [0x20u8; 3];

    let parts: alloc::vec::Vec<&str> = name.split('.').collect();
    let base = parts[0].as_bytes();
    for i in 0..base.len().min(8) {
        short_name[i] = base[i].to_ascii_uppercase();
    }
    if parts.len() > 1 {
        let ext = parts[1].as_bytes();
        for i in 0..ext.len().min(3) {
            short_ext[i] = ext[i].to_ascii_uppercase();
        }
    }
    (short_name, short_ext)
}

/// A parsed directory entry.
#[derive(Debug, Clone)]
pub struct DirEntry32 {
    pub name: alloc::string::String,
    pub cluster: u32,
    pub size: u32,
    pub is_dir: bool,
}
