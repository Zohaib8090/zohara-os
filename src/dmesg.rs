// src/dmesg.rs

//! In-memory kernel log ring buffer.
//!
//! Captures all `print!`/`println!` output alongside serial writes so
//! `dmesg` can dump recent history without relying on external tools.

use crate::spinlock::SpinLock;

/// Ring buffer capacity in bytes. No heap — just a static array.
const DMESG_BUF_SIZE: usize = 16384;

static mut BUF: [u8; DMESG_BUF_SIZE] = [0; DMESG_BUF_SIZE];
static mut WRITE_POS: usize = 0;
static mut Wrapped: bool = false;
static DMESG_LOCK: SpinLock<()> = SpinLock::new(());

/// Append a single byte to the ring buffer.
pub fn push_byte(b: u8) {
    let _guard = DMESG_LOCK.lock();
    unsafe {
        BUF[WRITE_POS] = b;
        WRITE_POS += 1;
        if WRITE_POS >= DMESG_BUF_SIZE {
            WRITE_POS = 0;
            Wrapped = true;
        }
    }
}

/// Append a byte slice to the ring buffer.
pub fn push_bytes(data: &[u8]) {
    for &b in data {
        push_byte(b);
    }
}

/// Dump the ring buffer contents to serial.
///
/// If the buffer has wrapped, we print from `WRITE_POS` (oldest) to
/// `WRITE_POS - 1` (newest), which gives the most recent DMESG_BUF_SIZE
/// bytes of output. If it hasn't wrapped, we print from 0 to WRITE_POS.
pub fn dump() {
    let _guard = DMESG_LOCK.lock();
    unsafe {
        let total = if Wrapped { DMESG_BUF_SIZE } else { WRITE_POS };
        if total == 0 {
            crate::println!("[dmesg] Buffer empty.");
            return;
        }
        let start = if Wrapped { WRITE_POS } else { 0 };
        crate::println!("--- dmesg ({} bytes) ---", total);
        for i in 0..total {
            let idx = (start + i) % DMESG_BUF_SIZE;
            let b = BUF[idx];
            if b != 0 {
                crate::arch::write_serial(b);
            }
        }
        crate::println!("\n--- end dmesg ---");
    }
}
