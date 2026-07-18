// src/arch/arm64/timer.rs

//! ARMv8 generic timer driver (physical non-secure).
//!
//! The generic timer is a per-core countdown timer that raises PPI 30 when it
//! reaches zero. QEMU's `virt` machine populates `CNTFRQ_EL0` with the timer
//! frequency (typically 62.5 MHz). We program a tick interval of `freq / 100`
//! to get ~100 Hz — matching x86_64's PIT rate, so the scheduler switches at
//! the same cadence on both architectures.

use core::arch::asm;

/// Tick rate in Hz. The timer reloads to this interval on every IRQ.
const TIMER_HZ: u64 = 100;

/// Read the timer frequency (CNTFRQ_EL0).
#[inline]
fn frequency() -> u64 {
    let freq: u64;
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) freq); }
    freq
}

/// Read the current countdown value (CNTP_TVAL_EL0).
#[inline]
fn tval() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, cntp_tval_el0", out(reg) v); }
    v
}

/// Write the countdown value (CNTP_TVAL_EL0).
#[inline]
fn set_tval(v: u64) {
    unsafe { asm!("msr cntp_tval_el0, {}", in(reg) v); }
}

/// Control register (CNTP_CTL_EL0):
///   bit 0 = enable
///   bit 1 = IMASK (mask the IRQ)
///   bit 2 = ISTATUS (pending, read-only)
#[inline]
fn set_ctl(v: u64) {
    unsafe { asm!("msr cntp_ctl_el0, {}", in(reg) v); }
}

/// Configure and start the generic timer.
///
/// Programs the first tick interval and enables the timer. The IRQ itself
/// is gated at the CPU by DAIF until `arch::enable_interrupts()` is called.
pub fn init() {
    let interval = frequency() / TIMER_HZ;
    // Disable, clear any pending state, then set the interval.
    set_ctl(0); // disabled, unmasked, clears ISTATUS
    set_tval(interval);
    set_ctl(1); // enabled, unmasked
}

/// Tick interval in counter ticks (used by `handle_irq` to reload).
fn interval() -> u64 {
    frequency() / TIMER_HZ
}

/// Called from the IRQ trampoline on every timer interrupt. Reloads the
/// countdown so the next tick fires after one interval.
pub fn handle_irq() {
    set_tval(interval());
    // Ensure the reload is observed before the next interrupt.
    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
}

/// Read the raw counter (CNTPCT_EL0). Useful for diagnostics / entropy.
#[allow(dead_code)]
pub fn counter() -> u64 {
    let v: u64;
    unsafe { asm!("mrs {}, cntpct_el0", out(reg) v); }
    v
}
