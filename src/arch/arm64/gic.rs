// src/arch/arm64/gic.rs

//! Minimal GICv2 (Generic Interrupt Controller v2) driver for the QEMU
//! `virt` machine.
//!
//! The QEMU virt machine maps the GICv2 at fixed addresses (confirmed via the
//! device-tree blob):
//!   - GICD (Distributor):        0x0800_0000
//!   - GICC (CPU Interface):      0x0801_0000
//!
//! We only need the timer to preempt the scheduler, so this driver enables
//! exactly one interrupt source: the physical non-secure generic timer
//! (PPI 30). Everything else is masked.

use core::ptr::{read_volatile, write_volatile};

// --- Memory-mapped base addresses (QEMU virt, GICv2) ----------------------

const GICD_BASE: usize = 0x0800_0000;
const GICC_BASE: usize = 0x0801_0000;

// --- GICD (Distributor) register offsets ----------------------------------

const GICD_CTLR: usize = GICD_BASE + 0x000;
const GICD_ICENABLER0: usize = GICD_BASE + 0x180; // clear-enable for SGIs+PPIs
const GICD_ISENABLER0: usize = GICD_BASE + 0x100; // set-enable for SGIs+PPIs
const GICD_ICPENDR0: usize = GICD_BASE + 0x280; // clear-pending for SGIs+PPIs
const GICD_IPRIORITYR: usize = GICD_BASE + 0x400; // priority (1 byte per IRQ)
const GICD_ITARGETSR: usize = GICD_BASE + 0x800; // target core (1 byte per IRQ)
const GICD_ICFGR: usize = GICD_BASE + 0xC00; // config (2 bits per IRQ)

// --- GICC (CPU Interface) register offsets --------------------------------

const GICC_CTLR: usize = GICC_BASE + 0x000;
const GICC_PMR: usize = GICC_BASE + 0x004; // priority mask
const GICC_IAR: usize = GICC_BASE + 0x00C; // interrupt acknowledge
const GICC_EOIR: usize = GICC_BASE + 0x010; // end of interrupt

/// The physical non-secure generic timer. PPIs occupy IRQ IDs 16..31, so PPI
/// 30 (the non-secure physical timer) is IRQ ID 30.
pub const TIMER_IRQ: u32 = 30;

#[inline]
unsafe fn reg_write(addr: usize, value: u32) {
    write_volatile(addr as *mut u32, value);
}

#[inline]
unsafe fn reg_read(addr: usize) -> u32 {
    read_volatile(addr as *const u32)
}

#[inline]
unsafe fn reg_write_byte(addr: usize, value: u8) {
    write_volatile(addr as *mut u8, value);
}

/// Bring up the GICv2: enable the distributor and CPU interface, then
/// configure the timer PPI 30 for core 0.
///
/// Called once during boot, before enabling CPU interrupts.
pub fn init() {
    unsafe {
        // Disable the distributor while we configure it.
        reg_write(GICD_CTLR, 0);

        // Disable ALL SGIs + PPIs (IDs 0..31) before selectively enabling.
        reg_write(GICD_ICENABLER0, 0xFFFF_0000);

        // Clear any pending SGIs + PPIs.
        reg_write(GICD_ICPENDR0, 0xFFFF_0000);

        // Configure PPI 30 as level-sensitive. Each IRQ has 2 config bits:
        //   0b00 = N/A, 0b01 = rising-edge, 0b10 = level
        // PPI 30 lives in GICD_ICFGR[n], where each register holds 16 IRQs
        // (32 bits / 2 bits). PPIs start at IRQ 16, so PPI 30 is bit-pair
        // (30 - 16) = 14 within the first config register (ICFGR[1] at +0x4).
        // But we indexed from ICFGR base; the register at +0x4 covers IRQs
        // 16..31. Bit-pair position = (30 - 16) * 2 = 28.
        let cfgr = reg_read(GICD_ICFGR + 0x4);
        reg_write(GICD_ICFGR + 0x4, (cfgr & !(0b11 << 28)) | (0b10 << 28));

        // Set the priority of IRQ 30 to 0 (highest). IPRIORITYR is 1 byte per
        // IRQ, so byte index = 30.
        reg_write_byte(GICD_IPRIORITYR + 30, 0x00);

        // Target IRQ 30 to core 0. ITARGETSR is 1 byte per IRQ. Core 0 mask
        // is 0b0001.
        reg_write_byte(GICD_ITARGETSR + 30, 0b0001);

        // Enable the timer PPI 30. ISENABLER0 covers IRQs 0..31; bit 30 = IRQ
        // 30.
        reg_write(GICD_ISENABLER0, 1 << 30);

        // Re-enable the distributor.
        reg_write(GICD_CTLR, 1);

        // --- CPU interface ---
        // Allow all priorities through.
        reg_write(GICC_PMR, 0xFF);
        // Enable the CPU interface.
        reg_write(GICC_CTLR, 1);
    }
}

/// Acknowledge the current interrupt. Reads GICC_IAR (which drops the IRQ's
/// priority and returns its ID). Called by the IRQ trampoline before running
/// the handler.
#[inline]
pub fn acknowledge() -> u32 {
    unsafe { reg_read(GICC_IAR) }
}

/// Signal end-of-interrupt to the GIC. Must be called exactly once per
/// acknowledged IRQ, with the same IRQ ID returned by `acknowledge()`.
#[inline]
pub fn end_of_interrupt(irq_id: u32) {
    unsafe { reg_write(GICC_EOIR, irq_id) }
}
