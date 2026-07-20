// src/drivers/ahci.rs

//! AHCI (Advanced Host Controller Interface) block device driver.
//!
//! Implements basic AHCI read/write for SATA disks.
//! Uses memory-mapped HBA registers and command lists.
//!
//! For QEMU: AHCI controller is typically at PCI 00:04.0 with ABAR at 0xFEBF0000.

use core::arch::asm;

/// HBA register offsets (from ABAR base).
const HBA_CAP: usize = 0x00;
const HBA_GHC: usize = 0x04;
const HBA_IS: usize = 0x08;
const HBA_PI: usize = 0x0C;
const HBA_VS: usize = 0x10;

/// Global HBA Control bits.
const GHC_AE: u32 = 1 << 31;      // AHCI Enable
const GHC_IE: u32 = 1 << 1;       // Interrupt Enable
const GHC_HR: u32 = 1 << 0;       // HBA Reset

/// Per-port register offsets (base = ABAR + 0x100 + port * 0x80).
const PORT_CLB: usize = 0x00;     // Command List Base (low)
const PORT_CLBU: usize = 0x04;    // Command List Base (high)
const PORT_FB: usize = 0x08;      // FIS Base (low)
const PORT_FBU: usize = 0x0C;     // FIS Base (high)
const PORT_IS: usize = 0x10;      // Interrupt Status
const PORT_IE: usize = 0x14;      // Interrupt Enable
const PORT_CMD: usize = 0x18;     // Command and Status
const PORT_TFD: usize = 0x20;     // Task File Data
const PORT_SSTS: usize = 0x28;    // SATA Status
const PORT_CI: usize = 0x38;      // Command Issue

/// Port CMD bits.
const PORT_CMD_ST: u32 = 1 << 0;  // Start
const PORT_CMD_FRE: u32 = 1 << 4; // FIS Receive Enable
const PORT_CMD_FR: u32 = 1 << 14; // FIS Receive Reset
const PORT_CMD_CR: u32 = 1 << 15; // Command List Running

/// Port TFD bits.
const PORT_TFD_BSY: u32 = 1 << 7;
const PORT_TFD_DRQ: u32 = 1 << 3;

/// FIS types.
const FIS_TYPE_H2D: u8 = 0x27;
const FIS_TYPE_D2H: u8 = 0x34;
const FIS_TYPE_DMA_ACT: u8 = 0x39;

/// Command types.
const CMD_READ_DMA_EXT: u8 = 0x25;
const CMD_WRITE_DMA_EXT: u8 = 0x35;
const CMD_IDENTIFY_DMA: u8 = 0xEC;

/// Read a 32-bit value from HBA registers.
unsafe fn hba_read(base: usize, offset: usize) -> u32 {
    core::ptr::read_volatile((base + offset) as *const u32)
}

/// Write a 32-bit value to HBA registers.
unsafe fn hba_write(base: usize, offset: usize, val: u32) {
    core::ptr::write_volatile((base + offset) as *mut u32, val);
}

/// Read a 32-bit value from port registers.
unsafe fn port_read(base: usize, port: usize, offset: usize) -> u32 {
    hba_read(base, 0x100 + port * 0x80 + offset)
}

/// Write a 32-bit value to port registers.
unsafe fn port_write(base: usize, port: usize, offset: usize, val: u32) {
    hba_write(base, 0x100 + port * 0x80 + offset, val);
}

/// AHCI port handle.
pub struct AhciPort {
    base: usize,       // HBA base address
    port: usize,       // port number
    clb_phys: u64,     // Command List physical address
    fb_phys: u64,      // FIS Base physical address
}

/// Command List Entry (32 bytes).
#[repr(C, packed)]
struct CmdHeader {
    dw0: u32,          // CFL, PMP, C, B, R, W, P, CCI
    dba_low: u32,      // Command FIS Base (low)
    dba_high: u32,     // Command FIS Base (high)
    _r1: u32,
    _r2: u32,
    _r3: u32,
    prdtl: u32,        // Physical Region Descriptor Table Length
    _r4: u32,
}

/// PRDT Entry (16 bytes).
#[repr(C, packed)]
struct PrdtEntry {
    dba_low: u32,
    dba_high: u32,
    _r: u32,
    dbc: u32,          // Byte count + interrupt
}

/// FIS_H2D (Host to Device, 20 bytes).
#[repr(C, packed)]
struct FisH2D {
    fis_type: u8,
    pm_port: u8,
    command: u8,
    feature_lo: u8,
    lba_lo: u8,
    lba_mid: u8,
    lba_hi: u8,
    device: u8,
    lba_high: u8,
    feature_hi: u8,
    sector_count_lo: u8,
    sector_count_hi: u8,
    _r1: u8,
    control: u8,
    _r2: [u8; 4],
}

