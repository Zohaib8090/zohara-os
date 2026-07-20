// src/apic.rs

//! Local APIC driver — MMIO register access, EOI, and IPI delivery.
//!
//! Based on Linux kernel's APIC initialization and Intel SDM Vol 3A.

use core::arch::asm;

/// LAPIC register offsets (from base address).
const LAPIC_ID: u32 = 0x020;
const LAPIC_VERSION: u32 = 0x030;
const LAPIC_TASK_PRIORITY: u32 = 0x080;
const LAPIC_EOI: u32 = 0x0B0;
const LAPIC_SPURIOUS: u32 = 0x0F0;  // Spurious Interrupt Vector Register — at 0x0F0, NOT 0x0D0!
const LAPIC_ICR_LOW: u32 = 0x0C0;
const LAPIC_ICR_HIGH: u32 = 0x0C4;

/// Read a 32-bit LAPIC register.
#[inline]
unsafe fn lapic_read(offset: u32) -> u32 {
    // Use hardcoded address to avoid potential static mut read issues
    let base: usize = 0xFEE00000;
    ((base + offset as usize) as *const u32).read_volatile()
}

/// Write a 32-bit LAPIC register.
#[inline]
unsafe fn lapic_write(offset: u32, val: u32) {
    let base: usize = 0xFEE00000;
    ((base + offset as usize) as *mut u32).write_volatile(val);
}

/// Read this core's LAPIC ID.
pub fn lapic_id() -> u32 {
    unsafe { lapic_read(LAPIC_ID) >> 24 }
}

/// Read LAPIC version.
pub fn lapic_version() -> u32 {
    unsafe { lapic_read(LAPIC_VERSION) }
}

/// Send End-Of-Interrupt.
pub fn send_eoi() {
    unsafe { lapic_write(LAPIC_EOI, 0); }
}

/// Initialize this core's Local APIC.
pub fn init_local_apic() {
    unsafe {
        // Check IA32_APIC_BASE MSR for x2APIC mode
        let (eax, edx): (u32, u32);
        core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, out("eax") eax, out("edx") edx);
        let apic_base_msr = ((edx as u64) << 32) | (eax as u64);
        let apic_enabled = (apic_base_msr >> 11) & 1;
        let x2apic_enabled = (apic_base_msr >> 10) & 1;
        let apic_base_addr = (apic_base_msr & 0xFFFFF000) as u32;
        crate::println!("[APIC] IA32_APIC_BASE={:#016X} base={:#x} enabled={} x2apic={}",
            apic_base_msr, apic_base_addr, apic_enabled, x2apic_enabled);
        crate::println!("[APIC] Write path: {}",
            if x2apic_enabled != 0 { "x2APIC MSR (WRMSR)" } else { "xAPIC MMIO (write_volatile)" });

        let id = lapic_read(LAPIC_ID);
        let version = lapic_read(LAPIC_VERSION);
        crate::println!("[APIC] LAPIC ID=0x{:08X} version=0x{:08X}", id, version);

        // Enable the LAPIC
        lapic_write(LAPIC_SPURIOUS, 0x1FF);

        // Verify
        let svr = lapic_read(LAPIC_SPURIOUS);
        crate::println!("[APIC] SVR after enable=0x{:08X}", svr);

        // Set task priority to 0
        lapic_write(LAPIC_TASK_PRIORITY, 0);
    }
}

/// Disable the LAPIC.
pub fn disable_local_apic() {
    unsafe {
        lapic_write(LAPIC_SPURIOUS, 0);
        lapic_write(LAPIC_TASK_PRIORITY, 0xFF);
    }
}

