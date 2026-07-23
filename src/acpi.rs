// src/acpi.rs

//! ACPI table discovery: RSDP → XSDT → MADT.
//!
//! Populates `smp::CPUS[]` and `smp::CPU_COUNT` with discovered cores.
//! Also records the Local APIC and I/O APIC MMIO addresses from MADT.

use crate::smp::{self, CpuInfo, MAX_CORES};

/// Physical address of the Local APIC MMIO registers.
pub static mut MADT_LOCAL_APIC_ADDR: u32 = 0xFEE00000;

/// Physical address of the I/O APIC MMIO registers.
pub static mut MADT_IO_APIC_ADDR: u32 = 0;

/// Global System Interrupt base for the I/O APIC.
pub static mut MADT_IO_APIC_GSI_BASE: u32 = 0;

// --- RSDP ---

#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    _rsdt_address: u32,
    _length: u32,
    _xsdt_address: u64,
    _extended_checksum: u8,
    _reserved: [u8; 3],
}

// --- XSDT / RSDT header ---

#[repr(C, packed)]
struct AcpiHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

/// Verify a byte-range checksum sums to zero (mod 256).
fn verify_checksum(ptr: *const u8, len: usize) -> bool {
    let mut sum: u8 = 0;
    for i in 0..len {
        sum = sum.wrapping_add(unsafe { *ptr.add(i) });
    }
    sum == 0
}

/// Search for RSDP in the EBDA and physical memory ranges.
fn find_rsdp() -> Option<usize> {
    // 1. EBDA segment address at physical 0x40E (2 bytes)
    let ebda_seg = unsafe { *(0x40E as *const u16) } as usize;
    let ebda_addr = ebda_seg << 4; // convert segment to physical
    if ebda_addr > 0 {
        if let Some(p) = scan_for_rsdp(ebda_addr, 0x400) {
            crate::println!("[ACPI] RSDP found at EBDA 0x{:x}", p);
            return Some(p);
        }
    }

    // 2. Search 0xE0000..0xFFFFF (128 KB of ROM space)
    if let Some(p) = scan_for_rsdp(0xE0000, 0x20000) {
        crate::println!("[ACPI] RSDP found at 0x{:x}", p);
        return Some(p);
    }

    // 3. Search 0x80000..0xDFFFF (additional BIOS area)
    if let Some(p) = scan_for_rsdp(0x80000, 0x60000) {
        crate::println!("[ACPI] RSDP found at 0x{:x}", p);
        return Some(p);
    }

    // 4. On UEFI, RSDP is in regular RAM — scan the multiboot memory map.
    // Temporarily map each region through the page tables to scan it.
    crate::println!("[ACPI] Scanning memory map for RSDP (with page table mapping)...");
    unsafe {
        extern "C" {
            static mb_info: u64;
        }
        let info = mb_info as usize;
        if info != 0 {
            let flags = *((info + 0) as *const u32);
            if flags & (1 << 6) != 0 {
                let mmap_addr = *((info + 48) as *const u32) as usize;
                let mmap_len = *((info + 44) as *const u32) as usize;
                if mmap_addr > 0 && mmap_len > 0 && mmap_addr < 0x40000000 {
                    let mut offset = 0usize;
                    while offset + 24 <= mmap_len {
                        let entry = (mmap_addr + offset) as *const u8;
                        let entry_size = *((entry as *const u32)) as usize;
                        let base = *((entry.add(4) as *const u64)) as usize;
                        let len = *((entry.add(12) as *const u64)) as usize;
                        let typ = *((entry.add(20) as *const u32)) as u32;

                        // Scan usable (type 1) and ACPI-reclaimable (type 3) regions
                        if (typ == 1 || typ == 3) && len > 0x10000 && base > 0x100000 {
                            // Map region pages through page tables so we can read them
                            let page_base = base & !0x1FFFFF;
                            let page_count = (len + 0x1FFFFF) / 0x200000;
                            for i in 0..page_count {
                                let addr = page_base + i * 0x200000;
                                crate::paging::map_page(addr);
                            }

                            let scan_end = core::cmp::min(base + len, 0xFFFFFFFF);
                            if let Some(p) = scan_for_rsdp(base, scan_end - base) {
                                crate::println!("[ACPI] RSDP found at 0x{:x} (type {})", p, typ);
                                return Some(p);
                            }
                        }
                        offset += if entry_size > 0 { entry_size + 4 } else { 24 };
                    }
                }
            }
        }
    }

    crate::println!("[ACPI] RSDP not found");
    None
}

