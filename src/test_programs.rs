// src/test_programs.rs

//! Embedded userspace test programs as raw machine code byte arrays.

#[cfg(target_arch = "x86_64")]
pub mod x86_64_programs {
    pub static TASK_A: &[u8] = &[
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
        0x48, 0x8D, 0x3D, 0x12, 0x00, 0x00, 0x00, // lea rdi, [rip+18]
        0x48, 0xC7, 0xC6, 0x13, 0x00, 0x00, 0x00, // mov rsi, 19
        0xCD, 0x80,                                  // int 0x80
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
        0xCD, 0x80,                                  // int 0x80
        b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r',
        b'o', b'm', b' ', b'T', b'a', b's', b'k', b' ',
        b'A', b'!', 0x0A,
    ];

    pub static TASK_B: &[u8] = &[
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00,
        0x48, 0x8D, 0x3D, 0x12, 0x00, 0x00, 0x00,
        0x48, 0xC7, 0xC6, 0x13, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
        0xCD, 0x80,
        b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r',
        b'o', b'm', b' ', b'T', b'a', b's', b'k', b' ',
        b'B', b'!', 0x0A,
    ];

    pub static PRIV_TEST: &[u8] = &[
        0xF4,       // hlt
        0xEB, 0xFC, // jmp self
    ];

    pub static PRIV_TEST_2: &[u8] = &[
        0xFA,       // cli
        0xEB, 0xFC, // jmp self
    ];

    /// Pointer validation test: calls Write(0x100000, 5) — a kernel-only
    /// address that's mapped but has US=0. copy_from_user should reject it
    /// and return -14 (EFAULT). Then writes a confirmation message.
    ///
    /// Layout:
    ///   0: mov rax, 0         (Write syscall)
    ///   7: mov rdi, 0x100000  (kernel address, US=0)
    ///  14: mov rsi, 5         (length)
    ///  21: int 0x80           (returns -14 in rax)
    ///  23: mov rax, 0         (Write syscall again)
    ///  30: lea rdi, [rip+X]   (-> msg at offset 44)
    ///  37: mov rsi, 21        (msg length)
    ///  44: int 0x80           (print confirmation)
    ///  46: mov rax, 1         (Exit)
    ///  53: int 0x80
    ///  55: "ptr_bad: EFAULT ok\n"
    pub static PTR_TEST_BAD: &[u8] = &[
        // Write(0x100000, 5) — kernel address, US=0
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
        0x48, 0xC7, 0xC7, 0x00, 0x00, 0x10, 0x00, // mov rdi, 0x100000
        0x48, 0xC7, 0xC6, 0x05, 0x00, 0x00, 0x00, // mov rsi, 5
        0xCD, 0x80,                                  // int 0x80
        // Write(msg, 21) — print confirmation
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
        0x48, 0x8D, 0x3D, 0x0D, 0x00, 0x00, 0x00, // lea rdi, [rip+13]
        0x48, 0xC7, 0xC6, 0x15, 0x00, 0x00, 0x00, // mov rsi, 21
        0xCD, 0x80,                                  // int 0x80
        // Exit
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
        0xCD, 0x80,                                  // int 0x80
        // msg: "ptr_bad: EFAULT ok\n"
        b'p', b't', b'r', b'_', b'b', b'a', b'd', b':',
        b' ', b'E', b'F', b'A', b'U', b'L', b'T', b' ',
        b'o', b'k', 0x0A,
    ];
}
