// src/arch/arm64/paging.rs

//! ARM64 MMU bring-up and per-task page table management.
//!
//! 4 KiB granule, 48-bit VA. On init we identity-map the first 2 GiB:
//!   0x0000_0000 – 0x3FFF_FFFF  Device-nGnRE (MMIO)
//!   0x4000_0000 – 0x7FFF_FFFF  Normal (RAM)
//!
//! Per-task page tables clone the kernel mapping and add user-accessible
//! pages (AP bits set for EL0) at USER_BASE_VA for ELF segments.

use crate::frame::{self, PhysFrame};

#[repr(C, align(4096))]
pub struct Table {
    pub entries: [u64; 512],
}

const DESC_BLOCK: u64 = 0b01;
const DESC_TABLE: u64 = 0b11;
const AF: u64 = 1 << 10;
const SH_INNER: u64 = 0b11 << 8;
const ATTR_NORMAL: u64 = 0 << 2;
const ATTR_DEVICE: u64 = 1 << 2;

/// AP bits for page descriptors:
/// AP[2:1] = 0b01 → Read/Write at EL0 and EL1
const AP_EL0_RW: u64 = 0b01 << 6;

/// XN (Execute-Never) bit for block descriptors
const XN: u64 = 1 << 54;
/// PXN (Privileged Execute-Never)
const PXN: u64 = 1 << 53;

fn normal_block(pa: u64) -> u64 {
    pa | ATTR_NORMAL | SH_INNER | AF | DESC_BLOCK
}

fn device_block(pa: u64) -> u64 {
    pa | ATTR_DEVICE | AF | DESC_BLOCK
}

fn table_desc(table_addr: u64) -> u64 {
    table_addr | DESC_TABLE
}

/// Build a 4 KiB page descriptor with given access permissions.
/// `user`: if true, set AP for EL0 access.
/// `executable`: if false, set XN (execute-never).
fn page_4k_desc(pa: u64, writable: bool, user: bool, executable: bool) -> u64 {
    let mut flags = pa | AF | SH_INNER;
    // Desc type = 0b11 for table entry at L3 (page descriptor)
    flags |= 0b11;
    if writable {
        // AP[2:1] = 0b00 for EL1-only RW, 0b01 for EL0+EL1 RW
        if user { flags |= AP_EL0_RW; }
        // else AP stays 0b00 (EL1 RW only)
    } else {
        // Read-only: AP[2:1] = 0b10 (EL1 RO) or 0b11 (EL0+EL1 RO)
        flags |= if user { 0b11 << 6 } else { 0b10 << 6 };
    }
    if !executable { flags |= XN | PXN; }
    flags
}

unsafe fn zero_table(table: *mut Table) {
    for entry in (*table).entries.iter_mut() {
        core::ptr::write_volatile(entry, 0);
    }
}

fn alloc_table_or_halt() -> PhysFrame {
    match frame::allocate_frame() {
        Some(f) => f,
        None => {
            crate::println!("[!!! FATAL: out of frames for page tables !!!]");
            crate::arch::halt();
        }
    }
}

// ---- Boot-time init ----

static mut KERNEL_TABLES: [PhysFrame; 4] = [PhysFrame { number: 0 }; 4];

pub fn init() {
    let l0 = alloc_table_or_halt();
    let l1 = alloc_table_or_halt();
    let l2_low = alloc_table_or_halt();
    let l2_ram = alloc_table_or_halt();

    unsafe { KERNEL_TABLES = [l0, l1, l2_low, l2_ram]; }

    let l0_pa = l0.start_address() as u64;
    let l1_pa = l1.start_address() as u64;
    let l2_low_pa = l2_low.start_address() as u64;
    let l2_ram_pa = l2_ram.start_address() as u64;

    unsafe {
        let l0_ptr = l0_pa as *mut Table;
        zero_table(l0_ptr);
        (*l0_ptr).entries[0] = table_desc(l1_pa);

        let l1_ptr = l1_pa as *mut Table;
        zero_table(l1_ptr);
        (*l1_ptr).entries[0] = table_desc(l2_low_pa);
        (*l1_ptr).entries[1] = table_desc(l2_ram_pa);

        let l2_low_ptr = l2_low_pa as *mut Table;
        zero_table(l2_low_ptr);
        for i in 0..512u64 {
            let pa = i * 0x20_0000;
            (*l2_low_ptr).entries[i as usize] = device_block(pa);
        }

        let l2_ram_ptr = l2_ram_pa as *mut Table;
        zero_table(l2_ram_ptr);
        for i in 0..512u64 {
            let pa = 0x4000_0000 + i * 0x20_0000;
            (*l2_ram_ptr).entries[i as usize] = normal_block(pa);
        }

        let mair: u64 = 0xFF | (0x04 << 8);
        core::arch::asm!("msr mair_el1, {}", in(reg) mair);

        let tcr: u64 = (0b010u64 << 32) | (16u64 << 16) | 16u64;
        core::arch::asm!("msr tcr_el1, {}", in(reg) tcr);

        core::arch::asm!("msr ttbr0_el1, {}", in(reg) l0_pa);
        core::arch::asm!("msr ttbr1_el1, {}", in(reg) l0_pa);

        core::arch::asm!("tlbi vmalle1");
        core::arch::asm!("dsb sy");
        core::arch::asm!("isb");

        let mut sctlr: u64;
        core::arch::asm!("mrs {}, sctlr_el1", out(reg) sctlr);
        sctlr |= 1;
        core::arch::asm!("msr sctlr_el1, {}", in(reg) sctlr);
        core::arch::asm!("isb");
    }
}

