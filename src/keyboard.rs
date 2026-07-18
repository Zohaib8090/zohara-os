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
// Called by the Shell
#[cfg(target_arch = "x86_64")]
pub fn try_get_key() -> Option<char> {
    KEY_BUFFER.lock(|buf| {
        if let Some(b) = buf {
            if let Some(data) = b.pop_front() {

                // Check for ANSI Escape Sequence (Arrow Keys)
                if data == 0x1B { // ESC byte
                    if b.len() >= 2 {
                        let c1 = b.pop_front();
                        let c2 = b.pop_front();
                        if c1 == Some(0x5B) { // '[' byte
                            match c2 {
                                Some(0x41) => return Some('\u{1000}'), // Up Arrow
                                Some(0x42) => return Some('\u{1001}'), // Down Arrow
                                Some(0x43) => return Some('\u{1002}'), // Right Arrow
                                Some(0x44) => return Some('\u{1003}'), // Left Arrow
                                _ => return None,
                            }
                        }
                    }
                    return None; // Just an ESC key press, ignore for now
                }

                // Convert Carriage Return to Newline
                if data == b'\r' {
                    return Some('\n');
                }

                // Convert Backspace/Delete (0x08 or 0x7F) to standard backspace
                if data == 0x08 || data == 0x7F {
                    return Some('\u{8}');
                }

                // Only return standard printable ASCII characters
                if data >= 0x20 && data <= 0x7E {
                    return Some(data as char);
                }
            }
        }
        None
    })
}