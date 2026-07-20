// src/logging.rs

//! Kernel logging infrastructure.
//!
//! Provides structured log macros with timestamps, PID, subsystem names,
//! and compile-time log level filtering. All output goes through the
//! dmesg ring buffer and serial port.

/// Log levels — higher numeric value = more severe.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info  = 2,
    Warn  = 3,
    Error = 4,
    Fatal = 5,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info  => "INFO ",
            Level::Warn  => "WARN ",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }
}

/// Compile-time maximum log level.
/// Debug builds: all levels enabled.
/// Release builds: Warn + Error + Fatal only.
#[cfg(debug_assertions)]
pub const MAX_LEVEL: Level = Level::Trace;
#[cfg(not(debug_assertions))]
pub const MAX_LEVEL: Level = Level::Warn;

/// Core logging function. Writes a structured, timestamped line to
/// serial + dmesg ring buffer.
///
/// Format: `[tick][PID ][LEVEL][subsys] message\n`
pub fn log(level: Level, subsystem: &str, args: core::fmt::Arguments) {
    if level < MAX_LEVEL {
        return;
    }

    let tick = crate::timer::ticks();
    let pid = crate::task::current_task();

    // Write structured prefix
    crate::print!("[{:>8}][T{:>2}][{}][{}] ", tick, pid, level.as_str(), subsystem);
    // Write the user's message
    crate::_print(args);
    crate::print!("\n");
}

/// Log at a specific level with a subsystem tag.
#[macro_export]
macro_rules! log {
    ($level:expr, $subsys:expr, $($arg:tt)*) => {
        $crate::logging::log($level, $subsys, format_args!($($arg)*));
    };
}

/// Convenience macros — one per level.
#[macro_export]
macro_rules! trace      { ($subsys:expr, $($arg:tt)*) => { $crate::log!($crate::logging::Level::Trace, $subsys, $($arg)*) }; }
#[macro_export]
macro_rules! trace_debug{ ($subsys:expr, $($arg:tt)*) => { $crate::log!($crate::logging::Level::Debug, $subsys, $($arg)*) }; }
#[macro_export]
macro_rules! info       { ($subsys:expr, $($arg:tt)*) => { $crate::log!($crate::logging::Level::Info,  $subsys, $($arg)*) }; }
#[macro_export]
macro_rules! warn       { ($subsys:expr, $($arg:tt)*) => { $crate::log!($crate::logging::Level::Warn,  $subsys, $($arg)*) }; }
#[macro_export]
macro_rules! error      { ($subsys:expr, $($arg:tt)*) => { $crate::log!($crate::logging::Level::Error, $subsys, $($arg)*) }; }
#[macro_export]
macro_rules! fatal      { ($subsys:expr, $($arg:tt)*) => { $crate::log!($crate::logging::Level::Fatal, $subsys, $($arg)*) }; }