impl AhciPort {
    /// Initialize an AHCI port.
    pub fn init(base: usize, port: usize) -> Result<Self, &'static str> {
        // Check if port is implemented
        let pi = unsafe { hba_read(base, HBA_PI) };
        if pi & (1 << port) == 0 {
            return Err("port not implemented");
        }

        // Stop the port
        unsafe {
            let cmd = port_read(base, port, PORT_CMD);
            port_write(base, port, PORT_CMD, cmd & !PORT_CMD_ST);
            // Wait for CR to clear
            let mut timeout = 100000u32;
            while port_read(base, port, PORT_CMD) & PORT_CMD_CR != 0 {
                timeout -= 1;
                if timeout == 0 { return Err("timeout stopping port"); }
            }
        }

        // Allocate command list (1 KB aligned)
        let clb_phys = crate::frame::allocate_frame()
            .ok_or("no frames for CLB")?
            .start_address() as u64;
        // Zero the command list
        unsafe {
            core::ptr::write_bytes(clb_phys as *mut u8, 0, 1024);
        }

        // Allocate FIS base (256 bytes, but we use a frame)
        let fb_phys = crate::frame::allocate_frame()
            .ok_or("no frames for FB")?
            .start_address() as u64;
        unsafe {
            core::ptr::write_bytes(fb_phys as *mut u8, 0, 4096);
        }

        // Set command list and FIS base addresses
        unsafe {
            port_write(base, port, PORT_CLB, clb_phys as u32);
            port_write(base, port, PORT_CLBU, (clb_phys >> 32) as u32);
            port_write(base, port, PORT_FB, fb_phys as u32);
            port_write(base, port, PORT_FBU, (fb_phys >> 32) as u32);

            // Clear interrupt status
            port_write(base, port, PORT_IS, 0xFFFF);

            // Enable FIS receive
            let cmd = port_read(base, port, PORT_CMD);
            port_write(base, port, PORT_CMD, cmd | PORT_CMD_FRE);

            // Spin up device
            let cmd = port_read(base, port, PORT_CMD);
            port_write(base, port, PORT_CMD, cmd | PORT_CMD_ST);

            // Wait for DRDY
            let mut timeout = 100000u32;
            while port_read(base, port, PORT_TFD) & 0x40 == 0 {
                timeout -= 1;
                if timeout == 0 {
                    crate::warn!("ahci", "port {} DRDY timeout", port);
                    break;
                }
            }
        }

        crate::info!("ahci", "port {} initialized", port);

