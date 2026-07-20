// src/drivers/ide.rs

//! IDE PIO block device driver.
//!
//! Provides raw sector read/write to a disk attached to the primary
//! IDE controller (I/O ports 0x1F0-0x1F7). Uses polling mode — no
//! IRQ handling needed for the initial implementation.

use core::arch::asm;

/// Primary IDE controller base port.
const PRIMARY_BASE: u16 = 0x1F0;
/// Primary IDE control/alternate status port.
const PRIMARY_CTRL: u16 = 0x3F6;

/// IDE I/O port offsets from base.
const DATA: u16 = 0;
const ERROR: u16 = 1;
const SECTOR_COUNT: u16 = 2;
const LBA_LOW: u16 = 3;
const LBA_MID: u16 = 4;
const LBA_HIGH: u16 = 5;
const DRIVE_HEAD: u16 = 6;
const STATUS: u16 = 7;
const COMMAND: u16 = 7;

/// Status register bit masks.
const SR_BSY: u8 = 0x80;
const SR_DRDY: u8 = 0x40;
const SR_DRQ: u8 = 0x08;
const SR_ERR: u8 = 0x01;

/// IDE commands.
const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_IDENTIFY: u8 = 0xEC;

/// Sector size in bytes.
pub const SECTOR_SIZE: usize = 512;

/// Read a byte from an I/O port.
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port);
    val
}

/// Write a byte to an I/O port.
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val);
}

/// Read a 16-bit word from an I/O port.
unsafe fn inw(port: u16) -> u16 {
    let val: u16;
    asm!("in ax, dx", out("ax") val, in("dx") port);
    val
}

/// Write a 16-bit word to an I/O port.
unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") val);
}

/// Wait for the drive to be ready (BSY clear, DRDY set).
unsafe fn wait_ready(base: u16) -> Result<(), &'static str> {
    let mut timeout = 100_000u32;
    loop {
        let status = inb(base + STATUS);
        if status & SR_BSY == 0 && status & SR_DRDY != 0 {
            return Ok(());
        }
        if status & SR_ERR != 0 {
            return Err("disk error during ready wait");
        }
        timeout = timeout.saturating_sub(1);
        if timeout == 0 {
            return Err("disk timeout during ready wait");
        }
    }
}

/// Wait for DRQ (data request ready).
unsafe fn wait_drq(base: u16) -> Result<(), &'static str> {
    let mut timeout = 100_000u32;
    loop {
        let status = inb(base + STATUS);
        if status & SR_DRQ != 0 {
            return Ok(());
        }
        if status & SR_ERR != 0 {
            return Err("disk error during DRQ wait");
        }
        timeout = timeout.saturating_sub(1);
        if timeout == 0 {
            return Err("disk timeout during DRQ wait");
        }
    }
}

