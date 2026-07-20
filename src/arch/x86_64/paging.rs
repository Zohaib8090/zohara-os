// src/arch/x86_64/paging.rs

use crate::frame::{self, PhysFrame};

#[repr(C, align(4096))]
pub struct Table {
    pub entries: [u64; 512],
}

const P_PRESENT: u64 = 1 << 0;
const P_WRITE: u64 = 1 << 1;
const P_USER: u64 = 1 << 2;
const P_PCD: u64 = 1 << 4;     // Page Cache Disable (uncacheable) — bit 4, NOT bit 3!
const P_SIZE: u64 = 1 << 7;
const P_NX: u64 = 1 << 63;

fn table_entry(pa: u64) -> u64 { pa | P_PRESENT | P_WRITE }
fn table_entry_user(pa: u64) -> u64 { pa | P_PRESENT | P_WRITE | P_USER }
fn page_2mb(pa: u64) -> u64 { pa | P_PRESENT | P_WRITE | P_SIZE }
fn page_4k(pa: u64, writable: bool, user: bool, executable: bool) -> u64 {
    let mut f = P_PRESENT;
    if writable { f |= P_WRITE; }
    if user { f |= P_USER; }
    if !executable { f |= P_NX; }
    pa | f
}
/// 4K page with Uncacheable memory type (for MMIO like LAPIC).
fn page_4k_uc(pa: u64) -> u64 {
    pa | P_PRESENT | P_WRITE | P_PCD
}

unsafe fn zero_table(table: *mut Table) {
    for e in (*table).entries.iter_mut() { core::ptr::write_volatile(e, 0); }
}

fn alloc_table_or_halt() -> PhysFrame {
    match frame::allocate_frame() {
        Some(f) => f,
        None => { loop { unsafe { core::arch::asm!("cli"); core::arch::asm!("hlt"); } } }
    }
}

static mut KERNEL_PML4_PA: usize = 0;

/// Map a single 4K page at the given virtual address to the same physical
/// address (identity mapping). Creates intermediate page tables as needed.
///
/// # Safety
/// Caller must ensure `virt` is a valid page-aligned address.
pub unsafe fn map_page(virt: usize) {
    let pml4_idx = (virt >> 39) & 0x1FF;
    let pdpt_idx = (virt >> 30) & 0x1FF;
    let pd_idx   = (virt >> 21) & 0x1FF;
    let pt_idx   = (virt >> 12) & 0x1FF;
    let phys = virt; // identity mapping

    let pml4 = KERNEL_PML4_PA as *mut Table;

    // PML4 → PDPT
    let pdpt_pa = if (*pml4).entries[pml4_idx] & P_PRESENT != 0 {
        (*pml4).entries[pml4_idx] as usize & 0x000F_FFFF_FFFF_F000
    } else {
        let f = alloc_table_or_halt();
        let pa = f.start_address();
        zero_table(pa as *mut Table);
        (*pml4).entries[pml4_idx] = table_entry(pa as u64);
        pa
    };

    // PDPT → PD
    let pdpt = pdpt_pa as *mut Table;
    let pd_pa = if (*pdpt).entries[pdpt_idx] & P_PRESENT != 0 {
        (*pdpt).entries[pdpt_idx] as usize & 0x000F_FFFF_FFFF_F000
    } else {
        let f = alloc_table_or_halt();
        let pa = f.start_address();
        zero_table(pa as *mut Table);
        (*pdpt).entries[pdpt_idx] = table_entry(pa as u64);
        pa
    };

    // PD → PT
    let pd = pd_pa as *mut Table;
    let pt_pa = if (*pd).entries[pd_idx] & P_PRESENT != 0 {
        if (*pd).entries[pd_idx] & P_SIZE != 0 {
            // 2MB page — need to split into 4K pages
            let old_2mb_base = (*pd).entries[pd_idx] & 0x000F_FFFF_FFE0_0000;
            let f = alloc_table_or_halt();
            let new_pt_pa = f.start_address();
            zero_table(new_pt_pa as *mut Table);
            let pt = new_pt_pa as *mut Table;
            for i in 0..512u64 {
                (*pt).entries[i as usize] = page_4k(old_2mb_base + i * 0x1000, true, false, true);
            }
            (*pd).entries[pd_idx] = table_entry(new_pt_pa as u64);
            new_pt_pa
        } else {
            (*pd).entries[pd_idx] as usize & 0x000F_FFFF_FFFF_F000
        }
    } else {
        let f = alloc_table_or_halt();
        let pa = f.start_address();
        zero_table(pa as *mut Table);
        (*pd).entries[pd_idx] = table_entry(pa as u64);
        pa
    };

    // PT → 4K page
    let pt = pt_pa as *mut Table;
    (*pt).entries[pt_idx] = page_4k(phys as u64, true, false, true);

    // Flush TLB for this address
    core::arch::asm!("invlpg [{}]", in(reg) virt);
}

