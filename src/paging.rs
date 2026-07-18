// src/paging.rs

pub fn init() {
    #[cfg(target_arch = "aarch64")]
    crate::arch::paging::init();

    #[cfg(target_arch = "x86_64")]
    crate::arch::paging::init();
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

pub const USER_BASE_VA: usize = 0x0040_0000;
