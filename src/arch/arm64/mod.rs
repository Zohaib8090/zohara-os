// src/arch/arm64/mod.rs

use core::arch::global_asm;

global_asm!(include_str!("boot.S"));
global_asm!(include_str!("exception.S"));

pub mod paging;
pub mod syscall;
pub mod gic;
pub mod timer;

pub fn halt() -> ! {
    unsafe {
        loop {
            core::arch::asm!("wfi");
        }
    }
}

pub fn write_serial(byte: u8) {
    crate::uart::Uart.write_byte(byte);
}

pub fn try_get_key() -> Option<char> {
    use crate::uart::Uart;
    let byte = Uart.read_byte()?;
    if byte == 0x1B {
        let c1 = Uart.read_byte();
        let c2 = Uart.read_byte();
        if c1 == Some(0x5B) {
            match c2 {
                Some(0x41) => return Some('\u{1000}'),
                Some(0x42) => return Some('\u{1001}'),
                Some(0x43) => return Some('\u{1002}'),
                Some(0x44) => return Some('\u{1003}'),
                _ => return None,
            }
        }
        return None;
    }
    if byte == b'\r' { return Some('\n'); }
    if byte == 0x08 || byte == 0x7F { return Some('\u{8}'); }
    if byte >= 0x20 && byte <= 0x7E { return Some(byte as char); }
    None
}

// --- Kernel-mode exception handler (halt) ---

#[no_mangle]
pub extern "C" fn handle_exception(_stack_frame: *mut u64) {
    let esr: u64;
    unsafe { core::arch::asm!("mrs {}, esr_el1", out(reg) esr); }
    let far: u64;
    unsafe { core::arch::asm!("mrs {}, far_el1", out(reg) far); }
    let elr: u64;
    unsafe { core::arch::asm!("mrs {}, elr_el1", out(reg) elr); }

    crate::println!("\n[!!! CPU EXCEPTION !!!]");
    crate::println!("ESR_EL1: 0x{:016X} (Exception Syndrome)", esr);
    crate::println!("FAR_EL1: 0x{:016X} (Faulting Address)", far);
    crate::println!("ELR_EL1: 0x{:016X} (Faulting PC)", elr);

    let ec = (esr >> 26) & 0x3F;
    crate::println!("Exception Class: 0x{:02X}", ec);
    match ec {
        0x15 => crate::println!("Cause: SVC (Supervisor Call) from AArch64"),
        0x20 | 0x21 => crate::println!("Cause: Instruction Abort"),
        0x24 | 0x25 => crate::println!("Cause: Data Abort (Page Fault / Invalid Memory)"),
        0x22 | 0x23 => crate::println!("Cause: PC Alignment Fault"),
        0x26 | 0x27 => crate::println!("Cause: SP Alignment Fault"),
        _ => crate::println!("Cause: Unknown"),
    }
    crate::println!("[!!! SYSTEM HALTED !!!]");
    halt();
}

// --- User-mode exception handler (kill task) ---

/// Called from user_sync_handler in exception.S when a non-SVC exception
/// occurs from EL0. Kills the offending task instead of halting the kernel.
#[no_mangle]
pub extern "C" fn handle_user_exception(_stack_frame: *mut u64) {
    let esr: u64;
    unsafe { core::arch::asm!("mrs {}, esr_el1", out(reg) esr); }
    let far: u64;
    unsafe { core::arch::asm!("mrs {}, far_el1", out(reg) far); }
    let elr: u64;
    unsafe { core::arch::asm!("mrs {}, elr_el1", out(reg) elr); }

    let task_idx = crate::task::current_task();
    let ec = (esr >> 26) & 0x3F;

    crate::println!(
        "[Task {}] Exception from EL0: ESR=0x{:016X} FAR=0x{:016X} ELR=0x{:016X} EC=0x{:02X}",
        task_idx, esr, far, elr, ec
    );

    match ec {
        0x15 => crate::println!("  Cause: SVC — should not reach user_sync_handler"),
        0x20 | 0x21 => crate::println!("  Cause: Instruction Abort from EL0"),
        0x24 | 0x25 => crate::println!("  Cause: Data Abort from EL0 (page fault)"),
        0x00 => crate::println!("  Cause: Unknown reason from EL0"),
        _ => crate::println!("  Cause: EC=0x{:02X} from EL0", ec),
    }

    crate::println!("  Killing task {}.", task_idx);
    unsafe { crate::task::exit_current_task(); }
}

pub fn init_exceptions() {
    unsafe {
        core::arch::asm!(
            "ldr x0, =exception_vector_base",
            "msr vbar_el1, x0",
            "isb",
            out("x0") _,
        );
    }
}

#[inline]
pub fn enable_interrupts() {
    unsafe { core::arch::asm!("msr daifclr, #0xf"); }
}

// --- asm-callable bridges ---

#[no_mangle]
pub extern "C" fn gic_acknowledge() {
    let irq_id = gic::acknowledge();
    gic::end_of_interrupt(irq_id);
}

#[no_mangle]
pub extern "C" fn timer_handle_irq() {
    timer::handle_irq();
}