/// Identity-map a 4K page as Uncacheable (UC) — required for MMIO like LAPIC.
pub unsafe fn map_page_uc(virt: usize) {
    let pml4_idx = (virt >> 39) & 0x1FF;
    let pdpt_idx = (virt >> 30) & 0x1FF;
    let pd_idx   = (virt >> 21) & 0x1FF;
    let pt_idx   = (virt >> 12) & 0x1FF;
    let phys = virt;

    let pml4 = KERNEL_PML4_PA as *mut Table;

    let pdpt_pa = if (*pml4).entries[pml4_idx] & P_PRESENT != 0 {
        (*pml4).entries[pml4_idx] as usize & 0x000F_FFFF_FFFF_F000
    } else {
        let f = alloc_table_or_halt();
        let pa = f.start_address();
        zero_table(pa as *mut Table);
        (*pml4).entries[pml4_idx] = table_entry(pa as u64);
        pa
    };

    let pdpt = pdpt_pa as *mut Table;
    let pd_pa = if (*pdpt).entries[pdpt_idx] & P_PRESENT != 0 {
        (*pdpt).entries[pdpt_idx] as usize & 0x000F_FFFF_FFFF_F000
    } else {
        let f = alloc_table_or_halt();
        let pa = f.start_address();
        zero_table(pa as *mut Table);
        (*pdpt).entries[pdpt_idx] = table_entry(pa as u64);
        pa
    };

    let pd = pd_pa as *mut Table;
    let pt_pa = if (*pd).entries[pd_idx] & P_PRESENT != 0 {
        if (*pd).entries[pd_idx] & P_SIZE != 0 {
            let old_2mb_base = (*pd).entries[pd_idx] & 0x000F_FFFF_FFE0_0000;
            let f = alloc_table_or_halt();
            let new_pt_pa = f.start_address();
            zero_table(new_pt_pa as *mut Table);
            let pt = new_pt_pa as *mut Table;
            for i in 0..512u64 {
                (*pt).entries[i as usize] = page_4k(old_2mb_base + i * 0x1000, true, false, true);
            }
            (*pd).entries[pd_idx] = table_entry(new_pt_pa as u64);
            new_pt_pa
        } else {
            (*pd).entries[pd_idx] as usize & 0x000F_FFFF_FFFF_F000
        }
    } else {
        let f = alloc_table_or_halt();
        let pa = f.start_address();
        zero_table(pa as *mut Table);
        (*pd).entries[pd_idx] = table_entry(pa as u64);
        pa
    };

    let pt = pt_pa as *mut Table;
    (*pt).entries[pt_idx] = page_4k_uc(phys as u64);

    core::arch::asm!("invlpg [{}]", in(reg) virt);
}

pub fn init() {
    let pml4 = alloc_table_or_halt();
    let pdpt = alloc_table_or_halt();
    let pml4_pa = pml4.start_address();
    let pdpt_pa = pdpt.start_address();

    unsafe {
        zero_table(pml4_pa as *mut Table);
        (*(pml4_pa as *mut Table)).entries[0] = table_entry(pdpt_pa as u64);
        zero_table(pdpt_pa as *mut Table);
    }

    for pdpt_idx in 0..2u64 {
        let pd = alloc_table_or_halt();
        let pd_pa = pd.start_address();
        unsafe {
            (*(pdpt_pa as *mut Table)).entries[pdpt_idx as usize] = table_entry(pd_pa as u64);
            zero_table(pd_pa as *mut Table);
            for i in 0..512u64 {
                (*(pd_pa as *mut Table)).entries[i as usize] = page_2mb(pdpt_idx * 0x4000_0000 + i * 0x20_0000);
            }
        }
    }

    unsafe {
        KERNEL_PML4_PA = pml4_pa;
        core::arch::asm!("mov cr3, {}", in(reg) pml4_pa as u64);
    }
}

pub const USER_BASE_VA: usize = 0x0040_0000;

