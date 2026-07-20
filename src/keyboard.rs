// src/keyboard.rs

use alloc::collections::VecDeque;
use core::cell::UnsafeCell;

// A simple lock that relies on interrupts being off during the handler
struct Mutex<T>(UnsafeCell<T>);
unsafe impl<T> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    const fn new(v: T) -> Self { Self(UnsafeCell::new(v)) }
    fn lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        f(unsafe { &mut *self.0.get() })
    }
}

static KEY_BUFFER: Mutex<Option<VecDeque<u8>>> = Mutex::new(None);

pub fn init_buffer() {
    KEY_BUFFER.lock(|buf| {
        *buf = Some(VecDeque::new());
    });
}

// Called by the Interrupt Handler
pub fn push_key(data: u8) {
    KEY_BUFFER.lock(|buf| {
        if let Some(b) = buf {
            b.push_back(data);
        }
    });
}

// --- Special key constants (Unicode Private Use Area) ---
pub const KEY_TAB: char = '\t';
pub const KEY_CTRL_A: char = '\u{0001}';
pub const KEY_CTRL_B: char = '\u{0002}';
pub const KEY_CTRL_C: char = '\u{0003}';
pub const KEY_CTRL_D: char = '\u{0004}';
pub const KEY_CTRL_E: char = '\u{0005}';
pub const KEY_CTRL_F: char = '\u{0006}';
pub const KEY_CTRL_K: char = '\u{000B}';
pub const KEY_CTRL_L: char = '\u{000C}';
pub const KEY_CTRL_N: char = '\u{000E}';
pub const KEY_CTRL_P: char = '\u{0010}';
pub const KEY_CTRL_U: char = '\u{0015}';
pub const KEY_CTRL_W: char = '\u{0017}';
pub const KEY_HOME: char = '\u{1004}';
pub const KEY_END: char = '\u{1005}';
pub const KEY_DELETE: char = '\u{1006}';
pub const KEY_PAGE_UP: char = '\u{1007}';
pub const KEY_PAGE_DOWN: char = '\u{1008}';

// Called by the Shell
#[cfg(target_arch = "x86_64")]
pub fn try_get_key() -> Option<char> {
    KEY_BUFFER.lock(|buf| {
        if let Some(b) = buf {
            if let Some(data) = b.pop_front() {

                // ANSI Escape Sequence
                if data == 0x1B { // ESC
                    if b.len() < 2 {
                        // Not enough bytes yet — push ESC back, try again next time
                        b.push_front(data);
                        return None;
                    }
                    let c1 = b.pop_front();
                    let c2 = b.pop_front();
                    if c1 == Some(0x5B) { // '['
                            match c2 {
                                Some(0x41) => return Some('\u{1000}'), // Up Arrow
                                Some(0x42) => return Some('\u{1001}'), // Down Arrow
                                Some(0x43) => return Some('\u{1002}'), // Right Arrow
                                Some(0x44) => return Some('\u{1003}'), // Left Arrow
                                Some(0x48) => return Some(KEY_HOME),     // Home
                                Some(0x46) => return Some(KEY_END),      // End
                                Some(0x33) => {
                                    // Delete: ESC [ 3 ~
                                    if b.len() >= 1 && b.pop_front() == Some(0x7E) {
                                        return Some(KEY_DELETE);
                                    }
                                    return None;
                                }
                                Some(0x35) => {
                                    // Page Up: ESC [ 5 ~
                                    if b.len() >= 1 && b.pop_front() == Some(0x7E) {
                                        return Some(KEY_PAGE_UP);
                                    }
                                    return None;
                                }
                                Some(0x36) => {
                                    // Page Down: ESC [ 6 ~
                                    if b.len() >= 1 && b.pop_front() == Some(0x7E) {
                                        return Some(KEY_PAGE_DOWN);
                                    }
                                    return None;
                                }
                                _ => return None,
                            }
                        } else if c1 == Some(0x4F) { // 'O' (F1-F4)
                            match c2 {
                                Some(0x50) => return Some('\u{1010}'), // F1
                                Some(0x51) => return Some('\u{1011}'), // F2
                                Some(0x52) => return Some('\u{1012}'), // F3
                                Some(0x53) => return Some('\u{1013}'), // F4
                                _ => return None,
                            }
                        }
                    return None;
                }

                // Control characters (Ctrl+key = key & 0x1F)
                match data {
                    0x01 => return Some(KEY_CTRL_A),   // Ctrl+A
                    0x02 => return Some(KEY_CTRL_B),   // Ctrl+B
                    0x03 => return Some(KEY_CTRL_C),   // Ctrl+C
                    0x04 => return Some(KEY_CTRL_D),   // Ctrl+D
                    0x05 => return Some(KEY_CTRL_E),   // Ctrl+E
                    0x06 => return Some(KEY_CTRL_F),   // Ctrl+F
                    0x0B => return Some(KEY_CTRL_K),   // Ctrl+K
                    0x0C => return Some(KEY_CTRL_L),   // Ctrl+L
                    0x0E => return Some(KEY_CTRL_N),   // Ctrl+N
                    0x10 => return Some(KEY_CTRL_P),   // Ctrl+P
                    0x15 => return Some(KEY_CTRL_U),   // Ctrl+U
                    0x17 => return Some(KEY_CTRL_W),   // Ctrl+W
                    _ => {}
                }

                // Tab
                if data == 0x09 { return Some(KEY_TAB); }

                // Carriage Return / Line Feed
                if data == b'\r' || data == 0x0A {
                    return Some('\n');
                }

                // Backspace / Delete
                if data == 0x08 || data == 0x7F {
                    return Some('\u{8}');
                }

                // Printable ASCII
                if data >= 0x20 && data <= 0x7E {
                    return Some(data as char);
                }
            }
        }
        None
    })
}
