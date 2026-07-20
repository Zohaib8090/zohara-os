// src/workqueue.rs

//! Work queue system — deferred processing from interrupt context.
//!
//! Interrupt handlers should stay minimal. They queue work items
//! that kernel worker threads process later.
//!
//! Pattern: IRQ → push work → kernel loop processes work items.

/// A work item: a function pointer + argument.
#[derive(Clone, Copy)]
pub struct WorkItem {
    pub func: fn(usize),
    pub arg: usize,
}

/// Fixed-size work queue (no heap).
const WORK_QUEUE_SIZE: usize = 64;

static mut WORK_QUEUE: [Option<WorkItem>; WORK_QUEUE_SIZE] = [None; WORK_QUEUE_SIZE];
static mut WORK_HEAD: usize = 0;
static mut WORK_TAIL: usize = 0;
static mut WORK_COUNT: usize = 0;
static WORK_LOCK: crate::spinlock::SpinLock<()> = crate::spinlock::SpinLock::new(());

/// Submit a work item for deferred processing.
pub fn submit(func: fn(usize), arg: usize) -> Result<(), ()> {
    let _guard = WORK_LOCK.lock();
    unsafe {
        if WORK_COUNT >= WORK_QUEUE_SIZE {
            return Err(());
        }
        WORK_QUEUE[WORK_HEAD] = Some(WorkItem { func, arg });
        WORK_HEAD = (WORK_HEAD + 1) % WORK_QUEUE_SIZE;
        WORK_COUNT += 1;
        Ok(())
    }
}

/// Process all pending work items (called from kernel main loop).
pub fn process_work() {
    loop {
        let item = {
            let _guard = WORK_LOCK.lock();
            unsafe {
                if WORK_COUNT == 0 { return; }
                let t = WORK_TAIL;
                let item = WORK_QUEUE[t].take();
                WORK_TAIL = (t + 1) % WORK_QUEUE_SIZE;
                WORK_COUNT -= 1;
                item
            }
        };
        if let Some(work) = item {
            (work.func)(work.arg);
        }
    }
}

/// Number of pending work items.
pub fn pending_count() -> usize {
    let _guard = WORK_LOCK.lock();
    unsafe { WORK_COUNT }
}