fn scan_for_rsdp(start: usize, len: usize) -> Option<usize> {
    // RSDP must be on a 16-byte boundary
    let mut addr = start & !0xF;
    let end = start + len;
    while addr + core::mem::size_of::<Rsdp>() <= end {
        let sig_ptr = addr as *const u8;
        let sig = unsafe { core::slice::from_raw_parts(sig_ptr, 8) };
        if sig == b"RSD PTR " {
            // Verify checksum (first 20 bytes)
            if verify_checksum(sig_ptr, 20) {
                return Some(addr);
            }
        }
        addr += 16;
    }
    None
}

/// Parse XSDT (64-bit addresses) to find a table by signature.
fn find_table_in_xsdt(xsdt_addr: usize, target_sig: &[u8; 4]) -> Option<usize> {
    let header = unsafe { &*(xsdt_addr as *const AcpiHeader) };
    let table_count = (header.length as usize - core::mem::size_of::<AcpiHeader>()) / 8;

    let entries_start = xsdt_addr + core::mem::size_of::<AcpiHeader>();
    for i in 0..table_count {
        let entry_ptr = (entries_start + i * 8) as *const u64;
        let table_addr = unsafe { *entry_ptr } as usize;
        if table_addr == 0 { continue; }

        let table_header = unsafe { &*(table_addr as *const AcpiHeader) };
        if &table_header.signature == target_sig {
            return Some(table_addr);
        }
    }
    None
}

/// Parse RSDT (32-bit addresses) to find a table by signature.
fn find_table_in_rsdt(rsdt_addr: usize, target_sig: &[u8; 4]) -> Option<usize> {
    let header = unsafe { &*(rsdt_addr as *const AcpiHeader) };
    let table_count = (header.length as usize - core::mem::size_of::<AcpiHeader>()) / 4;

    let entries_start = rsdt_addr + core::mem::size_of::<AcpiHeader>();
    for i in 0..table_count {
        let entry_ptr = (entries_start + i * 4) as *const u32;
        let table_addr = unsafe { *entry_ptr } as usize;
        if table_addr == 0 { continue; }

        let table_header = unsafe { &*(table_addr as *const AcpiHeader) };
        if &table_header.signature == target_sig {
            return Some(table_addr);
        }
    }
    None
}

/// Parse MADT (Multiple APIC Description Table) records.
///
/// Populates:
/// - `smp::CPUS[]` / `smp::CPU_COUNT` from Local APIC records (type 0)
/// - `MADT_LOCAL_APIC_ADDR` from MADT header (or overridden by type 5)
/// - `MADT_IO_APIC_ADDR` / `MADT_IO_APIC_GSI_BASE` from type 1
fn parse_madt(madt_addr: usize) {
    let header = unsafe { &*(madt_addr as *const AcpiHeader) };
    let madt_len = header.length as usize;

    // MADT-specific fields after the standard header
    let local_apic_addr = unsafe { *((madt_addr + 36) as *const u32) };
    unsafe { MADT_LOCAL_APIC_ADDR = local_apic_addr; }

    let flags = unsafe { *((madt_addr + 40) as *const u32) };
    crate::println!("[ACPI] MADT: local_apic={:#x} flags={:#x}", local_apic_addr, flags);

    // Walk variable-length records starting at offset 44
    let mut offset = madt_addr + 44; // absolute address of first record
    let mut core_idx = 1; // 0 = BSP, assigned later

    while offset + 2 <= madt_addr + madt_len {
        let rec_type = unsafe { *(offset as *const u8) };
        let rec_len = unsafe { *((offset + 1) as *const u8) } as usize;
        if rec_len < 2 { break; } // prevent infinite loop

        match rec_type {
            0 => {
                // Type 0: Processor Local APIC
                let apic_id = unsafe { *((offset + 3) as *const u8) } as u32;
                let _acpi_processor_id = unsafe { *((offset + 2) as *const u8) } as u32;
                let flags = unsafe { *((offset + 4) as *const u16) } as u32;

                if flags & 1 != 0 {
                    unsafe {
                        if smp::CPU_COUNT < MAX_CORES {
                            let is_bsp = smp::CPU_COUNT == 0;
                            let cpu = CpuInfo {
                                apic_id,
                                core_id: smp::CPU_COUNT,
                                is_bsp,
                            };
                            smp::CPUS[smp::CPU_COUNT] = Some(cpu);
                            smp::CPU_COUNT += 1;

                            crate::println!(
                                "[ACPI] CPU {}: apic_id={} {}",
                                smp::CPU_COUNT - 1,
                                apic_id,
                                if is_bsp { "(BSP)" } else { "" }
                            );
                        }
                    }
                }
            }
            1 => {
                // Type 1: I/O APIC
                let io_apic_id = unsafe { *((offset + 2) as *const u8) };
                let io_apic_addr = unsafe { *((offset + 4) as *const u32) };
                let gsi_base = unsafe { *((offset + 8) as *const u32) };
                unsafe {
                    MADT_IO_APIC_ADDR = io_apic_addr;
                    MADT_IO_APIC_GSI_BASE = gsi_base;
                }
                crate::println!(
                    "[ACPI] I/O APIC: id={} addr={:#x} gsi_base={}",
                    io_apic_id, io_apic_addr, gsi_base
                );
            }
            5 => {
                // Type 5: Local APIC Address Override (64-bit)
                let override_addr = unsafe { *((offset + 4) as *const u64) };
                if override_addr != 0 {
                    unsafe { MADT_LOCAL_APIC_ADDR = override_addr as u32; }
                    crate::println!("[ACPI] LAPIC address overridden to {:#x}", override_addr);
                }
            }
            _ => {} // Skip other record types
        }

        offset += rec_len;
    }

    // Dump all MADT-parsed APIC IDs for diagnostic verification
    crate::println!("[MADT] --- All Local APIC IDs from ACPI table ---");
    for i in 0..unsafe { smp::CPU_COUNT } {
        if let Some(cpu) = unsafe { smp::CPUS[i] } {
            crate::println!("[MADT] Local APIC ID = {}", cpu.apic_id);
        }
    }
}

