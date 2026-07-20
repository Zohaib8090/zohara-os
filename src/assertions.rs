// src/assertions.rs

//! Kernel assertions — zero overhead in release builds.
//!
//! `kassert!` panics in debug builds, no-op in release.
//! `krequire!` always checks (for invariants that must never be violated).
//! `kunreachable!` always panics (for truly impossible code paths).

/// Assert a condition in debug builds. No-op in release.
#[macro_export]
macro_rules! kassert {
    ($cond:expr) => {
        #[cfg(debug_assertions)]
        if !$cond {
            panic!("kassert failed: {} at {}:{}", stringify!($cond), file!(), line!());
        }
    };
    ($cond:expr, $($arg:tt)+) => {
        #[cfg(debug_assertions)]
        if !$cond {
            panic!("kassert failed: {} at {}:{} — {}", stringify!($cond), file!(), line!(), format_args!($($arg)+));
        }
    };
}

/// Always-on assertion. Checks in both debug and release builds.
/// Use for invariants that must never be violated (memory safety, etc.).
#[macro_export]
macro_rules! krequire {
    ($cond:expr) => {
        if !$cond {
            panic!("krequire failed: {} at {}:{}", stringify!($cond), file!(), line!());
        }
    };
    ($cond:expr, $($arg:tt)+) => {
        if !$cond {
            panic!("krequire failed: {} at {}:{} — {}", stringify!($cond), file!(), line!(), format_args!($($arg)+));
        }
    };
}

/// Mark a code path as unreachable. Always panics.
#[macro_export]
macro_rules! kunreachable {
    () => {
        panic!("unreachable reached at {}:{}", file!(), line!());
    };
    ($msg:expr) => {
        panic!("unreachable: {} at {}:{}", $msg, file!(), line!());
    };
}

/// Debug-only panic. Only fires in debug builds.
#[macro_export]
macro_rules! kdebug_panic {
    ($($arg:tt)+) => {
        #[cfg(debug_assertions)]
        panic!($($arg)+);
    };
}

/// Debug-only warning. Prints in debug builds, no-op in release.
#[macro_export]
macro_rules! kdebug_warn {
    ($($arg:tt)+) => {
        #[cfg(debug_assertions)]
        {
            $crate::warn!("kernel", $($arg)+);
        }
    };
}
