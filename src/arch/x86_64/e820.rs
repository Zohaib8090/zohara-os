// src/arch/x86_64/e820.rs

//! E820 memory map parser via Multiboot 1 info structure.
//!
//! At boot, GRUB/E820 provides a memory map through the Multiboot info
//! structure (pointer saved in `mb_info` by boot.S). This module parses it
//! to discover total usable RAM and mark reserved regions.

/// Parse the Multiboot memory map and return total usable RAM in bytes.
/// Also marks reserved regions (type != 1) via `frame::mark_used`.
pub fn detect_memory() -> usize {
    extern "C" {
        static mb_magic: u32;
        static mb_info: u32;
    }

    unsafe {
        // Check Multiboot magic (0x2BADB002 = booted by Multiboot-compliant loader).
        if mb_magic != 0x2BADB002 {
            crate::println!("[e820] No Multiboot magic (got 0x{:X}), defaulting to 1 GiB", mb_magic);
            return 1 << 30;
        }

        let info = mb_info as usize;
        if info == 0 {
            crate::println!("[e820] Null info pointer, defaulting to 1 GiB");
            return 1 << 30;
        }

        // Multiboot info header:
        //   offset 0: flags (u32)
        //   offset 44: mmap_length (u32) — total size of memory map in bytes
        //   offset 48: mmap_addr (u32) — physical address of memory map
        let flags = *((info + 0) as *const u32);
        let has_mmap = (flags & (1 << 6)) != 0;

        if !has_mmap {
            crate::println!("[e820] Multiboot info has no memory map flag, defaulting to 1 GiB");
            return 1 << 30;
        }

        let mmap_addr = *((info + 48) as *const u32) as usize;
        let mmap_len = *((info + 44) as *const u32) as usize;

        if mmap_addr == 0 || mmap_len == 0 {
            crate::println!("[e820] Memory map address/length is zero, defaulting to 1 GiB");
            return 1 << 30;
        }

        let mut total_usable: usize = 0;
        let mut offset = 0usize;

        crate::println!("[e820] Parsing memory map at 0x{:X}, length {} bytes", mmap_addr, mmap_len);

        while offset + 24 <= mmap_len {
            let entry = (mmap_addr + offset) as *const u8;

            // Each entry: size(u32) | base(u64) | length(u64) | type(u32)
            let entry_size = *((entry as *const u32)) as usize;
            let base = *((entry.add(4) as *const u64)) as usize;
            let len = *((entry.add(12) as *const u64)) as usize;
            let typ = *((entry.add(20) as *const u32)) as u32;

            let end = base.saturating_add(len);

            match typ {
                1 => {
                    // Usable RAM
                    if end > total_usable {
                        total_usable = end;
                    }
                    crate::println!("[e820]   Usable: 0x{:08X} - 0x{:08X} ({} MiB)",
                        base, end, len / (1024 * 1024));
                }
                2 => {
                    // Reserved — mark as used so frames aren't allocated here
                    if len > 0 && base < total_usable {
                        crate::frame::mark_used(base, end);
                        crate::println!("[e820]   Reserved: 0x{:08X} - 0x{:08X}", base, end);
                    }
                }
                3 | 4 => {
                    // ACPI reclaimable / ACPI NVS — mark as used for safety
                    if len > 0 && base < total_usable {
                        crate::frame::mark_used(base, end);
                    }
                }
                _ => {}
            }

            // Advance past this entry. entry_size is the size of the entry
            // data minus 4 bytes (the size field itself is not counted).
            offset += if entry_size > 0 { entry_size + 4 } else { 24 };
        }

        if total_usable == 0 {
            crate::println!("[e820] No usable RAM found, defaulting to 1 GiB");
            return 1 << 30;
        }

        crate::println!("[e820] Detected {} MiB total usable RAM", total_usable / (1024 * 1024));
        total_usable
    }
}
