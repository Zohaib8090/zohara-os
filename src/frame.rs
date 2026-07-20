// src/frame.rs

//! Arch-agnostic physical frame allocator.
//!
//! Tracks 4 KiB frames across RAM using a flat bitmap. The total RAM size is
//! detected at boot (E820 on x86_64, FDT on ARM64) and stored in a mutable
//! static — the bitmap reserves space for up to 8 GiB but only the detected
//! range is active.
//!
//! Frames covering the kernel image, static heap, this bitmap, and the boot
//! stack are reserved at `init()` time; everything else is free for page
//! tables, user pages, and future allocations.
//!
//! When tasks exit, their frames are reclaimed via `deallocate_frame`.

use core::sync::atomic::{AtomicUsize, Ordering};
use crate::spinlock::SpinLock;

pub const FRAME_SIZE: usize = 4096;

#[cfg(target_arch = "aarch64")]
pub const RAM_BASE: usize = 0x4000_0000;
#[cfg(target_arch = "x86_64")]
pub const RAM_BASE: usize = 0;

/// Maximum RAM the bitmap can track: 8 GiB.
/// 8 GiB / 4 KiB / 64 = 32 768 words = 256 KiB of BSS.
const MAX_WORDS: usize = 32_768;
const MAX_FRAME_COUNT: usize = MAX_WORDS * 64;

/// The bitmap storage — big enough for 8 GiB of RAM.
static mut BITMAP: [u64; MAX_WORDS] = [0; MAX_WORDS];

/// Actual number of frames we manage (set at boot from detected RAM).
static mut ACTUAL_WORDS: usize = 0;
static mut ACTUAL_FRAME_COUNT: usize = 0;
static mut TOTAL_RAM: usize = 0;

/// Scan hint so allocate_frame doesn't restart from 0 every time.
static NEXT_FREE: AtomicUsize = AtomicUsize::new(0);

/// Global lock protecting bitmap mutations — prevents double-allocation under SMP.
static FRAME_LOCK: SpinLock<()> = SpinLock::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysFrame {
    pub number: usize,
}

impl PhysFrame {
    pub fn from_addr(addr: usize) -> Self {
        Self { number: (addr - RAM_BASE) / FRAME_SIZE }
    }
    pub fn start_address(&self) -> usize {
        RAM_BASE + self.number * FRAME_SIZE
    }
}

fn frame_of(addr: usize) -> usize {
    addr.saturating_sub(RAM_BASE) / FRAME_SIZE
}

/// Set the total RAM size before calling `init()`. Caps at 8 GiB.
pub fn set_total_ram(size: usize) {
    let capped = size.min(MAX_FRAME_COUNT * FRAME_SIZE);
    unsafe {
        TOTAL_RAM = capped;
        ACTUAL_FRAME_COUNT = capped / FRAME_SIZE;
        ACTUAL_WORDS = (ACTUAL_FRAME_COUNT + 63) / 64;
    }
}

/// Get the total RAM the allocator manages (in bytes).
pub fn total_ram() -> usize {
    unsafe { TOTAL_RAM }
}

/// Get the number of free frames.
pub fn free_frame_count() -> usize {
    unsafe {
        let words = ACTUAL_WORDS;
        let count = ACTUAL_FRAME_COUNT;
        let bitmap_ptr = core::ptr::addr_of!(BITMAP) as *const u64;
        let mut free = 0usize;
        for w in 0..words {
            let bits = bitmap_ptr.add(w).read_volatile();
            let mut inverted = !bits;
            // Mask off bits beyond ACTUAL_FRAME_COUNT in the last word.
            let word_start = w * 64;
            if word_start + 64 > count {
                let valid = count - word_start;
                inverted &= (1u64 << valid) - 1;
            }
            free += inverted.count_ones() as usize;
        }
        free
    }
}

unsafe fn set_used(frame: usize) {
    if frame >= ACTUAL_FRAME_COUNT { return; }
    let bitmap_ptr = core::ptr::addr_of_mut!(BITMAP) as *mut u64;
    bitmap_ptr.add(frame / 64).write_volatile(
        bitmap_ptr.add(frame / 64).read_volatile() | (1u64 << (frame % 64)),
    );
}

unsafe fn set_free(frame: usize) {
    if frame >= ACTUAL_FRAME_COUNT { return; }
    let bitmap_ptr = core::ptr::addr_of_mut!(BITMAP) as *mut u64;
    bitmap_ptr.add(frame / 64).write_volatile(
        bitmap_ptr.add(frame / 64).read_volatile() & !(1u64 << (frame % 64)),
    );
}

pub unsafe fn mark_used(start: usize, end: usize) {
    let _guard = FRAME_LOCK.lock();
    let ram_end = RAM_BASE + TOTAL_RAM;
    if end <= start || start >= ram_end { return; }
    let first = frame_of(start);
    let last = frame_of(end.saturating_sub(1)).min(ACTUAL_FRAME_COUNT - 1);
    for frame in first..=last {
        set_used(frame);
    }
}

extern "Rust" {
    static _end: u8;
}

pub fn init() {
    unsafe {
        // If set_total_ram was never called, default to 1 GiB.
        if TOTAL_RAM == 0 {
            set_total_ram(1 << 30);
        }

        let bitmap_ptr = core::ptr::addr_of_mut!(BITMAP) as *mut u64;
        for i in 0..ACTUAL_WORDS {
            bitmap_ptr.add(i).write_volatile(0);
        }

        let kernel_end = core::ptr::addr_of!(_end) as usize;
        mark_used(RAM_BASE, kernel_end);

        #[cfg(target_arch = "x86_64")]
        mark_used(0, 0x100000);

        NEXT_FREE.store(frame_of(kernel_end) / 64, Ordering::Relaxed);
    }
}

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

pub fn allocate_frame() -> Option<PhysFrame> {
    let _guard = FRAME_LOCK.lock();
    unsafe {
        let words = ACTUAL_WORDS;
        let count = ACTUAL_FRAME_COUNT;
        let start = NEXT_FREE.load(Ordering::Relaxed).min(words);
        let bitmap_ptr = core::ptr::addr_of_mut!(BITMAP) as *mut u64;

        for w in start..words {
            let bits = bitmap_ptr.add(w).read_volatile();
            if bits != !0 {
                let bit = bits.trailing_ones() as usize;
                let frame = w * 64 + bit;
                if frame < count {
                    set_used(frame);
                    NEXT_FREE.store(w, Ordering::Relaxed);
                    ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
                    crate::stats::count_frame_alloc();
                    return Some(PhysFrame { number: frame });
                }
            }
        }

        for w in 0..start {
            let bits = bitmap_ptr.add(w).read_volatile();
            if bits != !0 {
                let bit = bits.trailing_ones() as usize;
                let frame = w * 64 + bit;
                if frame < count {
                    set_used(frame);
                    NEXT_FREE.store(w, Ordering::Relaxed);
                    ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
                    crate::stats::count_frame_alloc();
                    return Some(PhysFrame { number: frame });
                }
            }
        }

        None
    }
}

pub fn alloc_count() -> usize {
    ALLOC_COUNT.load(Ordering::Relaxed)
}

pub fn deallocate_frame(frame: PhysFrame) {
    let _guard = FRAME_LOCK.lock();
    if frame.number >= unsafe { ACTUAL_FRAME_COUNT } { return; }
    unsafe { set_free(frame.number) }
    crate::stats::count_frame_free();
}
