// src/elf_builder.rs

//! Build a minimal ELF64 executable header around a flat code blob.
//!
//! Produces an in-memory ELF64 ET_EXEC binary with a single PT_LOAD segment
//! at `load_addr` so the ELF parser can process it like a real binary. No
//! section headers, no relocations — just an ELF header, one program header,
//! and the code bytes.

/// Build a minimal ELF64 byte vector: ELF header (64 bytes) + one program
/// header (56 bytes) + code bytes, suitable for passing to `elf::parse_elf`.
///
/// The segment is mapped as Read+Write+Execute at `load_addr`.
pub fn build_flat_elf(code: &[u8], load_addr: usize) -> alloc::vec::Vec<u8> {
    let code_len = code.len();
    let ph_offset: usize = 64;           // program header right after ELF header
    let code_offset: usize = 64 + 56;    // code right after program header
    let total = code_offset + code_len;

    let mut buf = alloc::vec![0u8; total];

    // --- ELF64 header (64 bytes) ---

    // e_ident
    buf[0] = 0x7F; buf[1] = b'E'; buf[2] = b'L'; buf[3] = b'F';
    buf[4] = 2;     // EI_CLASS = ELFCLASS64
    buf[5] = 1;     // EI_DATA = ELFDATA2LSB
    buf[6] = 1;     // EI_VERSION = EV_CURRENT
    buf[7] = 0;     // EI_OSABI = ELFOSABI_NONE

    // e_type = ET_EXEC (2)
    buf[0x10] = 2; buf[0x11] = 0;

    // e_machine
    #[cfg(target_arch = "x86_64")] { buf[0x12] = 0x3E; buf[0x13] = 0x00; }
    #[cfg(target_arch = "aarch64")] { buf[0x12] = 0xB7; buf[0x13] = 0x00; }

    // e_version = 1
    buf[0x14] = 1;

    // e_entry = load_addr (8 bytes, little-endian)
    let entry = load_addr as u64;
    buf[0x18..0x20].copy_from_slice(&entry.to_le_bytes());

    // e_phoff = 64
    buf[0x20..0x28].copy_from_slice(&ph_offset.to_le_bytes());

    // e_ehsize = 64
    buf[0x34..0x36].copy_from_slice(&64u16.to_le_bytes());

    // e_phentsize = 56
    buf[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());

    // e_phnum = 1
    buf[0x38..0x3A].copy_from_slice(&1u16.to_le_bytes());

    // --- Program header (56 bytes at offset 64) ---

    let p = ph_offset;

    // p_type = PT_LOAD (1)
    buf[p..p+4].copy_from_slice(&1u32.to_le_bytes());

    // p_flags = PF_R | PF_W | PF_X = 7
    buf[p+4..p+8].copy_from_slice(&7u32.to_le_bytes());

    // p_offset = code_offset (where code lives in the file)
    buf[p+8..p+16].copy_from_slice(&code_offset.to_le_bytes());

    // p_vaddr = load_addr
    buf[p+16..p+24].copy_from_slice(&(load_addr as u64).to_le_bytes());

    // p_paddr = load_addr
    buf[p+24..p+32].copy_from_slice(&(load_addr as u64).to_le_bytes());

    // p_filesz = code_len
    buf[p+32..p+40].copy_from_slice(&(code_len as u64).to_le_bytes());

    // p_memsz = code_len (no BSS)
    buf[p+40..p+48].copy_from_slice(&(code_len as u64).to_le_bytes());

    // p_align = 0x1000
    buf[p+48..p+56].copy_from_slice(&0x1000u64.to_le_bytes());

    // --- Code bytes ---
    buf[code_offset..].copy_from_slice(code);

    buf
}
