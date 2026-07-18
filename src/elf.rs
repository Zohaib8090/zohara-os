// src/elf.rs

//! Minimal ELF64 parser for loading static, non-relocatable user programs.
//!
//! Handles only `PT_LOAD` segments from statically-linked ELF64 binaries.
//! No dynamic linking, no relocations, no section headers needed. The loader
//! reads the ELF header and program headers, then produces a list of segments
//! that the arch-specific page table builder can map into a task's address
//! space.

/// Maximum number of loadable segments we support per binary.
const MAX_SEGMENTS: usize = 8;

/// Error codes from ELF parsing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ElfError {
    /// The file doesn't start with the ELF magic bytes.
    BadMagic,
    /// Not a 64-bit ELF.
    NotElf64,
    /// Not a statically-linked executable.
    NotExecutable,
    /// Architecture mismatch with the current build target.
    BadMachine,
    /// A program header offset or size is out of bounds.
    BadPhdr,
    /// Too many PT_LOAD segments.
    TooManySegments,
}

/// A single loadable segment extracted from an ELF program header.
#[derive(Debug, Clone, Copy)]
pub struct ElfSegment {
    /// Virtual address where this segment is mapped.
    pub vaddr: usize,
    /// Physical address (same as vaddr for static binaries).
    pub paddr: usize,
    /// Pointer to the segment data within the ELF image.
    pub data_ptr: usize,
    /// Bytes of actual data in the ELF file (may be < memsz for .bss).
    pub filesz: usize,
    /// Total virtual memory size (filesz + zero-fill for .bss).
    pub memsz: usize,
    /// Permission flags (PF_R | PF_W | PF_X).
    pub flags: u32,
}

impl ElfSegment {
    pub fn readable(&self) -> bool { self.flags & 4 != 0 }
    pub fn writable(&self) -> bool { self.flags & 2 != 0 }
    pub fn executable(&self) -> bool { self.flags & 1 != 0 }
}

/// Parsed ELF image: entry point + up to MAX_SEGMENTS loadable segments.
pub struct ElfImage {
    pub entry_point: usize,
    pub segments: [ElfSegment; MAX_SEGMENTS],
    pub segment_count: usize,
}

/// Parse a raw ELF64 binary blob and extract loadable segments.
///
/// The returned `ElfSegment` pointers (`data_ptr`) reference into the original
/// `elf_data` slice — no copies are made here. The caller must copy the data
/// when mapping into page tables.
pub fn parse_elf(elf_data: &[u8]) -> Result<ElfImage, ElfError> {
    // Minimum size: ELF64 header (64 bytes)
    if elf_data.len() < 64 {
        return Err(ElfError::BadMagic);
    }

    // Check ELF magic: 0x7F 'E' 'L' 'F'
    if elf_data[0] != 0x7F || elf_data[1] != b'E' || elf_data[2] != b'L' || elf_data[3] != b'F' {
        return Err(ElfError::BadMagic);
    }

    // EI_CLASS: 64-bit
    if elf_data[4] != 2 {
        return Err(ElfError::NotElf64);
    }

    // e_type at offset 0x10 (2 bytes, little-endian)
    let e_type = u16::from_le_bytes([elf_data[0x10], elf_data[0x11]]);
    if e_type != 2 {
        return Err(ElfError::NotExecutable); // ET_EXEC = 2
    }

    // e_machine at offset 0x12 (2 bytes)
    let e_machine = u16::from_le_bytes([elf_data[0x12], elf_data[0x13]]);
    #[cfg(target_arch = "x86_64")]
    if e_machine != 0x3E {
        return Err(ElfError::BadMachine); // EM_X86_64
    }
    #[cfg(target_arch = "aarch64")]
    if e_machine != 0xB7 {
        return Err(ElfError::BadMachine); // EM_AARCH64
    }

    // e_entry at offset 0x18 (8 bytes)
    let entry_point = u64::from_le_bytes(elf_data[0x18..0x20].try_into().unwrap()) as usize;

    // e_phoff at offset 0x20 (8 bytes)
    let e_phoff = u64::from_le_bytes(elf_data[0x20..0x28].try_into().unwrap()) as usize;

    // e_phentsize at offset 0x36 (2 bytes)
    let e_phentsize = u16::from_le_bytes([elf_data[0x36], elf_data[0x37]]) as usize;

    // e_phnum at offset 0x38 (2 bytes)
    let e_phnum = u16::from_le_bytes([elf_data[0x38], elf_data[0x39]]) as usize;

    if e_phentsize < 56 {
        return Err(ElfError::BadPhdr); // ELF64 Phdr is exactly 56 bytes
    }

    let mut image = ElfImage {
        entry_point,
        segments: [ElfSegment {
            vaddr: 0, paddr: 0, data_ptr: 0,
            filesz: 0, memsz: 0, flags: 0,
        }; MAX_SEGMENTS],
        segment_count: 0,
    };

    for i in 0..e_phnum {
        let off = e_phoff + i * e_phentsize;
        if off + 56 > elf_data.len() {
            return Err(ElfError::BadPhdr);
        }

        let p_type = u32::from_le_bytes(elf_data[off..off + 4].try_into().unwrap());
        if p_type != 1 {
            continue; // PT_LOAD = 1; skip everything else
        }

        let p_flags = u32::from_le_bytes(elf_data[off + 4..off + 8].try_into().unwrap());
        let p_offset = u64::from_le_bytes(elf_data[off + 8..off + 16].try_into().unwrap()) as usize;
        let p_vaddr = u64::from_le_bytes(elf_data[off + 16..off + 24].try_into().unwrap()) as usize;
        let p_paddr = u64::from_le_bytes(elf_data[off + 24..off + 32].try_into().unwrap()) as usize;
        let p_filesz = u64::from_le_bytes(elf_data[off + 32..off + 40].try_into().unwrap()) as usize;
        let p_memsz = u64::from_le_bytes(elf_data[off + 40..off + 48].try_into().unwrap()) as usize;

        if image.segment_count >= MAX_SEGMENTS {
            return Err(ElfError::TooManySegments);
        }

        let data_ptr = elf_data.as_ptr() as usize + p_offset;

        image.segments[image.segment_count] = ElfSegment {
            vaddr: p_vaddr,
            paddr: p_paddr,
            data_ptr,
            filesz: p_filesz,
            memsz: p_memsz,
            flags: p_flags,
        };
        image.segment_count += 1;
    }

    Ok(image)
}
