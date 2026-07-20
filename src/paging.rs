// src/paging.rs

pub fn init() {
    #[cfg(target_arch = "aarch64")]
    crate::arch::paging::init();

    #[cfg(target_arch = "x86_64")]
    crate::arch::paging::init();
}

/// Identity-map a single 4K page at the given virtual address.
///
/// # Safety
/// Caller must ensure `virt` is page-aligned and the mapping is valid.
pub unsafe fn map_page(virt: usize) {
    #[cfg(target_arch = "x86_64")]
    crate::arch::paging::map_page(virt);
}

pub fn create_user_page_tables(
    elf_segments: &[(usize, usize, usize, usize, u32)],
) -> (usize, alloc::vec::Vec<(usize, usize, usize)>) {
    #[cfg(target_arch = "x86_64")]
    return crate::arch::paging::create_user_page_tables(elf_segments);

    #[cfg(target_arch = "aarch64")]
    return crate::arch::paging::create_user_page_tables(elf_segments);
}

/// Free a task's user page tables, reclaiming all allocated page table frames.
pub fn free_user_page_tables(pml4_pa: usize) {
    #[cfg(target_arch = "x86_64")]
    unsafe { crate::arch::paging::free_user_page_tables(pml4_pa); }

    #[cfg(target_arch = "aarch64")]
    unsafe { crate::arch::paging::free_user_page_tables(pml4_pa); }
}

/// Identity-map a 4K page as Uncacheable (UC) — required for MMIO like LAPIC.
pub unsafe fn map_page_uc(virt: usize) {
    #[cfg(target_arch = "x86_64")]
    crate::arch::paging::map_page_uc(virt);
}

pub const USER_BASE_VA: usize = 0x0040_0000;

/// Check if a virtual address is mapped user-accessible in the given page table.
pub fn is_user_mapped(pml4_pa: usize, virt: usize) -> bool {
    #[cfg(target_arch = "x86_64")]
    return crate::arch::paging::is_user_mapped(pml4_pa, virt);
    #[cfg(target_arch = "aarch64")]
    return false; // TODO: implement for aarch64
}