// ---- Per-task page table creation ----

/// Virtual base address where user ELF binaries are loaded.
pub const USER_BASE_VA: usize = 0x0040_0000;

/// Build per-task translation tables. Clones the kernel identity map and adds
/// user pages for ELF segments at their virtual addresses.
///
/// Returns the physical address of the new L0 (loadable into TTBR0_EL1).
pub fn create_user_page_tables(
    elf_segments: &[(usize, usize, usize, usize, u32)],
) -> usize {
    let new_l0 = alloc_table_or_halt();
    let new_l0_pa = new_l0.start_address();

    // Allocate L1 and two L2s for the full identity map.
    let new_l1 = alloc_table_or_halt();
    let new_l2_low = alloc_table_or_halt();
    let new_l2_ram = alloc_table_or_halt();

    unsafe {
        let l0_ptr = new_l0_pa as *mut Table;
        zero_table(l0_ptr);
        (*l0_ptr).entries[0] = table_desc(new_l1.start_address() as u64);

        let l1_ptr = new_l1.start_address() as *mut Table;
        zero_table(l1_ptr);
        (*l1_ptr).entries[0] = table_desc(new_l2_low.start_address() as u64);
        (*l1_ptr).entries[1] = table_desc(new_l2_ram.start_address() as u64);

        // Device region (first 1 GiB) — same as kernel, no user pages here.
        let l2_low_ptr = new_l2_low.start_address() as *mut Table;
        zero_table(l2_low_ptr);
        for i in 0..512u64 {
            let pa = i * 0x20_0000;
            (*l2_low_ptr).entries[i as usize] = device_block(pa);
        }

        // RAM region (second 1 GiB) — identity-map with 2 MiB blocks,
        // then break the block covering user segments into 4 KiB pages.
        let l2_ram_ptr = new_l2_ram.start_address() as *mut Table;
        zero_table(l2_ram_ptr);
        for i in 0..512u64 {
            let pa = 0x4000_0000 + i * 0x20_0000;
            (*l2_ram_ptr).entries[i as usize] = normal_block(pa);
        }

        // For each ELF segment, break the 2 MiB block into 4 KiB pages
        // and set the user-accessible ones.
        for &(vaddr, _data_ptr, _filesz, memsz, flags) in elf_segments {
            if memsz == 0 { continue; }
            let seg_start = vaddr;
            let seg_end = vaddr + memsz;
            let mut page_addr = seg_start & !0xFFF;
            while page_addr < seg_end {
                map_user_page_4kb(new_l0_pa, page_addr, flags);
                page_addr += 0x1000;
            }
        }
    }

    new_l0_pa
}

