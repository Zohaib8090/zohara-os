// src/arch/x86_64/page_fault.rs

//! x86_64 Page Fault (#PF) handler — IDT vector 14 (0x0E).
//!
//! If the fault occurred in user mode (error_code bit 2 set), the offending
//! task is killed and the kernel continues. If the fault is in kernel mode,
//! it's a genuine bug and we halt with diagnostics.

use crate::interrupts::InterruptStackFrame;

fn decode_error_code(code: u64) -> (&'static str, &'static str, &'static str, &'static str) {
    let present = if code & (1 << 0) != 0 { "present" } else { "not-present" };
    let rw = if code & (1 << 1) != 0 { "write" } else { "read/fetch" };
    let user = if code & (1 << 2) != 0 { "user-mode" } else { "supervisor" };
    let nx = if code & (1 << 4) != 0 { "NX" } else { "" };
    (present, rw, user, nx)
}

#[no_mangle]
pub extern "x86-interrupt" fn page_fault_handler(
    _stack_frame: &mut InterruptStackFrame,
    error_code: u64,
) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2); }

    let (present, rw, user, nx) = decode_error_code(error_code);
    let task_idx = crate::task::current_task();

    if error_code & (1 << 2) != 0 {
        crate::println!("[Task {}] #PF user: VA=0x{:016X} err=0x{:X} ({}, {}, {}, {}) — killing task",
            task_idx, cr2, error_code, present, rw, user, nx);
        crate::task::exit_current_task();
        // exit_current_task loops forever; unreachable.
    } else {
        // Kernel-mode fault: genuine bug, halt.
        crate::println!("\n[!!! KERNEL PAGE FAULT !!!]");
        crate::println!("Faulting VA (CR2): 0x{:016X}", cr2);
        crate::println!("Error Code: 0x{:02X} ({}, {}, {}, {})", error_code, present, rw, user, nx);
        crate::println!("Task: {}", task_idx);
        crate::println!("[!!! SYSTEM HALTED !!!]");
        loop {
            unsafe {
                core::arch::asm!("cli");
                core::arch::asm!("hlt");
            }
        }
    }
}

pub fn init() {
    unsafe {
        crate::interrupts::set_handler_with_error_code(14, page_fault_handler);
        extern "x86-interrupt" fn gp_handler(_sf: &mut InterruptStackFrame, err: u64) {
            let task_idx = crate::task::current_task();
            if task_idx != 0 {
                crate::println!(
                    "[Task {}] #GP in Ring 3: error=0x{:X} — killing task",
                    task_idx, err
                );
                crate::task::exit_current_task();
            } else {
                crate::println!("\n[!!! KERNEL #GP !!!] Error code: 0x{:X}", err);
                // Scan the stack for a return address in kernel text range.
                // The saved RIP from the faulting instruction should be on the stack.
                let rbp: u64;
                unsafe { core::arch::asm!("mov {}, rbp", out(reg) rbp); }
                crate::println!("  RBP: 0x{:016X}", rbp);
                crate::println!("  Free frames: {}", crate::frame::free_frame_count());
                crate::println!("  Active tasks: {}", crate::task::active_task_count());
                // Dump some stack values to find the faulting address.
                for i in 0..16u64 {
                    let val = unsafe { *((rbp + i * 8) as *const u64) };
                    if val >= 0x200000 && val < 0x400000 {
                        crate::println!("  stack[rbp+{:2}] = 0x{:016X}  <-- possible RIP", i * 8, val);
                    }
                }
                loop { unsafe { core::arch::asm!("cli"); core::arch::asm!("hlt"); } }
            }
        }
        crate::interrupts::set_handler_with_error_code(13, gp_handler);
    }
}
