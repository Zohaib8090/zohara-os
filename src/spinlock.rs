// src/spinlock.rs

//! Minimal ticket spinlock — no heap, no deps, `no_std`-safe.
//!
//! Ticket locks provide FIFO ordering: threads acquire in the order they
//! request the lock, preventing starvation. Under low contention (typical
//! for kernel subsystems), the ticket + spin_loop pattern is fast enough.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

pub struct SpinLock<T> {
    ticket: AtomicU32,
    next: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> SpinLock<T> {
    pub const fn new(data: T) -> Self {
        Self {
            ticket: AtomicU32::new(0),
            next: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        let ticket = self.ticket.fetch_add(1, Ordering::Relaxed);
        while self.next.load(Ordering::Acquire) != ticket {
            core::hint::spin_loop();
        }
        SpinLockGuard { lock: self }
    }

    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        let current = self.ticket.load(Ordering::Relaxed);
        let next = self.next.load(Ordering::Relaxed);
        if current == next {
            if self.ticket.compare_exchange(
                current,
                current + 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ).is_ok() {
                while self.next.load(Ordering::Acquire) != current {
                    core::hint::spin_loop();
                }
                return Some(SpinLockGuard { lock: self });
            }
        }
        None
    }
}

impl<T> core::ops::Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> core::ops::DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.next.fetch_add(1, Ordering::Release);
    }
}
