// src/fs/bcache.rs

//! Buffer cache — in-memory cache for disk sectors.
//!
//! Reduces disk I/O by caching recently accessed sectors in RAM.
//! Uses a simple direct-mapped cache (sector_number % CACHE_SIZE → slot).
//!
//! For a 1 MB disk with 512-byte sectors, we have 2048 sectors.
//! A 64-slot cache covers 32 KB of cached data (6.25% of disk).
//! This is sufficient for the initial implementation.

use crate::drivers::ide::{IdeDisk, SECTOR_SIZE};

/// Number of cached sectors.
const CACHE_SIZE: usize = 64;

/// A single cache slot.
#[derive(Clone, Copy)]
struct CacheEntry {
    sector: u32,       // which sector is cached (u32::MAX = empty)
    valid: bool,       // is this entry valid?
    dirty: bool,       // does this entry need writeback?
}

impl CacheEntry {
    const fn empty() -> Self {
        Self { sector: u32::MAX, valid: false, dirty: false }
    }
}

/// The cache data: CACHE_SIZE sectors × 512 bytes = 32 KB.
static mut CACHE_DATA: [[u8; SECTOR_SIZE]; CACHE_SIZE] = [[0u8; SECTOR_SIZE]; CACHE_SIZE];

/// Cache metadata.
static mut CACHE_ENTRIES: [CacheEntry; CACHE_SIZE] = [CacheEntry::empty(); CACHE_SIZE];

/// Statistics.
static mut CACHE_HITS: usize = 0;
static mut CACHE_MISSES: usize = 0;
static mut CACHE_WRITES: usize = 0;

/// Get the cache slot index for a sector number.
fn cache_slot(sector: u32) -> usize {
    (sector as usize) % CACHE_SIZE
}

/// Look up a sector in the cache. Returns a pointer to the cached data if found.
fn cache_lookup(sector: u32) -> Option<*const [u8; SECTOR_SIZE]> {
    unsafe {
        let slot = cache_slot(sector);
        if CACHE_ENTRIES[slot].valid && CACHE_ENTRIES[slot].sector == sector {
            CACHE_HITS += 1;
            Some(unsafe { &core::ptr::read_volatile(core::ptr::addr_of!(CACHE_DATA[slot])) })
        } else {
            CACHE_MISSES += 1;
            None
        }
    }
}

/// Read a sector, using the cache if available.
pub fn cached_read(disk: &IdeDisk, sector: u32, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
    let slot = cache_slot(sector);
    let slot_sector = unsafe { CACHE_ENTRIES[slot].sector };
    let slot_valid = unsafe { CACHE_ENTRIES[slot].valid };
    let hit = slot_valid && slot_sector == sector;
    if hit {
        if let Some(cached) = cache_lookup(sector) {
            buf.copy_from_slice(unsafe { &*cached });
            return Ok(());
        }
    }
    crate::println!("[bcache] MISS sector={} slot={} slot_valid={} slot_sector={}", sector, slot, slot_valid, slot_sector);

    // Cache miss: read from disk
    disk.read_sector(sector, buf)?;

    // Store in cache
    let slot = cache_slot(sector);
    unsafe {
        // If the slot is dirty, write it back first
        if CACHE_ENTRIES[slot].dirty && CACHE_ENTRIES[slot].valid {
            let old_sector = CACHE_ENTRIES[slot].sector;
            disk.write_sector(old_sector, unsafe { &core::ptr::read_volatile(core::ptr::addr_of!(CACHE_DATA[slot])) })?;
            unsafe { CACHE_WRITES += 1; }
        }
        CACHE_DATA[slot].copy_from_slice(buf);
        CACHE_ENTRIES[slot] = CacheEntry { sector, valid: true, dirty: false };
    }

    Ok(())
}

/// Write a sector, updating the cache.
pub fn cached_write(disk: &IdeDisk, sector: u32, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
    // Write to disk
    disk.write_sector(sector, buf)?;
    unsafe { CACHE_WRITES += 1; }

    // Update cache
    let slot = cache_slot(sector);
    unsafe {
        CACHE_DATA[slot].copy_from_slice(buf);
        CACHE_ENTRIES[slot] = CacheEntry { sector, valid: true, dirty: false };
    }

    Ok(())
}

/// Flush all dirty cache entries to disk.
pub fn flush(disk: &IdeDisk) -> Result<(), &'static str> {
    unsafe {
        for i in 0..CACHE_SIZE {
            if CACHE_ENTRIES[i].dirty && CACHE_ENTRIES[i].valid {
                disk.write_sector(CACHE_ENTRIES[i].sector, unsafe { &core::ptr::read_volatile(core::ptr::addr_of!(CACHE_DATA[i])) })?;
                CACHE_ENTRIES[i].dirty = false;
                unsafe { CACHE_WRITES += 1; }
            }
        }
    }
    Ok(())
}

/// Invalidate the entire cache (e.g., after disk format).
pub fn invalidate() {
    unsafe {
        for i in 0..CACHE_SIZE {
            CACHE_ENTRIES[i] = CacheEntry::empty();
        }
    }
}

/// Print cache statistics.
pub fn dump_stats() {
    unsafe {
        let hits = CACHE_HITS;
        let misses = CACHE_MISSES;
        let total = hits + misses;
        let hit_rate = if total > 0 { hits * 100 / total } else { 0 };
        crate::println!("=== Buffer Cache ===");
        crate::println!("  Slots:   {}", CACHE_SIZE);
        crate::println!("  Hits:    {}", hits);
        crate::println!("  Misses:  {}", misses);
        crate::println!("  Hit rate: {}%", hit_rate);
        crate::println!("  Writes:  {}", CACHE_WRITES);
    }
}

/// Read multiple consecutive sectors using the cache.
pub fn cached_read_sectors(disk: &IdeDisk, start: u32, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
    for i in 0..count {
        let offset = (i as usize) * SECTOR_SIZE;
        if offset + SECTOR_SIZE > buf.len() { return Err("buffer too small"); }
        let mut sector = [0u8; SECTOR_SIZE];
        cached_read(disk, start + i, &mut sector)?;
        buf[offset..offset + SECTOR_SIZE].copy_from_slice(&sector);
    }
    Ok(())
}

/// Write multiple consecutive sectors using the cache.
pub fn cached_write_sectors(disk: &IdeDisk, start: u32, count: u32, buf: &[u8]) -> Result<(), &'static str> {
    for i in 0..count {
        let offset = (i as usize) * SECTOR_SIZE;
        if offset + SECTOR_SIZE > buf.len() { return Err("buffer too small"); }
        let mut sector = [0u8; SECTOR_SIZE];
        sector.copy_from_slice(&buf[offset..offset + SECTOR_SIZE]);
        cached_write(disk, start + i, &sector)?;
    }
    Ok(())
}