/// Map a single 4 KiB page at `virt_addr` with user permissions.
/// Walks L0→L1→L2, breaking 2 MiB blocks into L3 page tables as needed.
unsafe fn map_user_page_4kb(l0_pa: usize, virt_addr: usize, elf_flags: u32) {
    // Only lower 1 GiB (L1[0] → L2_low or L2_ram) is mapped.
    // User segments at 0x400000 (4 MiB) are in the second 1 GiB (L1[1] → L2_ram).
    let l0_ptr = l0_pa as *mut Table;
    let l1_pa = (*l0_ptr).entries[0] & 0x0000_FFFF_FFFF_F000;
    let l1_ptr = l1_pa as *mut Table;

    let l1_idx = if virt_addr < 0x4000_0000 { 0 } else { 1 };
    let l2_pa = (*l1_ptr).entries[l1_idx] & 0x0000_FFFF_FFFF_F000;
    let l2_ptr = l2_pa as *mut Table;

    let l2_idx = ((virt_addr - l1_idx * 0x4000_0000) >> 21) & 0x1FF;

    // If this is a 2 MiB block, break it into an L3 table.
    let old_entry = (*l2_ptr).entries[l2_idx];
    let l3_pa = if old_entry & 0x3 == DESC_BLOCK {
        // 2 MiB block — break into L3 table
        let block_pa = old_entry & 0x0000_FFFF_FFFE_0000;
        let l3_frame = alloc_table_or_halt();
        let l3 = l3_frame.start_address();
        let l3_ptr = l3 as *mut Table;
        zero_table(l3_ptr);
        for i in 0..512u64 {
            let pa = block_pa + i * 0x1000;
            // These are supervisor-only identity-mapped by default.
            (*l3_ptr).entries[i as usize] = page_4k_desc(pa, true, false, true);
        }
        (*l2_ptr).entries[l2_idx] = table_desc(l3);
        l3
    } else if old_entry & 0x3 == DESC_TABLE {
        old_entry & 0x0000_FFFF_FFFF_F000
    } else {
        // Empty — allocate L3 and fill with identity pages.
        let l3_frame = alloc_table_or_halt();
        let l3 = l3_frame.start_address();
        let l3_ptr = l3 as *mut Table;
        zero_table(l3_ptr);
        // Compute base PA for this 2 MiB range.
        let base_pa = l1_idx as u64 * 0x4000_0000 + l2_idx as u64 * 0x20_0000;
        for i in 0..512u64 {
            let pa = base_pa + i * 0x1000;
            (*l3_ptr).entries[i as usize] = page_4k_desc(pa, true, false, true);
        }
        (*l2_ptr).entries[l2_idx] = table_desc(l3);
        l3
    };

    // Set the specific page with user permissions.
    let l3_ptr = l3_pa as *mut Table;
    let l3_idx = (virt_addr >> 12) & 0x1FF;
    let writable = (elf_flags & 2) != 0;
    let executable = (elf_flags & 1) != 0;
    let pa = virt_addr as u64; // identity-mapped
    (*l3_ptr).entries[l3_idx] = page_4k_desc(pa, writable, true, executable);
}

// ---- Page table query ----

pub fn is_user_mapped(ttbr0: usize, virt_addr: usize) -> bool {
    unsafe {
        let l0 = ttbr0 as *const Table;
        let l1_entry = (*l0).entries[0]; // All user addresses in L1[0]
        if l1_entry & 0x3 != DESC_TABLE { return false; }
        let l1_pa = l1_entry & 0x0000_FFFF_FFFF_F000;
        let l1 = l1_pa as *const Table;

        let l1_idx = if virt_addr < 0x4000_0000 { 0 } else { 1 };
        if l1_idx >= 2 { return false; }
        let l2_entry = (*l1).entries[l1_idx];
        if l2_entry & 0x3 == 0 { return false; }
        if l2_entry & 0x3 == DESC_BLOCK {
            // 2 MiB block — not user-mapped (kernel only).
            return false;
        }
        let l2_pa = l2_entry & 0x0000_FFFF_FFFF_F000;
        let l2 = l2_pa as *const Table;
        let l2_idx = ((virt_addr - l1_idx * 0x4000_0000) >> 21) & 0x1FF;
        let l3_entry = (*l2).entries[l2_idx];
        if l3_entry & 0x3 == 0 { return false; }
        if l3_entry & 0x3 == DESC_BLOCK { return false; }
        let l3_pa = l3_entry & 0x0000_FFFF_FFFF_F000;
        let l3 = l3_pa as *const Table;
        let l3_idx = (virt_addr >> 12) & 0x1FF;
        let page_entry = (*l3).entries[l3_idx];
        if page_entry & 0x3 == 0 { return false; }
        // Check AP[2:1] bits: if bit 6 is set, EL0 has at least read access.
        (page_entry & (1 << 6)) != 0
    }
}

/// Free a task's user page tables (ARM64).
///
/// Walks L0→L1→L2→L3 and frees page table frames that were exclusively
/// allocated for this task. Kernel-shared entries are skipped.
pub unsafe fn free_user_page_tables(l0_pa: usize) {
    let l0 = l0_pa as *const Table;

    for l0_idx in 0..512 {
        let l1_entry = (*l0).entries[l0_idx];
        if l1_entry & DESC_TABLE == 0 { continue; }

        let l1_pa = l1_entry & 0x0000_FFFF_FFFF_F000;
        let l1 = l1_pa as *const Table;

        for l1_idx in 0..512 {
            let l2_entry = (*l1).entries[l1_idx];
            if l2_entry & DESC_TABLE == 0 { continue; }

            let l2_pa = l2_entry & 0x0000_FFFF_FFFF_F000;
            let l2 = l2_pa as *const Table;

            for l2_idx in 0..512 {
                let l3_entry = (*l2).entries[l2_idx];
                if l3_entry & DESC_TABLE == 0 { continue; }

                let l3_pa = l3_entry & 0x0000_FFFF_FFFF_F000;
                crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(l3_pa));
            }
            crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(l2_pa));
        }
        crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(l1_pa));
    }
    crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(l0_pa));
}
