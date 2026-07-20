// src/config.rs

//! Boot-time kernel configuration.
//!
//! A structured config is written to a fixed physical address by the
//! bootloader / QEMU loader device.  The kernel reads it once at boot
//! and uses it to control test parameters, debug output, etc.

use core::sync::atomic::{AtomicBool, Ordering};

/// Magic value "ZOHA" — verifies the config struct is valid.
pub const CONFIG_MAGIC: u32 = 0x5A4F_4841;

/// Physical address where the config lives (set by QEMU `-device loader`).
pub const CONFIG_ADDR: usize = 0x9_0000;

/// Boot-time kernel configuration, written by the loader at a fixed address.
#[repr(C)]
pub struct KernelConfig {
    pub magic: u32,
    pub debug_enabled: u32,
    pub task_count: u32,
    pub run_verification: u32,
}

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static RUN_VERIFICATION: AtomicBool = AtomicBool::new(false);
static CONFIG_LOADED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    let raw = unsafe { &*(CONFIG_ADDR as *const KernelConfig) };
    let all_zero = raw.magic == 0 && raw.debug_enabled == 0
        && raw.task_count == 0 && raw.run_verification == 0;

    if !all_zero && raw.magic == CONFIG_MAGIC {
        DEBUG_ENABLED.store(raw.debug_enabled != 0, Ordering::Relaxed);
        RUN_VERIFICATION.store(raw.run_verification != 0, Ordering::Relaxed);
        CONFIG_LOADED.store(true, Ordering::Relaxed);
        crate::println!(
            "[Config] magic={:#X} debug={} tasks={} verify={}",
            raw.magic, raw.debug_enabled, raw.task_count, raw.run_verification,
        );
    } else {
        DEBUG_ENABLED.store(false, Ordering::Relaxed);
        RUN_VERIFICATION.store(false, Ordering::Relaxed);
        CONFIG_LOADED.store(false, Ordering::Relaxed);
        crate::println!("[Config] No valid config at {:#X} — using defaults", CONFIG_ADDR);
    }
}

pub fn task_count(default: u32) -> u32 {
    if CONFIG_LOADED.load(Ordering::Relaxed) {
        let raw = unsafe { &*(CONFIG_ADDR as *const KernelConfig) };
        raw.task_count
    } else {
        default
    }
}

pub fn is_debug() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

pub fn run_verification() -> bool {
    RUN_VERIFICATION.load(Ordering::Relaxed)
}