/// Select the drive and LBA address.
unsafe fn select_drive_lba(base: u16, lba: u32, slave: bool) {
    let head = 0xE0 | ((lba >> 24) & 0x0F) as u8 | if slave { 0x10 } else { 0x00 };
    outb(base + DRIVE_HEAD, head);
    outb(base + LBA_LOW, (lba & 0xFF) as u8);
    outb(base + LBA_MID, ((lba >> 8) & 0xFF) as u8);
    outb(base + LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
}

/// IDE disk handle.
#[derive(Copy, Clone)]
pub struct IdeDisk {
    pub base: u16,
    pub slave: bool,
    pub total_sectors: u32,
}

impl IdeDisk {
    /// Create a handle for the primary master IDE disk.
    pub fn primary_master() -> Self {
        Self {
            base: PRIMARY_BASE,
            slave: false,
            total_sectors: 0,
        }
    }

    /// Create a handle for the primary slave IDE disk.
    pub fn primary_slave() -> Self {
        Self {
            base: PRIMARY_BASE,
            slave: true,
            total_sectors: 0,
        }
    }

    /// Initialize the disk: issue IDENTIFY DEVICE and read total sectors.
    pub fn init(&mut self) -> Result<(), &'static str> {
        unsafe {
            // Select drive
            select_drive_lba(self.base, 0, self.slave);
            outb(self.base + SECTOR_COUNT, 0);
            outb(self.base + LBA_LOW, 0);
            outb(self.base + LBA_MID, 0);
            outb(self.base + LBA_HIGH, 0);

            // Issue IDENTIFY DEVICE
            outb(self.base + COMMAND, CMD_IDENTIFY);

            // Wait for BSY to clear
            let mut timeout = 100_000u32;
            while inb(self.base + STATUS) & SR_BSY != 0 {
                timeout = timeout.saturating_sub(1);
                if timeout == 0 {
                    // Drive may not exist (not an error for QEMU without disk)
                    self.total_sectors = 0;
                    crate::info!("ide", "no disk detected on primary:{}", if self.slave { "slave" } else { "master" });
                    return Ok(());
                }
            }

            let status = inb(self.base + STATUS);
            if status == 0x00 || (status & SR_ERR != 0) {
                // status 0x00 = no drive present; ERR = drive error
                self.total_sectors = 0;
                crate::info!("ide", "no disk on primary:{}", if self.slave {"slave"} else {"master"});
                return Ok(());
            }

            // Wait for DRQ
            wait_drq(self.base)?;

            // Read 256 words of identify data
            let mut identify = [0u16; 256];
            for i in 0..256 {
                identify[i] = inw(self.base + DATA);
            }

            // Words 60-61: total addressable sectors (LBA28)
            self.total_sectors = ((identify[61] as u32) << 16) | (identify[60] as u32);

            let size_mb = (self.total_sectors as u64 * SECTOR_SIZE as u64) / (1024 * 1024);
            crate::info!("ide", "disk detected: {} sectors ({} MB)", self.total_sectors, size_mb);

            Ok(())
        }
    }

    /// Read a single 512-byte sector from the disk.
    pub fn read_sector(&self, lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= self.total_sectors {
            return Err("LBA out of range");
        }
        unsafe {
            select_drive_lba(self.base, lba, self.slave);
            outb(self.base + SECTOR_COUNT, 1);
            outb(self.base + COMMAND, CMD_READ_SECTORS);

            wait_ready(self.base)?;
            wait_drq(self.base)?;

            // Read 256 words (512 bytes)
            for i in 0..256 {
                let word = inw(self.base + DATA);
                buf[i * 2] = word as u8;
                buf[i * 2 + 1] = (word >> 8) as u8;
            }
            Ok(())
        }
    }

    /// Write a single 512-byte sector to the disk.
    pub fn write_sector(&self, lba: u32, buf: &[u8; SECTOR_SIZE]) -> Result<(), &'static str> {
        if lba >= self.total_sectors {
            return Err("LBA out of range");
        }
        unsafe {
            select_drive_lba(self.base, lba, self.slave);
            outb(self.base + SECTOR_COUNT, 1);
            outb(self.base + COMMAND, CMD_WRITE_SECTORS);

            wait_ready(self.base)?;
            wait_drq(self.base)?;

            // Write 256 words (512 bytes)
            for i in 0..256 {
                let word = (buf[i * 2] as u16) | ((buf[i * 2 + 1] as u16) << 8);
                outw(self.base + DATA, word);
            }

            // Flush: wait for write to complete
            wait_ready(self.base)?;
            Ok(())
        }
    }

    /// Read multiple consecutive sectors.
    pub fn read_sectors(&self, start_lba: u32, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        for i in 0..count {
            let offset = (i as usize) * SECTOR_SIZE;
            if offset + SECTOR_SIZE > buf.len() {
                return Err("buffer too small");
            }
            let mut sector = [0u8; SECTOR_SIZE];
            self.read_sector(start_lba + i, &mut sector)?;
            buf[offset..offset + SECTOR_SIZE].copy_from_slice(&sector);
        }
        Ok(())
    }

    /// Get total sectors on disk.
    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }

    /// Check if a disk is present.
    pub fn is_present(&self) -> bool {
        self.total_sectors > 0
    }
}
