// src/fs/gpt.rs

//! GPT (GUID Partition Table) support.
//!
//! Reads and validates GPT partition tables from disk.
//! Allows Zohara to use disks with multiple partitions.

use alloc::vec::Vec;
use crate::drivers::ide::{IdeDisk, SECTOR_SIZE};

/// GPT Signature: "EFI PART"
const GPT_SIGNATURE: [u8; 8] = [0x45, 0x46, 0x49, 0x20, 0x50, 0x41, 0x52, 0x54];

/// GPT Header (LBA 1, 92 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GptHeader {
    pub signature: [u8; 8],
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub reserved: u32,
    pub my_lba: u64,
    pub alternate_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub partition_entry_size: u32,
    pub partition_entry_crc32: u32,
}

/// GPT Partition Entry (128 bytes).
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GptPartitionEntry {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    pub name: [u8; 72], // UTF-16LE, 36 characters
}

/// Well-known partition type GUIDs.
pub const PARTITION_TYPE_LINUX_FS: [u8; 16] = [
    0x0F, 0xC6, 0x3D, 0xA8, 0xF4, 0x8D, 0xE5, 0x11,
    0x9A, 0xA7, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
];

/// Information about a partition.
#[derive(Debug, Clone, Copy)]
pub struct Partition {
    pub index: usize,
    pub first_lba: u64,
    pub last_lba: u64,
    pub size_sectors: u64,
    pub first_byte: u64,
    pub size_bytes: u64,
}

/// Read and validate the GPT header from disk.
pub fn read_gpt_header(disk: &IdeDisk) -> Result<GptHeader, &'static str> {
    // GPT header is at LBA 1 (second sector)
    let mut buf = [0u8; SECTOR_SIZE];
    disk.read_sector(1, &mut buf)?;

    let header = unsafe { core::ptr::read(buf.as_ptr() as *const GptHeader) };

    // Validate signature
    if header.signature != GPT_SIGNATURE {
        return Err("no GPT partition table found");
    }

    // Validate header size
    if header.header_size < 92 {
        return Err("invalid GPT header size");
    }

    Ok(header)
}

/// List all valid partitions from the GPT.
pub fn list_partitions(disk: &IdeDisk) -> Result<Vec<Partition>, &'static str> {
    let header = read_gpt_header(disk)?;

    let mut partitions = Vec::new();
    let entries_per_sector = SECTOR_SIZE / header.partition_entry_size as usize;

    // Read partition entries (they span multiple sectors)
    let start_lba = header.partition_entry_lba;
    let num_entries = header.num_partition_entries as usize;

    for i in 0..num_entries {
        let sector_offset = i / entries_per_sector;
        let entry_offset = i % entries_per_sector;

        let mut buf = [0u8; SECTOR_SIZE];
        disk.read_sector(start_lba as u32 + sector_offset as u32, &mut buf)?;

        let entry = unsafe {
            core::ptr::read(buf.as_ptr().add(entry_offset * header.partition_entry_size as usize) as *const GptPartitionEntry)
        };

        // Check if partition is non-empty (first_lba != 0)
        if entry.first_lba != 0 && entry.last_lba >= entry.first_lba {
            let size_sectors = entry.last_lba - entry.first_lba + 1;
            partitions.push(Partition {
                index: i,
                first_lba: entry.first_lba,
                last_lba: entry.last_lba,
                size_sectors,
                first_byte: entry.first_lba * 512,
                size_bytes: size_sectors * 512,
            });
        }
    }

    Ok(partitions)
}

/// Get partition information by index.
pub fn get_partition(disk: &IdeDisk, index: usize) -> Result<Partition, &'static str> {
    let header = read_gpt_header(disk)?;

    let entries_per_sector = SECTOR_SIZE / header.partition_entry_size as usize;
    let sector_offset = index / entries_per_sector;
    let entry_offset = index % entries_per_sector;

    let mut buf = [0u8; SECTOR_SIZE];
    disk.read_sector(header.partition_entry_lba as u32 + sector_offset as u32, &mut buf)?;

    let entry = unsafe {
        core::ptr::read(buf.as_ptr().add(entry_offset * header.partition_entry_size as usize) as *const GptPartitionEntry)
    };

    if entry.first_lba == 0 || entry.last_lba < entry.first_lba {
        return Err("partition not found");
    }

    let size_sectors = entry.last_lba - entry.first_lba + 1;
    Ok(Partition {
        index,
        first_lba: entry.first_lba,
        last_lba: entry.last_lba,
        size_sectors,
        first_byte: entry.first_lba * 512,
        size_bytes: size_sectors * 512,
    })
}

/// Print GPT partition information.
pub fn dump_partitions(disk: &IdeDisk) {
    match read_gpt_header(disk) {
        Ok(header) => {
            let last_lba = header.last_usable_lba;
            let first_lba = header.first_usable_lba;
            let size_gb = (last_lba * 512) / (1024 * 1024 * 1024);
            crate::println!("=== GPT Partition Table ===");
            crate::println!("  Disk GUID present: yes");
            crate::println!("  Usable LBA range: {} - {}", first_lba, last_lba);
            crate::println!("  Disk size: ~{} GB", size_gb);

            match list_partitions(disk) {
                Ok(partitions) => {
                    if partitions.is_empty() {
                        crate::println!("  No partitions found");
                    } else {
                        crate::println!("  Partitions:");
                        for p in &partitions {
                            let size_mb = p.size_bytes / (1024 * 1024) as u64;
                            crate::println!("    [{}] LBA {}-{} ({} MB)", p.index, p.first_lba, p.last_lba, size_mb);
                        }
                    }
                }
                Err(e) => crate::println!("  Error listing partitions: {}", e),
            }
        }
        Err(e) => {
            crate::println!("=== No GPT partition table ===");
            crate::println!("  ({})", e);
        }
    }
}
