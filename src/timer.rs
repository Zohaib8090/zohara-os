// src/timer.rs

//! Timer / Clock subsystem.
//!
//! Manages the global monotonic tick counter, provides time conversion
//! APIs, and handles sleeping task wake-up. The PIT fires at ~100 Hz
//! (divisor 11932 → 1,193,182 / 11932 ≈ 100.0 Hz).

use core::sync::atomic::{AtomicUsize, Ordering};

/// PIT channel 0 frequency divisor (1,193,182 / 11932 ≈ 100 Hz).
pub const PIT_DIVISOR: u16 = 11932;

/// Actual timer frequency in Hz (approximate).
pub const PIT_HZ: usize = 100;

/// Milliseconds per timer tick (1000 / 100 = 10).
pub const MS_PER_TICK: usize = 10;

/// Global monotonic tick counter. Incremented once per timer IRQ.
static TICKS: AtomicUsize = AtomicUsize::new(0);

/// Read the raw tick count since boot.
pub fn ticks() -> usize {
    TICKS.load(Ordering::SeqCst)
}

/// Return elapsed time since boot in milliseconds.
///
/// Uses wrapping arithmetic — on a 64-bit system this won't wrap for ~184 years.
pub fn uptime_ms() -> usize {
    ticks().wrapping_mul(MS_PER_TICK)
}

/// Convert milliseconds to ticks (ceiling division).
///
/// `ms_to_ticks(0)` → 0, `ms_to_ticks(1)` → 1, `ms_to_ticks(10)` → 1,
/// `ms_to_ticks(11)` → 2.
pub fn ms_to_ticks(ms: usize) -> usize {
    (ms + MS_PER_TICK - 1) / MS_PER_TICK
}

/// Called by `timer_handler_rust` on every timer IRQ.
/// Increments the global tick counter.
pub fn tick() {
    TICKS.fetch_add(1, Ordering::SeqCst);
}

/// Scan all tasks and wake any that have passed their wake_tick.
///
/// Called from `timer_handler_rust` on every tick, before the
/// context-switch logic.
pub fn wake_sleeping(tasks: &mut [crate::task::Task; crate::task::MAX_TASKS]) {
    let current = ticks();
    for i in 1..crate::task::MAX_TASKS {
        if tasks[i].state == crate::task::TaskState::Sleeping
            && current >= tasks[i].wake_tick
        {
            tasks[i].state = crate::task::TaskState::Ready;
        }
    }
}

/// Put the current task to sleep until `wake_tick` is reached.
///
/// Enables interrupts and halts in a loop. The timer IRQ fires,
/// `wake_sleeping` transitions the task to Ready, and the next
/// scheduler tick picks it up.
pub fn sleep_until(wake_tick: usize) {
    unsafe {
        let cur = crate::task::current_task();
        crate::task::set_task_wake_tick(cur, wake_tick);
        crate::task::set_state(crate::task::TaskState::Sleeping);
        core::arch::asm!("sti");
        loop {
            core::arch::asm!("hlt");
            if crate::task::current_state() != crate::task::TaskState::Sleeping {
                break;
            }
        }
    }
}

/// Put the current task to sleep for `duration_ms` milliseconds.
pub fn sleep_ms(duration_ms: usize) {
    let wake = ticks().wrapping_add(ms_to_ticks(duration_ms));
    sleep_until(wake);
}
