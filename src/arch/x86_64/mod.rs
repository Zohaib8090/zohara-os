// src/arch/x86_64/mod.rs

use core::arch::global_asm;

// Safely pull in our boot assembly code
global_asm!(include_str!("boot.S"));

// Halt the x86 CPU safely
pub fn halt() -> ! {
    unsafe {
        core::arch::asm!("cli"); // Disable interrupts
        loop {
            core::arch::asm!("hlt"); // Halt CPU execution instruction
        }
    }
}