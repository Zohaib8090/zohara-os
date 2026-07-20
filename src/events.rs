// src/events.rs

//! Event framework — interrupt-to-kernel deferred processing.
//!
//! Instead of doing heavy work in interrupt handlers, ISRs queue
//! events that kernel worker threads process later.
//!
//! Future: keyboard, mouse, timer, disk, network, power events.

/// Event types.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq)]
pub enum EventType {
    Keyboard,
    Timer,
    Disk,
    Network,
    Power,
    Custom(u8),
}

/// A queued event.
#[derive(Clone, Copy)]
pub struct Event {
    pub event_type: EventType,
    pub data0: usize,
    pub data1: usize,
}

/// Fixed-size event ring buffer (no heap).
const EVENT_QUEUE_SIZE: usize = 128;

static mut EVENT_QUEUE: [Option<Event>; EVENT_QUEUE_SIZE] = [None; EVENT_QUEUE_SIZE];
static mut EVENT_HEAD: usize = 0;  // next write position
static mut EVENT_TAIL: usize = 0;  // next read position
static mut EVENT_COUNT: usize = 0;
static EVENT_LOCK: crate::spinlock::SpinLock<()> = crate::spinlock::SpinLock::new(());

/// Push an event into the queue. Returns Ok(()) or Err if full.
pub fn push_event(event: Event) -> Result<(), ()> {
    let _guard = EVENT_LOCK.lock();
    unsafe {
        if EVENT_COUNT >= EVENT_QUEUE_SIZE {
            return Err(());
        }
        EVENT_QUEUE[EVENT_HEAD] = Some(event);
        EVENT_HEAD = (EVENT_HEAD + 1) % EVENT_QUEUE_SIZE;
        EVENT_COUNT += 1;
        Ok(())
    }
}

/// Pop the next event from the queue. Returns None if empty.
pub fn pop_event() -> Option<Event> {
    let _guard = EVENT_LOCK.lock();
    unsafe {
        if EVENT_COUNT == 0 {
            return None;
        }
        let event = EVENT_QUEUE[EVENT_TAIL].take();
        EVENT_TAIL = (EVENT_TAIL + 1) % EVENT_QUEUE_SIZE;
        EVENT_COUNT -= 1;
        event
    }
}

/// Number of pending events.
pub fn pending_count() -> usize {
    let _guard = EVENT_LOCK.lock();
    unsafe { EVENT_COUNT }
}

/// Process all pending events (called from kernel main loop / idle).
pub fn process_events() {
    while let Some(event) = pop_event() {
        match event.event_type {
            EventType::Keyboard => {
                crate::trace!("events", "keyboard event: data={:#x}", event.data0);
            }
            EventType::Timer => {
                // Timer events are handled directly by the IRQ handler.
                // Timer events in the queue are for deferred processing.
            }
            EventType::Disk => {
                crate::trace!("events", "disk event: data={:#x}", event.data0);
            }
            EventType::Network => {
                crate::trace!("events", "network event: data={:#x}", event.data0);
            }
            EventType::Power => {
                crate::trace!("events", "power event: data={:#x}", event.data0);
            }
            EventType::Custom(id) => {
                crate::trace!("events", "custom event id={}: data={:#x}", id, event.data0);
            }
        }
    }
}
