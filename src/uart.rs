// src/uart.rs

use core::fmt;

// The PL011 UART register map offset constants
const UART0_DR: *mut u32 = 0x09000000 as *mut u32; // Data Register
const UART0_FR: *const u32 = (0x09000000 + 0x018) as *const u32; // Flag Register

// Flag Register Bit Masks
const RXFE: u32 = 1 << 4; // Receive FIFO Empty
const TXFF: u32 = 1 << 5; // Transmit FIFO Full

pub struct Uart;

impl Uart {
    /// Write a single byte to the UART interface
    pub fn write_byte(&self, byte: u8) {
        // Wait until the transmit FIFO has space
        unsafe {
            while (core::ptr::read_volatile(UART0_FR) & TXFF) != 0 {
                core::hint::spin_loop();
            }
            // Write the byte to the data register
            core::ptr::write_volatile(UART0_DR, byte as u32);
        }
    }

    /// Read a single byte from the UART interface if one is available.
    ///
    /// Polls the PL011 Flag Register: bit 4 (RXFE) is 1 when the receive
    /// FIFO is empty. Returns `None` when no key has been pressed.
    pub fn read_byte(&self) -> Option<u8> {
        unsafe {
            if (core::ptr::read_volatile(UART0_FR) & RXFE) != 0 {
                return None;
            }
            // The lower 8 bits of the Data Register hold the received byte.
            Some(core::ptr::read_volatile(UART0_DR) as u8)
        }
    }
}

// Implement the core::fmt::Write trait so we can use formatted writing
impl fmt::Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            // PL011 expects '\r\n' for a clean new line on terminals
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}