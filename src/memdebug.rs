// src/memdebug.rs

//! Memory diagnostics — debugging and monitoring for the memory subsystem.
//!
//! Tracks allocation patterns, detects potential leaks, and provides
//! heap statistics. All tracking is optional and zero-cost when disabled.

/// Memory diagnostic counters.
pub struct MemStats {
    pub heap_total: usize,
    pub heap_used: usize,
    pub heap_free: usize,
    pub kernel_pages: usize,
    pub user_pages: usize,
    pub reserved_pages: usize,
    pub total_frames: usize,
    pub free_frames: usize,
    pub used_frames: usize,
    pub alloc_count: usize,
    pub free_count: usize,
}

impl MemStats {
    pub fn new() -> Self {
        let total = crate::frame::total_ram() / crate::frame::FRAME_SIZE;
        let free = crate::frame::free_frame_count();
        let used = total - free;
        Self {
            heap_total: 65536, // 64 KB fixed heap
            heap_used: 0, // TODO: track actual heap usage
            heap_free: 65536,
            kernel_pages: 0, // TODO: compute from frame allocator
            user_pages: 0,
            reserved_pages: 0,
            total_frames: total,
            free_frames: free,
            used_frames: used,
            alloc_count: crate::stats::STATS.frame_allocs.load(core::sync::atomic::Ordering::Relaxed),
            free_count: crate::stats::STATS.frame_frees.load(core::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Print memory diagnostics.
pub fn dump() {
    let stats = MemStats::new();
    crate::println!("=== Memory Diagnostics ===");
    crate::println!("  Total frames:     {} ({} KiB)", stats.total_frames, stats.total_frames * 4);
    crate::println!("  Free frames:      {} ({} KiB)", stats.free_frames, stats.free_frames * 4);
    crate::println!("  Used frames:      {} ({} KiB)", stats.used_frames, stats.used_frames * 4);
    crate::println!("  Heap total:       {} bytes", stats.heap_total);
    crate::println!("  Frame allocs:     {}", stats.alloc_count);
    crate::println!("  Frame frees:      {}", stats.free_count);
    let leak = stats.alloc_count as isize - stats.free_count as isize;
    if leak > 0 {
        crate::println!("  Potential leak:   {} frames ({} bytes)", leak, leak as usize * 4096);
    } else {
        crate::println!("  Leak:             none (net free = {})", -leak);
    }
}
