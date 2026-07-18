// src/arch/arm32/mod.rs

use core::arch::global_asm;

global_asm!(include_str!("boot.S"));

pub fn halt() -> ! {
    unsafe {
        loop {
            core::arch::asm!("wfi");
        }
    }
}

// Write a single byte to the ARM Versatile Express UART (PL011)
pub fn write_serial(byte: u8) {
    unsafe {
        let uart_dr = 0x10009000 as *mut u32;
        let uart_fr = 0x10009018 as *mut u32;

        // Wait until the UART transmit FIFO is not full (TXFF bit is 5)
        while (core::ptr::read_volatile(uart_fr) & (1 << 5)) != 0 {
            core::arch::asm!("nop");
        }

        // Write the byte
        core::ptr::write_volatile(uart_dr, byte as u32);
    }
}