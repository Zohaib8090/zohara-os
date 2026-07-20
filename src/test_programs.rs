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

    /// Sleep test: calls Sleep(500), then writes "SLEEP OK\n", then exits.
    ///
    /// Layout:
    ///   0: mov rax, 2         (Sleep syscall)
    ///   7: mov rdi, 500       (500 ms)
    ///  14: int 0x80
    ///  16: mov rax, 0         (Write syscall)
    ///  23: lea rdi, [rip+18]  (-> msg at offset 48; RIP after=30, 30+18=48)
    ///  30: mov rsi, 9         (msg length)
    ///  37: int 0x80
    ///  39: mov rax, 1         (Exit)
    ///  46: int 0x80
    ///  48: "SLEEP OK\n"       (9 bytes)
    pub static SLEEP_TEST: &[u8] = &[
        // Sleep(500)
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00, // mov rax, 2
        0x48, 0xC7, 0xC7, 0xF4, 0x01, 0x00, 0x00, // mov rdi, 500
        0xCD, 0x80,                                  // int 0x80
        // Write(msg, 9)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
        0x48, 0x8D, 0x3D, 0x12, 0x00, 0x00, 0x00, // lea rdi, [rip+18]
        0x48, 0xC7, 0xC6, 0x09, 0x00, 0x00, 0x00, // mov rsi, 9
        0xCD, 0x80,                                  // int 0x80
        // Exit
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
        0xCD, 0x80,                                  // int 0x80
        // msg: "SLEEP OK\n"
        b'S', b'L', b'E', b'E', b'P', b' ', b'O', b'K', b'\n',
    ];

    /// Syscall test: GetPid prints "PID=N", Yield, then exit.
    /// Single-digit only (task index < 10). No stack usage.
    /// Stores digit at a scratch byte within the code page (RWX-mapped).
    ///
    /// Layout:
    ///   0-8:   GetPid (9 bytes)
    ///   9-12:  add rax, 0x30 (4 bytes)
    ///  13-19:  lea rdi, [rip+47] → scratch at offset 67 (7 bytes)
    ///  20-21:  mov [rdi], al (2 bytes)
    ///  22-28:  mov rax, 0 / Write (7 bytes)
    ///  29-35:  lea rdi, [rip+27] → "PID=" at offset 63 (7 bytes)
    ///  36-42:  mov rsi, 5 (7 bytes)
    ///  43-44:  int 0x80 (2 bytes)
    ///  45-53:  Yield (9 bytes)
    ///  54-62:  Exit (9 bytes)
    ///  63-66:  "PID=" (4 bytes)
    ///     67:  scratch byte (1 byte)
    pub static SYSCALL_TEST: &[u8] = &[
        // GetPid
        0x48, 0xC7, 0xC0, 0x04, 0x00, 0x00, 0x00, // 0:  mov rax, 4
        0xCD, 0x80,                                  // 7:  int 0x80
        // Convert PID to ASCII digit
        0x48, 0x83, 0xC0, 0x30,                      // 9:  add rax, 0x30
        // Store digit at scratch byte (offset 67 in code page)
        0x48, 0x8D, 0x3D, 0x2F, 0x00, 0x00, 0x00, // 13: lea rdi, [rip+47] → 67
        0x88, 0x07,                                  // 20: mov [rdi], al
        // Write("PID=" + digit, 5)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // 22: mov rax, 0
        0x48, 0x8D, 0x3D, 0x1B, 0x00, 0x00, 0x00, // 29: lea rdi, [rip+27] → 63
        0x48, 0xC7, 0xC6, 0x05, 0x00, 0x00, 0x00, // 36: mov rsi, 5
        0xCD, 0x80,                                  // 43: int 0x80
        // Yield
        0x48, 0xC7, 0xC0, 0x05, 0x00, 0x00, 0x00, // 45: mov rax, 5
        0xCD, 0x80,                                  // 52: int 0x80
        // Exit
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // 54: mov rax, 1
        0xCD, 0x80,                                  // 61: int 0x80
        // Data
        b'P', b'I', b'D', b'=',                      // 63: "PID="
        0x00,                                         // 67: scratch byte (overwritten with digit)
    ];

    /// Yield interleaving test: prints a character, yields, prints another
    /// character, exits. Two copies (Y and Z) are spawned before the timer
    /// starts. If Yield truly forces a reschedule, the output should
    /// interleave: Y Z Y2 Z2 (not YY ZZ).
    ///
    /// Layout:
    ///   0:  Write("Y", 1)
    ///  23:  Yield
    ///  32:  Write("Y2", 2)
    ///  57:  Exit
    ///  59:  "Y\n" (2 bytes)
    ///  61:  "Y2\n" (3 bytes)
    pub static YIELD_TEST_Y: &[u8] = &[
        // Write("Y", 1)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // 0:  mov rax, 0
        0x48, 0x8D, 0x3D, 0x32, 0x00, 0x00, 0x00, // 7:  lea rdi, [rip+50] → 64 "Y"
        0x48, 0xC7, 0xC6, 0x01, 0x00, 0x00, 0x00, // 14: mov rsi, 1
        0xCD, 0x80,                                  // 21: int 0x80
        // Yield — forces reschedule if working
        0x48, 0xC7, 0xC0, 0x05, 0x00, 0x00, 0x00, // 23: mov rax, 5
        0xCD, 0x80,                                  // 30: int 0x80
        // Write("Y2", 2)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // 32: mov rax, 0
        0x48, 0x8D, 0x3D, 0x13, 0x00, 0x00, 0x00, // 39: lea rdi, [rip+19] → 65 "Y2"
        0x48, 0xC7, 0xC6, 0x02, 0x00, 0x00, 0x00, // 46: mov rsi, 2
        0xCD, 0x80,                                  // 53: int 0x80
        // Exit
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // 55: mov rax, 1
        0xCD, 0x80,                                  // 62: int 0x80
        // Data
        b'Y',                                         // 64: "Y"
        b'Y', b'2',                                   // 65: "Y2"
    ];

    pub static YIELD_TEST_Z: &[u8] = &[
        // Write("Z", 1)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // 0:  mov rax, 0
        0x48, 0x8D, 0x3D, 0x32, 0x00, 0x00, 0x00, // 7:  lea rdi, [rip+50] → 64 "Z"
        0x48, 0xC7, 0xC6, 0x01, 0x00, 0x00, 0x00, // 14: mov rsi, 1
        0xCD, 0x80,                                  // 21: int 0x80
        // Yield
        0x48, 0xC7, 0xC0, 0x05, 0x00, 0x00, 0x00, // 23: mov rax, 5
        0xCD, 0x80,                                  // 30: int 0x80
        // Write("Z2", 2)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // 32: mov rax, 0
        0x48, 0x8D, 0x3D, 0x13, 0x00, 0x00, 0x00, // 39: lea rdi, [rip+19] → 65 "Z2"
        0x48, 0xC7, 0xC6, 0x02, 0x00, 0x00, 0x00, // 46: mov rsi, 2
        0xCD, 0x80,                                  // 53: int 0x80
        // Exit
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // 55: mov rax, 1
        0xCD, 0x80,                                  // 62: int 0x80
        // Data
        b'Z',                                         // 64: "Z"
        b'Z', b'2',                                   // 65: "Z2"
    ];

    /// Permission test: calls TestPriv (syscall 6, requires UID 0).
    /// A normal userspace task (UID 1000) should get -EPERM (-13 = 0xFFF...F3).
    ///
    /// Layout:
    ///   0: mov rax, 6         (TestPriv syscall)
    ///   7: int 0x80           (returns -13 in rax)
    ///   9: mov rax, 0         (Write)
    ///  16: lea rdi, [rip+18]  (-> msg at offset 34; RIP after=23, 23+18=41... wait)
    ///
    /// Actually recalculating:
    ///   0: mov rax, 6         (7 bytes)
    ///   7: int 0x80           (2 bytes)
    ///   9: mov rax, 0         (7 bytes)
    ///  16: lea rdi, [rip+X]   (7 bytes, RIP after=23)
    ///  23: mov rsi, 14        (7 bytes)
    ///  30: int 0x80           (2 bytes)
    ///  32: mov rax, 1         (7 bytes)
    ///  39: int 0x80           (2 bytes)
    ///  41: "PERM DENIED OK\n" (14 bytes)
    pub static PERM_TEST: &[u8] = &[
        // DebugLog (requires UID 0, syscall 12)
        0x48, 0xC7, 0xC0, 0x0C, 0x00, 0x00, 0x00, // mov rax, 12
        0xCD, 0x80,                                  // int 0x80
        // Write("PERM DENIED OK\n", 14)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // mov rax, 0
        0x48, 0x8D, 0x3D, 0x12, 0x00, 0x00, 0x00, // lea rdi, [rip+18]
        0x48, 0xC7, 0xC6, 0x0E, 0x00, 0x00, 0x00, // mov rsi, 14
        0xCD, 0x80,                                  // int 0x80
        // Exit
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
        0xCD, 0x80,                                  // int 0x80
        // msg: "PERM DENIED OK\n"
        b'P', b'E', b'R', b'M', b' ', b'D', b'E', b'N',
        b'I', b'E', b'D', b' ', b'O', b'K', b'\n',
    ];

    /// Runtime test: GetPid + Write + Exit. Minimal proof that runtime works.
    pub static RUNTIME_TEST: &[u8] = &[
        // GetPid → rax
        0x48, 0xC7, 0xC0, 0x04, 0x00, 0x00, 0x00, // 0:  mov rax, 4
        0xCD, 0x80,                                  // 7:  int 0x80
        // Write("RT OK\n", 6)
        0x48, 0xC7, 0xC0, 0x00, 0x00, 0x00, 0x00, // 9:  mov rax, 0
        0x48, 0x8D, 0x3D, 0x18, 0x00, 0x00, 0x00, // 16: lea rdi, [rip+24] → 48 "RT OK\n"
        0x48, 0xC7, 0xC6, 0x06, 0x00, 0x00, 0x00, // 23: mov rsi, 6
        0xCD, 0x80,                                  // 30: int 0x80
        // Exit(0)
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // 32: mov rax, 1
        0x48, 0xC7, 0xC7, 0x00, 0x00, 0x00, 0x00, // 39: mov rdi, 0
        0xCD, 0x80,                                  // 46: int 0x80
        // Data
        b'R', b'T', b' ', b'O', b'K', b'\n',       // 48: "RT OK\n"
    ];
}
