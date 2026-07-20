// src/stats.rs

//! Kernel statistics — lightweight counters for observability.
//!
//! All counters are AtomicUsize for lock-free increment from any context
//! (including interrupt handlers). No heap allocation.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Global kernel statistics.
pub struct KernelStats {
    pub context_switches: AtomicUsize,
    pub timer_ticks: AtomicUsize,
    pub syscalls_total: AtomicUsize,
    pub syscalls_by_num: [AtomicUsize; 64],
    pub page_faults_user: AtomicUsize,
    pub page_faults_kernel: AtomicUsize,
    pub interrupts_total: AtomicUsize,
    pub frame_allocs: AtomicUsize,
    pub frame_frees: AtomicUsize,
    pub tasks_spawned: AtomicUsize,
    pub tasks_exited: AtomicUsize,
    pub panics: AtomicUsize,
}

impl KernelStats {
    const fn new() -> Self {
        // Can't use array init with AtomicUsize::new() in const context
        // for all elements, so we use a helper.
        Self {
            context_switches: AtomicUsize::new(0),
            timer_ticks: AtomicUsize::new(0),
            syscalls_total: AtomicUsize::new(0),
            syscalls_by_num: new_atomic_array(),
            page_faults_user: AtomicUsize::new(0),
            page_faults_kernel: AtomicUsize::new(0),
            interrupts_total: AtomicUsize::new(0),
            frame_allocs: AtomicUsize::new(0),
            frame_frees: AtomicUsize::new(0),
            tasks_spawned: AtomicUsize::new(0),
            tasks_exited: AtomicUsize::new(0),
            panics: AtomicUsize::new(0),
        }
    }
}

const fn new_atomic_array() -> [AtomicUsize; 64] {
    // Const-friendly array init: all zeros.
    // AtomicUsize(0) is valid for the initial state.
    // We use a workaround since we can't call AtomicUsize::new in const
    // on stable Rust. Use a simpler approach.
    [
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
        AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0), AtomicUsize::new(0),
    ]
}

pub static STATS: KernelStats = KernelStats::new();

// ---- convenience increment functions (usable from any context) ----

pub fn count_context_switch() { STATS.context_switches.fetch_add(1, Ordering::Relaxed); }
pub fn count_tick() { STATS.timer_ticks.fetch_add(1, Ordering::Relaxed); }
pub fn count_syscall(num: usize) {
    STATS.syscalls_total.fetch_add(1, Ordering::Relaxed);
    if num < 64 {
        STATS.syscalls_by_num[num].fetch_add(1, Ordering::Relaxed);
    }
}
pub fn count_page_fault_user() { STATS.page_faults_user.fetch_add(1, Ordering::Relaxed); }
pub fn count_page_fault_kernel() { STATS.page_faults_kernel.fetch_add(1, Ordering::Relaxed); }
pub fn count_interrupt() { STATS.interrupts_total.fetch_add(1, Ordering::Relaxed); }
pub fn count_frame_alloc() { STATS.frame_allocs.fetch_add(1, Ordering::Relaxed); }
pub fn count_frame_free() { STATS.frame_frees.fetch_add(1, Ordering::Relaxed); }
pub fn count_task_spawn() { STATS.tasks_spawned.fetch_add(1, Ordering::Relaxed); }
pub fn count_task_exit() { STATS.tasks_exited.fetch_add(1, Ordering::Relaxed); }
pub fn count_panic() { STATS.panics.fetch_add(1, Ordering::Relaxed); }

/// Print a summary of all statistics.
pub fn dump() {
    crate::println!("=== Kernel Statistics ===");
    crate::println!("  Context switches: {}", STATS.context_switches.load(Ordering::Relaxed));
    crate::println!("  Timer ticks:      {}", STATS.timer_ticks.load(Ordering::Relaxed));
    crate::println!("  Syscalls total:   {}", STATS.syscalls_total.load(Ordering::Relaxed));
    crate::println!("  Page faults user: {}", STATS.page_faults_user.load(Ordering::Relaxed));
    crate::println!("  Page faults kern: {}", STATS.page_faults_kernel.load(Ordering::Relaxed));
    crate::println!("  Interrupts:       {}", STATS.interrupts_total.load(Ordering::Relaxed));
    crate::println!("  Frame allocs:     {}", STATS.frame_allocs.load(Ordering::Relaxed));
    crate::println!("  Frame frees:      {}", STATS.frame_frees.load(Ordering::Relaxed));
    crate::println!("  Tasks spawned:    {}", STATS.tasks_spawned.load(Ordering::Relaxed));
    crate::println!("  Tasks exited:     {}", STATS.tasks_exited.load(Ordering::Relaxed));
    crate::println!("  Panics:           {}", STATS.panics.load(Ordering::Relaxed));
}
