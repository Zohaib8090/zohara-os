// src/runtime.rs

//! Userspace runtime — syscall wrappers, startup code, and basic functions.
//!
//! These are raw x86_64 machine code fragments that get embedded into
//! ELF test binaries. They provide the building blocks for userspace
//! programs to call kernel syscalls and use basic utilities.
//!
//! Syscall convention (x86_64):
//!   rax = syscall number
//!   rdi = arg0, rsi = arg1, rdx = arg2
//!   int 0x80
//!   rax = return value

// ---- Syscall numbers (must match syscall.rs) ----
pub const SYS_WRITE:     usize = 0;
pub const SYS_EXIT:      usize = 1;
pub const SYS_SLEEP:     usize = 2;
pub const SYS_READ:      usize = 3;
pub const SYS_GETPID:    usize = 4;
pub const SYS_YIELD:     usize = 5;
pub const SYS_GETTIME:   usize = 6;
pub const SYS_GETUPTIME: usize = 7;
pub const SYS_VERSION:   usize = 8;

// ---- Machine code helpers ----

/// `mov rax, imm32` — 7 bytes
pub fn mov_rax_imm(val: u32) -> [u8; 7] {
    [0x48, 0xC7, 0xC0, val as u8, (val >> 8) as u8, (val >> 16) as u8, (val >> 24) as u8]
}

/// `mov rdi, imm32` — 7 bytes
pub fn mov_rdi_imm(val: u32) -> [u8; 7] {
    [0x48, 0xC7, 0xC7, val as u8, (val >> 8) as u8, (val >> 16) as u8, (val >> 24) as u8]
}

/// `mov rsi, imm32` — 7 bytes
pub fn mov_rsi_imm(val: u32) -> [u8; 7] {
    [0x48, 0xC7, 0xC6, val as u8, (val >> 8) as u8, (val >> 16) as u8, (val >> 24) as u8]
}

/// `int 0x80` — 2 bytes
pub fn syscall() -> [u8; 2] {
    [0xCD, 0x80]
}

/// `ret` — 1 byte
pub fn ret() -> [u8; 1] {
    [0xC3]
}

/// `nop` — 1 byte
pub fn nop() -> [u8; 1] {
    [0x90]
}

/// `jmp rel32` (self-loop) — 2 bytes
pub fn jmp_self() -> [u8; 2] {
    [0xEB, 0xFE]
}

// ---- Higher-level instruction builders ----

/// Write syscall: Write(buf, len)
/// rax = SYS_WRITE, rdi = buf, rsi = len, int 0x80
/// Returns bytes written in rax.
pub fn emit_write_syscall(buf_addr: u32, buf_len: u32) -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    code.extend_from_slice(&mov_rax_imm(SYS_WRITE as u32));
    code.extend_from_slice(&mov_rdi_imm(buf_addr));
    code.extend_from_slice(&mov_rsi_imm(buf_len));
    code.extend_from_slice(&syscall());
    code
}

/// Exit syscall: Exit(code)
/// rax = SYS_EXIT, rdi = code, int 0x80
pub fn emit_exit_syscall(exit_code: u32) -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    code.extend_from_slice(&mov_rax_imm(SYS_EXIT as u32));
    code.extend_from_slice(&mov_rdi_imm(exit_code));
    code.extend_from_slice(&syscall());
    code
}

/// Sleep syscall: Sleep(ms)
/// rax = SYS_SLEEP, rdi = duration_ms, int 0x80
pub fn emit_sleep_syscall(ms: u32) -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    code.extend_from_slice(&mov_rax_imm(SYS_SLEEP as u32));
    code.extend_from_slice(&mov_rdi_imm(ms));
    code.extend_from_slice(&syscall());
    code
}

/// GetPid syscall: returns PID in rax
pub fn emit_getpid_syscall() -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    code.extend_from_slice(&mov_rax_imm(SYS_GETPID as u32));
    code.extend_from_slice(&syscall());
    code
}

/// Yield syscall: gives up timeslice
pub fn emit_yield_syscall() -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    code.extend_from_slice(&mov_rax_imm(SYS_YIELD as u32));
    code.extend_from_slice(&syscall());
    code
}

/// GetTime syscall: returns uptime_ms in rax
pub fn emit_gettime_syscall() -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    code.extend_from_slice(&mov_rax_imm(SYS_GETTIME as u32));
    code.extend_from_slice(&syscall());
    code
}

// ---- Utility functions ----

/// strlen: count bytes until null terminator
/// Input: rdi = pointer to string
/// Output: rax = length
pub fn emit_strlen() -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    // xor rax, rax (clear counter)
    code.extend_from_slice(&[0x48, 0x31, 0xC0]);
    // loop: cmp byte [rdi+rax], 0; je done; inc rax; jmp loop
    // .loop:
    code.extend_from_slice(&[0x80, 0x3C, 0x07, 0x00]); // cmp byte [rdi+rax], 0
    code.extend_from_slice(&[0x74, 0x03]);               // je +3 (to ret)
    code.extend_from_slice(&[0x48, 0xFF, 0xC0]);         // inc rax
    code.extend_from_slice(&[0xEB, 0xF9]);               // jmp -7 (back to cmp)
    // ret
    code.extend_from_slice(&ret());
    code
}

/// memcpy: copy rdx bytes from rsi to rdi
pub fn emit_memcpy() -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    // test rdx, rdx; jz done
    code.extend_from_slice(&[0x48, 0x85, 0xD2]);
    code.extend_from_slice(&[0x74, 0x06]); // je +6
    // .loop: mov al, [rsi]; mov [rdi], al; inc rsi; inc rdi; dec rdx; jnz .loop
    code.extend_from_slice(&[0x8A, 0x06]);           // mov al, [rsi]
    code.extend_from_slice(&[0x88, 0x07]);           // mov [rdi], al
    code.extend_from_slice(&[0x48, 0xFF, 0xC6]);     // inc rsi
    code.extend_from_slice(&[0x48, 0xFF, 0xC7]);     // inc rdi
    code.extend_from_slice(&[0x48, 0xFF, 0xCA]);     // dec rdx
    code.extend_from_slice(&[0x75, 0xF8]);           // jnz -8
    // ret
    code.extend_from_slice(&ret());
    code
}

/// memset: fill rdx bytes at rdi with al
pub fn emit_memset() -> alloc::vec::Vec<u8> {
    let mut code = alloc::vec::Vec::new();
    // test rdx, rdx; jz done
    code.extend_from_slice(&[0x48, 0x85, 0xD2]);
    code.extend_from_slice(&[0x74, 0x05]); // je +5
    // .loop: mov [rdi], al; inc rdi; dec rdx; jnz .loop
    code.extend_from_slice(&[0x88, 0x07]);           // mov [rdi], al
    code.extend_from_slice(&[0x48, 0xFF, 0xC7]);     // inc rdi
    code.extend_from_slice(&[0x48, 0xFF, 0xCA]);     // dec rdx
    code.extend_from_slice(&[0x75, 0xF9]);           // jnz -5
    // ret
    code.extend_from_slice(&ret());
    code
}