/// Lazily break a 2 MiB block in the user's page table into a 4 KiB PT
/// and return the PT physical address.  The PDPT→PD hierarchy must already
/// exist (set up by `create_user_page_tables`).
///
/// If the PD entry is already a PT pointer (not 2 MiB), just return it.
pub unsafe fn map_user_page(pml4_pa: usize, virt: usize) -> usize {
    let pml4_idx = (virt >> 39) & 0x1FF;
    let pdpt_idx = (virt >> 30) & 0x1FF;
    let pd_idx   = (virt >> 21) & 0x1FF;

    let pdpt_pa = (*(pml4_pa as *const Table)).entries[pml4_idx] as usize & 0x000F_FFFF_FFFF_F000;
    let pd_pa = (*(pdpt_pa as *const Table)).entries[pdpt_idx] as usize & 0x000F_FFFF_FFFF_F000;
    let pd_entry = (*(pd_pa as *const Table)).entries[pd_idx];

    if pd_entry & P_SIZE != 0 {
        // 2 MiB block — break into a 4 KiB PT, preserving existing mappings.
        let base = pd_entry & 0x000F_FFFF_FFE0_0000;
        let frame = alloc_table_or_halt();
        let pt_pa = frame.start_address();
        let pt = pt_pa as *mut Table;
        zero_table(pt);
        for i in 0..512u64 {
            (*pt).entries[i as usize] = page_4k(base + i * 0x1000, true, false, true);
        }
        (*(pd_pa as *mut Table)).entries[pd_idx] = table_entry_user(pt_pa as u64);
        pt_pa
    } else {
        pd_entry as usize & 0x000F_FFFF_FFFF_F000
    }
}

/// Build per-task page tables by lazily mapping only the pages the task
/// actually uses.  This replaces the old eager clone that copied the
/// entire kernel page table hierarchy (~1028 frames per task).
///
/// Cost per task: 1 PML4 + 1 PDPT + 2 PD + 1 PT + user data frames
/// ≈ 6 frames (down from 1030).
pub fn create_user_page_tables(
    elf_segments: &[(usize, usize, usize, usize, u32)],
) -> (usize, alloc::vec::Vec<(usize, usize, usize)>) {
    let new_pml4 = alloc_table_or_halt();
    let new_pml4_pa = new_pml4.start_address();
    unsafe { zero_table(new_pml4_pa as *mut Table); }

    // --- Step 1: Clone kernel identity-mapping hierarchy for PML4[0]. ---
    // Creates new PDPT + PD frames, populates them with copies of the
    // kernel's 2 MiB page entries.  This gives the user task access to
    // kernel code/data (supervisor-only) for syscalls, ISRs, etc.
    unsafe {
        let kernel_pml4 = KERNEL_PML4_PA as *const Table;
        // User pages all live under PML4 index 0 (addresses < 512 GiB).
        let pml4_idx = 0usize;
        let kernel_pdpt_pa = (*kernel_pml4).entries[pml4_idx] as usize & 0x000F_FFFF_FFFF_F000;

        let new_pdpt = alloc_table_or_halt();
        let new_pdpt_pa = new_pdpt.start_address();
        zero_table(new_pdpt_pa as *mut Table);

        let kernel_pdpt = kernel_pdpt_pa as *const Table;
        for pdpt_idx in 0..512usize {
            let kernel_pd_entry = (*kernel_pdpt).entries[pdpt_idx];
            if kernel_pd_entry & P_PRESENT == 0 { continue; }

            if kernel_pd_entry & P_SIZE != 0 {
                // 1 GiB page — copy as-is (supervisor, will be broken on demand).
                (*(new_pdpt_pa as *mut Table)).entries[pdpt_idx] = kernel_pd_entry;
            } else {
                // Table pointer → new PD, copy kernel's 2 MiB entries.
                let kernel_pd_pa = kernel_pd_entry as usize & 0x000F_FFFF_FFFF_F000;
                let kernel_pd = kernel_pd_pa as *const Table;

                let new_pd = alloc_table_or_halt();
                let new_pd_pa = new_pd.start_address();
                zero_table(new_pd_pa as *mut Table);

                for k in 0..512usize {
                    (*(new_pd_pa as *mut Table)).entries[k] = (*kernel_pd).entries[k];
                }

                (*(new_pdpt_pa as *mut Table)).entries[pdpt_idx] = table_entry_user(new_pd_pa as u64);
            }
        }

        (*(new_pml4_pa as *mut Table)).entries[pml4_idx] = table_entry_user(new_pdpt_pa as u64);
    }

    // --- Step 2: Map user pages (breaking 2 MiB blocks on demand). ---
    let mut user_phys_frames: alloc::vec::Vec<(usize, usize, usize)> = alloc::vec::Vec::new();

    for &(vaddr, _, _filesz, memsz, flags) in elf_segments {
        if memsz == 0 { continue; }
        let mut addr = vaddr & !0xFFF;
        let end = vaddr + memsz;
        while addr < end {
            let fresh = alloc_table_or_halt();
            let fresh_pa = fresh.start_address();
            unsafe { core::ptr::write_bytes(fresh_pa as *mut u8, 0, 4096); }
            unsafe {
                let pt_pa = map_user_page(new_pml4_pa, addr);
                let pt_idx = (addr >> 12) & 0x1FF;
                let writable = (flags & 2) != 0;
                let executable = (flags & 1) != 0;
                core::ptr::write_volatile(
                    &mut (*(pt_pa as *mut Table)).entries[pt_idx],
                    page_4k(fresh_pa as u64, writable, true, executable),
                );
            }
            user_phys_frames.push((addr, 4096, fresh_pa));
            addr += 0x1000;
        }
    }

    // Map a user stack page at USER_BASE_VA + 0x100000 (RW, user, non-exec).
    {
        let stack_vaddr = USER_BASE_VA + 0x100000;
        let fresh = alloc_table_or_halt();
        let fresh_pa = fresh.start_address();
        unsafe {
            core::ptr::write_bytes(fresh_pa as *mut u8, 0, 4096);
            let pt_pa = map_user_page(new_pml4_pa, stack_vaddr);
            let pt_idx = (stack_vaddr >> 12) & 0x1FF;
            core::ptr::write_volatile(
                &mut (*(pt_pa as *mut Table)).entries[pt_idx],
                page_4k(fresh_pa as u64, true, true, false),
            );
        }
        user_phys_frames.push((stack_vaddr, 4096, fresh_pa));
    }

    (new_pml4_pa, user_phys_frames)
}

