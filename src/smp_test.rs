// src/smp_test.rs

//! SMP SpinLock contention proof test.
//!
//! All cores (BSP + APs) simultaneously increment a shared counter
//! 1,000,000 times each, protected by a SpinLock. If the lock is
//! correct, the final value is exactly N * 1,000,000.

use crate::spinlock::SpinLock;

static SHARED_COUNTER: SpinLock<u64> = SpinLock::new(0);
static CORES_DONE: SpinLock<u64> = SpinLock::new(0);

const ITERATIONS: u64 = 1_000_000;
const NUM_CORES: u64 = 4;

/// Run by each core. Increments the shared counter 1,000,000 times.
pub fn contention_worker(core_id: usize) {
    for _ in 0..ITERATIONS {
        let mut guard = SHARED_COUNTER.lock();
        *guard += 1;
    }
    crate::println!("[SMP-TEST] core {} finished its loop", core_id);

    // Mark this core as done
    let mut done = CORES_DONE.lock();
    *done += 1;
}

/// Called by BSP after all cores are done.
/// Reads the final value and reports PASS/FAIL.
pub fn report_result() {
    // Wait until all cores (including BSP) are done
    loop {
        {
            let done = CORES_DONE.lock();
            if *done >= NUM_CORES {
                break;
            }
        }
        // Brief yield
        for _ in 0..1000u32 {
            core::hint::spin_loop();
        }
    }

    let final_value = *SHARED_COUNTER.lock();
    let expected = NUM_CORES * ITERATIONS;
    crate::println!("[SMP-TEST] FINAL COUNTER = {}", final_value);
    crate::println!("[SMP-TEST] EXPECTED = {}", expected);
    if final_value == expected {
        crate::println!("[SMP-TEST] PASS - lock is safe");
    } else {
        let lost = expected - final_value;
        crate::println!("[SMP-TEST] FAIL - lock is broken, lost {} increments", lost);
    }
}
