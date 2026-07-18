// src/panic.rs
use core::panic::PanicInfo;
use crate::println; // <--- Add this line to fix the unresolved reference!

/// Custom panic handler for Zohara OS.
/// This hooks into our `println!` macro to dump the error to the serial port
/// before halting the CPU.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Print a highly visible error banner
    println!("\n[!!! KERNEL PANIC !!!]");

    // Print the panic location and message provided by Rust
    println!("{}", info);

    println!("[!!! SYSTEM HALTED !!!]\n");

    // Freeze the CPU forever using the architecture-specific halt
    loop {
        crate::arch::halt();
    }
}