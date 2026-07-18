// src/shell.rs

//! The Zohara interactive shell.
//!
//! Arch-neutral: it polls `crate::arch::try_get_key()` for input, which each
//! architecture implements (x86_64 → COM1 IRQ buffer, aarch64 → polled PL011
//! UART). The command set is shared; a handful of commands that touch
//! architecture-specific facilities (port I/O exit, canonical-address #PF test)
//! are gated with `#[cfg]`.

use alloc::string::String;
use alloc::vec::Vec;

// The print!/println! macros are #[macro_export]'d from the crate root, so a
// submodule must import them explicitly to use them by their unqualified names.
use crate::{print, println};

/// Start the interactive shell. Does not return.
pub fn start() -> ! {
    println!("=== Zohara Shell v0.5 ===");
    println!("Type 'help' and press Enter. Use Up/Down arrows for history.");
    print!("> ");

    let mut input = String::new();
    let mut history: Vec<String> = Vec::new();
    let mut history_index: usize = 0;

    loop {
        if let Some(c) = crate::arch::try_get_key() {
            match c {
                '\n' => {
                    println!();
                    if !input.is_empty() {
                        history.push(input.clone());
                        history_index = history.len();
                    }
                    let args: Vec<&str> = input.split_whitespace().collect();
                    if !args.is_empty() {
                        match args[0] {
                            "help" => {
                                println!("Available commands:");
                                println!("  help                    - Show this help message");
                                println!("  clear                   - Clear the screen");
                                println!("  about                   - About Zohara OS");
                                println!("  echo <text>             - Print text back to the screen");
                                println!("  status                  - Show system status");
                                println!("  peek <hex_addr>         - Read a byte from memory");
                                println!("  poke <hex_addr> <val>   - Write a byte to memory");
                                #[cfg(target_arch = "x86_64")]
                                println!("  exit                    - Shut down Zohara OS");
                                #[cfg(target_arch = "x86_64")]
                                println!("  crash                   - Trigger a page fault (test #PF handler)");
                                #[cfg(target_arch = "aarch64")]
                                println!("  exit                    - Shut down Zohara OS");
                                #[cfg(target_arch = "aarch64")]
                                println!("  crash                   - Trigger a data abort (test exception handler)");
                            }
                            "clear" => { for _ in 0..50 { println!(); } }
                            "about" => { println!("Zohara OS - Built with Rust. Dual-Architecture Kernel."); }
                            "echo" => {
                                if args.len() > 1 { println!("{}", args[1..].join(" ")); } else { println!(); }
                            }
                            "status" => {
                                println!("System Status:");
                                println!("  OS: Zohara v0.5");
                                #[cfg(target_arch = "x86_64")]
                                println!("  Arch: x86_64");
                                #[cfg(target_arch = "aarch64")]
                                println!("  Arch: aarch64");
                                println!("  Heap: 64 KB (Fixed-Block Allocator)");
                                #[cfg(target_arch = "x86_64")]
                                {
                                    println!("  Interrupts: Enabled (Timer + Serial)");
                                    println!("  Scheduler: Preemptive (2 Tasks)");
                                }
                                #[cfg(target_arch = "aarch64")]
                                {
                                    println!("  Interrupts: Exceptions only (no IRQs)");
                                    println!("  Scheduler: None (single-task)");
                                }
                                println!("  Paging: 4 KiB pages, identity-mapped");
                            }
                            "peek" => {
                                if args.len() == 2 {
                                    if let Ok(addr) = u64::from_str_radix(args[1].trim_start_matches("0x"), 16) {
                                        let val = unsafe { *(addr as *const u8) };
                                        println!("Memory [0x{:X}] = 0x{:02X}", addr, val);
                                    } else { println!("Invalid address format."); }
                                } else { println!("Usage: peek <hex_address>"); }
                            }
                            "poke" => {
                                if args.len() == 3 {
                                    let addr_res = u64::from_str_radix(args[1].trim_start_matches("0x"), 16);
                                    let val_res = u8::from_str_radix(args[2].trim_start_matches("0x"), 16);
                                    if let (Ok(addr), Ok(val)) = (addr_res, val_res) {
                                        unsafe { *(addr as *mut u8) = val; }
                                        println!("Wrote 0x{:02X} to Memory [0x{:X}]", val, addr);
                                    } else { println!("Invalid arguments."); }
                                } else { println!("Usage: poke <hex_address> <hex_value>"); }
                            }
                            #[cfg(target_arch = "x86_64")]
                            "exit" => {
                                println!("Shutting down Zohara OS...");
                                unsafe { core::arch::asm!("out dx, al", in("dx") 0xF4u16, in("al") 0x01u8); }
                                loop { unsafe { core::arch::asm!("hlt"); } }
                            }
                            #[cfg(target_arch = "x86_64")]
                            "crash" => {
                                // Verification: deliberately read from a
                                // canonical but unmapped address (0x80000000
                                // = 2 GB + 1 page, just past our 2 GB map).
                                // Must be canonical (bits 48-63 = bit 47)
                                // or the CPU raises GPF, not #PF.
                                println!("Deliberately reading unmapped address 0x80000000...");
                                let bad_ptr = 0x8000_0000 as *const u8;
                                let _val = unsafe { bad_ptr.read_volatile() };
                                println!("If you see this, the page fault handler failed!");
                            }
                            #[cfg(target_arch = "aarch64")]
                            "exit" => {
                                // PSCI SYSTEM_OFF (function ID 0x84000008) via HVC #0.
                                // QEMU's virt machine intercepts this at EL2 and
                                // exits cleanly. Using HVC (not SMC) because the
                                // default virt has secure=off (no EL3 firmware).
                                // Ref: ARM DEN0022, QEMU virt documentation.
                                println!("Shutting down Zohara OS (PSCI SYSTEM_OFF)...");
                                unsafe {
                                    core::arch::asm!("hvc #0", in("x0") 0x8400_0008u64);
                                }
                                // If QEMU didn't exit, halt the CPU.
                                crate::arch::halt();
                            }
                            #[cfg(target_arch = "aarch64")]
                            "crash" => {
                                // Verification: deliberately read from an unmapped
                                // address (0x8000_0000_0000, well above our 2 GB
                                // identity map). ARM64 has no canonical-address
                                // restriction like x86_64, so this cleanly produces
                                // a Data Abort (ESR EC 0x24/0x25) through our
                                // exception handler — no GPF gotcha.
                                println!("Deliberately reading unmapped address 0x800000000000...");
                                let bad_ptr = 0x8000_0000_0000u64 as *const u8;
                                let _val = unsafe { bad_ptr.read_volatile() };
                                println!("If you see this, the exception handler failed!");
                            }
                            _ => { println!("Unknown command: {}", args[0]); }
                        }
                    }
                    input.clear();
                    print!("> ");
                }
                '\u{8}' => {
                    if !input.is_empty() {
                        input.pop();
                        print!("\u{8} \u{8}");
                    }
                }
                '\u{1000}' => {
                    if !history.is_empty() && history_index > 0 {
                        history_index -= 1;
                        while !input.is_empty() { input.pop(); print!("\u{8} \u{8}"); }
                        input = history[history_index].clone();
                        print!("{}", input);
                    }
                }
                '\u{1001}' => {
                    if !history.is_empty() && history_index < history.len() - 1 {
                        history_index += 1;
                        while !input.is_empty() { input.pop(); print!("\u{8} \u{8}"); }
                        input = history[history_index].clone();
                        print!("{}", input);
                    } else if history_index == history.len() - 1 {
                        history_index = history.len();
                        while !input.is_empty() { input.pop(); print!("\u{8} \u{8}"); }
                    }
                }
                c if c >= 0x20 as char && c <= 0x7E as char => {
                    print!("{}", c);
                    input.push(c);
                }
                _ => {}
            }
        }
        // Arch-neutral idle: x86_64 `hlt` waits for the next interrupt; on
        // aarch64 (no IRQ-driven input yet) we spin so polled UART input is
        // still serviced.
        #[cfg(target_arch = "x86_64")]
        unsafe { core::arch::asm!("hlt"); }
        #[cfg(target_arch = "aarch64")]
        core::hint::spin_loop();
    }
}