/// Clean 6-step INIT-SIPI-SIPI sequence with full ICR logging.
/// Per MP spec: INIT assert → INIT deassert → wait → SIPI → wait → SIPI.
pub fn send_init_sipi_clean(target_apic_id: u32) {
    unsafe {
        crate::println!("[SMP] --- 6-step INIT-SIPI-SIPI to APIC {} ---", target_apic_id);

        // Step 1: INIT assert (edge trigger, physical destination)
        let icr_high_val = (target_apic_id << 24) & 0xFF000000;
        crate::println!("[SMP] ICR_HIGH=0x{:08x} dest_id={}", icr_high_val, (icr_high_val >> 24) & 0xFF);
        lapic_write(LAPIC_ICR_HIGH, icr_high_val);
        lapic_write(LAPIC_ICR_LOW, 0x00004500);  // INIT, level=1, trigger=0(edge), dest=phys
        crate::println!("[SMP] Step1 INIT assert: wrote ICR_LOW=0x{:08X}", 0x00004500u32);
        let r = lapic_read(LAPIC_ICR_LOW);
        crate::println!("[SMP] Step1 ICR_LOW readback=0x{:08X} (delivery_status bit12={})", r, (r >> 12) & 1);
        wait_for_delivery();

        // Step 2: INIT deassert (edge trigger, physical destination)
        let icr_high_val = (target_apic_id << 24) & 0xFF000000;
        crate::println!("[SMP] ICR_HIGH=0x{:08x} dest_id={}", icr_high_val, (icr_high_val >> 24) & 0xFF);
        lapic_write(LAPIC_ICR_HIGH, icr_high_val);
        lapic_write(LAPIC_ICR_LOW, 0x00000500);  // INIT, level=0, trigger=0(edge), dest=phys
        crate::println!("[SMP] Step2 INIT deassert: wrote ICR_LOW=0x{:08X}", 0x00000500u32);
        wait_for_delivery();

        // Step 3: Wait 10ms
        crate::println!("[SMP] Step3 waiting 10ms...");
        delay_ms(10);

        // Step 4: SIPI vector=0x08
        let icr_high_val = (target_apic_id << 24) & 0xFF000000;
        crate::println!("[SMP] ICR_HIGH=0x{:08x} dest_id={}", icr_high_val, (icr_high_val >> 24) & 0xFF);
        lapic_write(LAPIC_ICR_HIGH, icr_high_val);
        lapic_write(LAPIC_ICR_LOW, 0x00004608);  // SIPI, edge, vector=0x08
        crate::println!("[SMP] Step4 SIPI vector=8: wrote ICR_LOW=0x{:08X}", 0x00004608u32);
        wait_for_delivery();

        // Step 5: Wait 200us
        delay_ms(1);

        // Step 6: Second SIPI
        let icr_high_val = (target_apic_id << 24) & 0xFF000000;
        crate::println!("[SMP] ICR_HIGH=0x{:08x} dest_id={}", icr_high_val, (icr_high_val >> 24) & 0xFF);
        lapic_write(LAPIC_ICR_HIGH, icr_high_val);
        lapic_write(LAPIC_ICR_LOW, 0x00004608);
        crate::println!("[SMP] Step6 second SIPI: wrote ICR_LOW=0x{:08X}", 0x00004608u32);
        wait_for_delivery();

        crate::println!("[SMP] --- 6-step sequence complete ---");
    }
}

/// Try to enable the AP's LAPIC by writing to its SVR via MMIO.
/// This is a QEMU TCG workaround: some implementations require the AP's
/// LAPIC to be software-enabled before SIPI delivery works.
/// The AP's LAPIC is at 0xFEE00000 (same physical address as BSP's LAPIC
/// but a separate device). We attempt to access it via the MMIO mapping.
pub fn try_enable_ap_lapic(target_apic_id: u32) {
    unsafe {
        crate::println!("[SMP] Attempting to enable AP {} LAPIC via SVR write...", target_apic_id);
        // Try writing to the AP's LAPIC SVR (0xFEE000F0)
        // In QEMU TCG, the LAPIC MMIO might be accessible per-vCPU
        // Write 0x1FF (enable + vector 0xFF) to SVR
        let svr_addr = 0xFEE000F0 as *mut u32;
        core::ptr::write_volatile(svr_addr, 0x1FF);
        let readback = core::ptr::read_volatile(svr_addr);
        crate::println!("[SMP] AP {} SVR write attempt: wrote 0x1FF, readback=0x{:08X}", target_apic_id, readback);
    }
}

/// Wait for ICR delivery to complete (poll bit 12 of ICR Low).
unsafe fn wait_for_delivery() {
    for _ in 0..1_000_000u32 {
        if lapic_read(LAPIC_ICR_LOW) & (1 << 12) == 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

/// Approximate busy-wait delay in milliseconds.
fn delay_ms(ms: u32) {
    // Calibrated for QEMU's default ~2 GHz CPU.
    // Each iteration is roughly 2-3 instructions.
    let iterations = ms as u64 * 200_000;
    for _ in 0..iterations {
        unsafe { asm!("nop"); }
    }
}