        Ok(Self { base, port, clb_phys, fb_phys })
    }

    /// Read a sector (512 bytes) from the disk.
    pub fn read_sector(&self, lba: u32, buf: &mut [u8; 512]) -> Result<(), &'static str> {
        unsafe {
            self.send_command(CMD_READ_DMA_EXT, lba, 1, buf.as_mut_ptr() as u64, false)?;
        }
        Ok(())
    }

    /// Write a sector (512 bytes) to the disk.
    pub fn write_sector(&self, lba: u32, buf: &[u8; 512]) -> Result<(), &'static str> {
        unsafe {
            self.send_command(CMD_WRITE_DMA_EXT, lba, 1, buf.as_ptr() as u64, true)?;
        }
        Ok(())
    }

    /// Send a command to the disk.
    unsafe fn send_command(&self, cmd: u8, lba: u32, count: u16, data_phys: u64, write: bool) -> Result<(), &'static str> {
        // Wait for port to be ready
        let mut timeout = 100000u32;
        while port_read(self.base, self.port, PORT_TFD) & (PORT_TFD_BSY | PORT_TFD_DRQ) != 0 {
            timeout -= 1;
            if timeout == 0 { return Err("port busy timeout"); }
        }

        // Command slot 0
        let slot = 0;
        let clb_offset = slot as usize * 32;

        // Build command FIS (H2D)
        let fis = FisH2D {
            fis_type: FIS_TYPE_H2D,
            pm_port: 0,
            command: cmd,
            feature_lo: 0,
            lba_lo: (lba & 0xFF) as u8,
            lba_mid: ((lba >> 8) & 0xFF) as u8,
            lba_hi: ((lba >> 16) & 0xFF) as u8,
            device: 0x40, // LBA mode
            lba_high: ((lba >> 24) & 0xFF) as u8,
            feature_hi: 0,
            sector_count_lo: count as u8,
            sector_count_hi: 0,
            _r1: 0,
            control: 0,
            _r2: [0; 4],
        };

        // Store FIS at CLB + 0x40 + slot * 0x100 (command FIS area)
        let fis_addr = self.clb_phys + 0x40 + slot as u64 * 0x100;
        core::ptr::copy_nonoverlapping(&fis as *const FisH2D, fis_addr as *mut FisH2D, 1);

        // Build PRDT entry for data transfer
        let prdt_offset = clb_offset + 0x80 + slot as usize * 0x100;
        let prdt = PrdtEntry {
            dba_low: data_phys as u32,
            dba_high: (data_phys >> 32) as u32,
            _r: 0,
            dbc: 512 - 1 | (1 << 31), // byte count + interrupt on completion
        };
        let prdt_addr = self.clb_phys + prdt_offset as u64;
        core::ptr::copy_nonoverlapping(&prdt as *const PrdtEntry, prdt_addr as *mut PrdtEntry, 1);

        // Set up command header
        let cmd_hdr = CmdHeader {
            dw0: (5 << 16) // 5 DW FIS length
                | (1 << 0) // PRDTL = 1 entry
                | if write { 1 << 6 } else { 0 }, // W bit
            dba_low: prdt_addr as u32,
            dba_high: (prdt_addr >> 32) as u32,
            _r1: 0,
            _r2: 0,
            _r3: 0,
            prdtl: 1,
            _r4: 0,
        };
        let hdr_addr = self.clb_phys + clb_offset as u64;
        core::ptr::copy_nonoverlapping(&cmd_hdr as *const CmdHeader, hdr_addr as *mut CmdHeader, 1);

        // Clear port interrupt status
        port_write(self.base, self.port, PORT_IS, 0xFFFF);

        // Issue command
        port_write(self.base, self.port, PORT_CI, 1 << slot);

        // Wait for completion
        let mut timeout = 1000000u32;
        while port_read(self.base, self.port, PORT_CI) & (1 << slot) != 0 {
            timeout -= 1;
            if timeout == 0 { return Err("command timeout"); }
        }

        // Check for errors
        let is = port_read(self.base, self.port, PORT_IS);
        if is & 0x1 != 0 { // Task File Error
            return Err("disk error");
        }

        Ok(())
    }
}

/// AHCI disk handle.
pub struct AhciDisk {
    port: AhciPort,
    total_sectors: u32,
}

impl AhciDisk {
    /// Initialize AHCI at a given base address.
    pub fn init(base: usize) -> Result<Self, &'static str> {
        unsafe {
            // Enable AHCI mode
            let ghc = hba_read(base, HBA_GHC);
            hba_write(base, HBA_GHC, ghc | GHC_AE);

            // Reset HBA
            let ghc = hba_read(base, HBA_GHC);
            hba_write(base, HBA_GHC, ghc | GHC_HR);
            let mut timeout = 100000u32;
            while hba_read(base, HBA_GHC) & GHC_HR != 0 {
                timeout -= 1;
                if timeout == 0 { return Err("HBA reset timeout"); }
            }

            // Re-enable AHCI mode after reset
            let ghc = hba_read(base, HBA_GHC);
            hba_write(base, HBA_GHC, ghc | GHC_AE);

            // Find first implemented port
            let pi = hba_read(base, HBA_PI);
            let mut port_num = 0;
            let mut found = false;
            for p in 0..32 {
                if pi & (1 << p) != 0 {
                    port_num = p;
                    found = true;
                    break;
                }
            }
            if !found { return Err("no ports implemented"); }

            let port = AhciPort::init(base, port_num)?;

            // Try to detect total sectors via identify
            let total_sectors = 0; // Will be detected on first access

            crate::info!("ahci", "AHCI initialized on port {}", port_num);

            Ok(Self { port, total_sectors })
        }
    }

    /// Read a sector.
    pub fn read_sector(&self, lba: u32, buf: &mut [u8; 512]) -> Result<(), &'static str> {
        self.port.read_sector(lba, buf)
    }

    /// Write a sector.
    pub fn write_sector(&self, lba: u32, buf: &[u8; 512]) -> Result<(), &'static str> {
        self.port.write_sector(lba, buf)
    }

    /// Check if disk is present.
    pub fn is_present(&self) -> bool {
        true // AHCI port was initialized
    }

    /// Get total sectors (estimated).
    pub fn total_sectors(&self) -> u32 {
        self.total_sectors
    }
}