/// Initialize ACPI: find RSDP → XSDT → MADT, discover cores.
pub fn init() {
    crate::println!("[ACPI] Searching for RSDP...");

    let rsdp_addr = match find_rsdp() {
        Some(addr) => addr,
        None => {
            crate::println!("[ACPI] RSDP not found — falling back to single core");
            // Set up a single BSP entry
            unsafe {
                smp::CPUS[0] = Some(CpuInfo {
                    apic_id: 0,
                    core_id: 0,
                    is_bsp: true,
                });
                smp::CPU_COUNT = 1;
            }
            return;
        }
    };

    crate::println!("[ACPI] RSDP found at {:#x}", rsdp_addr);

    // RSDP.revision >= 2 → XSDT (64-bit), else RSDT (32-bit)
    let rsdp = unsafe { &*(rsdp_addr as *const Rsdp) };

    let xsdt_addr = if rsdp.revision >= 2 {
        let addr = rsdp._xsdt_address as usize;
        crate::println!("[ACPI] XSDT at {:#x}", addr);
        addr
    } else {
        // RSDT: 32-bit pointers — for now, just use it as if it were XSDT
        // (the pointer width differs but layout is similar enough for scanning)
        let addr = rsdp._rsdt_address as usize;
        crate::println!("[ACPI] RSDT at {:#x} (legacy, 32-bit)", addr);
        addr
    };

    if xsdt_addr == 0 {
        crate::println!("[ACPI] XSDT/RSDT address is 0 — no ACPI tables");
        unsafe {
            smp::CPUS[0] = Some(CpuInfo { apic_id: 0, core_id: 0, is_bsp: true });
            smp::CPU_COUNT = 1;
        }
        return;
    }

    // Verify XSDT header checksum
    let xsdt_header = unsafe { &*(xsdt_addr as *const AcpiHeader) };
    let len = xsdt_header.length;
    if !verify_checksum(xsdt_addr as *const u8, len as usize) {
        crate::println!("[ACPI] XSDT checksum invalid");
        unsafe {
            smp::CPUS[0] = Some(CpuInfo { apic_id: 0, core_id: 0, is_bsp: true });
            smp::CPU_COUNT = 1;
        }
        return;
    }

    // Find MADT
    let madt_result = if rsdp.revision >= 2 {
        find_table_in_xsdt(xsdt_addr, b"APIC")
    } else {
        find_table_in_rsdt(xsdt_addr, b"APIC")
    };
    match madt_result {
        Some(madt_addr) => {
            crate::println!("[ACPI] MADT found at {:#x}", madt_addr);
            parse_madt(madt_addr);
        }
        None => {
            crate::println!("[ACPI] MADT not found — falling back to single core");
            unsafe {
                smp::CPUS[0] = Some(CpuInfo { apic_id: 0, core_id: 0, is_bsp: true });
                smp::CPU_COUNT = 1;
            }
        }
    }

    crate::println!("[ACPI] Total cores discovered: {}", unsafe { smp::CPU_COUNT });
}

// --- SMP helper functions ---

/// Get the number of CPUs discovered by MADT parsing.
pub fn smp_cpu_count() -> usize {
    unsafe { smp::CPU_COUNT }
}

/// Get a copy of the CpuInfo for a given core index.
pub fn smp_cpu(idx: usize) -> Option<CpuInfo> {
    unsafe { smp::CPUS[idx] }
}
