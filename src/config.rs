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
///
/// Layout is `#[repr(C)]` to match what QEMU writes byte-by-byte.
#[repr(C)]
pub struct KernelConfig {
    /// Must equal `CONFIG_MAGIC` for the config to be trusted.
    pub magic: u32,
    /// 0 = quiet, 1 = verbose debug output.
    pub debug_enabled: u32,
    /// Number of dynamic test tasks to spawn (0 = none).
    pub task_count: u32,
    /// 0 = normal boot, 1 = run scaling test suite, etc.
    pub test_mode: u32,
}

// Global config state, populated once at boot.
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static CONFIG_LOADED: AtomicBool = AtomicBool::new(false);

/// Read and initialize the global config. Call once at boot.
pub fn init() {
    let raw = unsafe { &*(CONFIG_ADDR as *const KernelConfig) };

    // If all fields are zero, no loader device wrote a config — use defaults.
    let all_zero = raw.magic == 0 && raw.debug_enabled == 0
        && raw.task_count == 0 && raw.test_mode == 0;

    if !all_zero && raw.magic == CONFIG_MAGIC {
        DEBUG_ENABLED.store(raw.debug_enabled != 0, Ordering::Relaxed);
        CONFIG_LOADED.store(true, Ordering::Relaxed);
        crate::println!(
            "[Config] magic={:#X} debug={} tasks={} mode={}",
            raw.magic, raw.debug_enabled, raw.task_count, raw.test_mode,
        );
    } else {
        DEBUG_ENABLED.store(false, Ordering::Relaxed);
        CONFIG_LOADED.store(false, Ordering::Relaxed);
        crate::println!("[Config] No valid config at {:#X} — using defaults", CONFIG_ADDR);
    }
}

/// Return the task count from the loaded config, or the given default.
pub fn task_count(default: u32) -> u32 {
    if CONFIG_LOADED.load(Ordering::Relaxed) {
        let raw = unsafe { &*(CONFIG_ADDR as *const KernelConfig) };
        raw.task_count
    } else {
        default
    }
}

/// Whether verbose debug logging is enabled.
pub fn is_debug() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}