pub fn is_user_mapped(pml4_pa: usize, virt: usize) -> bool {
    let pml4_idx = (virt >> 39) & 0x1FF;
    let pdpt_idx = (virt >> 30) & 0x1FF;
    let pd_idx   = (virt >> 21) & 0x1FF;
    let pt_idx   = (virt >> 12) & 0x1FF;

    unsafe {
        let e1 = (*(pml4_pa as *const Table)).entries[pml4_idx];
        if e1 & P_PRESENT == 0 { return false; }
        let pdpt = (e1 & 0x000F_FFFF_FFFF_F000) as *const Table;
        let e2 = (*pdpt).entries[pdpt_idx];
        if e2 & P_PRESENT == 0 { return false; }
        let pd = (e2 & 0x000F_FFFF_FFFF_F000) as *const Table;
        let e3 = (*pd).entries[pd_idx];
        if e3 & P_PRESENT == 0 { return false; }
        if e3 & P_SIZE != 0 {
            // 2 MiB block — user bit is on the PD entry itself.
            return (e3 & P_USER) != 0;
        }
        let pt = (e3 & 0x000F_FFFF_FFFF_F000) as *const Table;
        let e4 = (*pt).entries[pt_idx];
        if e4 & P_PRESENT == 0 { return false; }
        (e4 & P_USER) != 0
    }
}

/// Free a task's user page table hierarchy.
///
/// Walks the per-task PML4 and frees every page table frame that was
/// exclusively allocated for this task. Kernel-shared entries (matching the
/// kernel PML4) are skipped — only the user's cloned PDPTs, PDs, and PTs
/// are freed.
pub unsafe fn free_user_page_tables(pml4_pa: usize) {
    let kernel_pml4 = KERNEL_PML4_PA as *const Table;
    let user_pml4 = pml4_pa as *const Table;

    for pml4_idx in 0..512 {
        let user_entry = (*user_pml4).entries[pml4_idx];
        if user_entry & P_PRESENT == 0 { continue; }

        let kernel_entry = (*kernel_pml4).entries[pml4_idx];
        // Skip entries that match kernel (shared, not our allocation).
        if user_entry == kernel_entry { continue; }

        let pdpt_pa = user_entry as usize & 0x000F_FFFF_FFFF_F000;
        let pdpt = pdpt_pa as *const Table;

        for pdpt_idx in 0..512 {
            let pd_entry = (*pdpt).entries[pdpt_idx];
            if pd_entry & P_PRESENT == 0 { continue; }
            if pd_entry & P_SIZE != 0 { continue; }

            let pd_pa = pd_entry as usize & 0x000F_FFFF_FFFF_F000;
            let pd = pd_pa as *const Table;

            for pd_idx in 0..512 {
                let pt_entry = (*pd).entries[pd_idx];
                if pt_entry & P_PRESENT == 0 { continue; }
                if pt_entry & P_SIZE != 0 { continue; }

                let pt_pa = pt_entry as usize & 0x000F_FFFF_FFFF_F000;
                crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(pt_pa));
            }
            crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(pd_pa));
        }
        crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(pdpt_pa));
    }
    crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(pml4_pa));
}
