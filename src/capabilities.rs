// src/capabilities.rs

//! Kernel capability system — fine-grained permissions beyond UID.
//!
//! Each task can hold a set of capability bits. A syscall checks
//! whether the caller has the required capability, not just UID == 0.
//!
//! Capabilities:
//!   CAP_MEMORY    — manipulate other tasks' memory
//!   CAP_NETWORK   — use network interfaces
//!   CAP_FILESYSTEM — mount, unmount, create/delete files
//!   CAP_DRIVER    — register/unregister drivers
//!   CAP_DEBUG     — debug other tasks, read kernel memory
//!   CAP_POWER     — shutdown, reboot
//!   CAP_MODULE    — load/unload kernel modules

/// Capability bit positions.
pub const CAP_MEMORY:    u32 = 1 << 0;
pub const CAP_NETWORK:   u32 = 1 << 1;
pub const CAP_FILESYSTEM: u32 = 1 << 2;
pub const CAP_DRIVER:    u32 = 1 << 3;
pub const CAP_DEBUG:     u32 = 1 << 4;
pub const CAP_POWER:     u32 = 1 << 5;
pub const CAP_MODULE:    u32 = 1 << 6;

/// All capabilities (for reference / debug printing).
pub const ALL_CAPS: u32 = CAP_MEMORY | CAP_NETWORK | CAP_FILESYSTEM
    | CAP_DRIVER | CAP_DEBUG | CAP_POWER | CAP_MODULE;

/// Kernel tasks (UID 0) have all capabilities by default.
pub const KERNEL_CAPS: u32 = ALL_CAPS;

/// Check if a capability set includes a specific capability.
pub fn has_cap(caps: u32, cap: u32) -> bool {
    (caps & cap) != 0
}

/// Format capabilities as a human-readable string.
pub fn caps_to_str(caps: u32) -> &'static str {
    if caps == ALL_CAPS { return "ALL"; }
    if caps == 0 { return "none"; }
    // For simplicity, just show the hex value.
    // Full string formatting would need a buffer.
    "partial"
}
