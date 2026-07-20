// src/shell.rs

//! The Zohara interactive shell.
//!
//! Arch-neutral: it polls `crate::arch::try_get_key()` for input, which each
//! architecture implements (x86_64 -> COM1 IRQ buffer, aarch64 -> polled PL011
//! UART). The command set is shared; a handful of commands that touch
//! architecture-specific facilities (port I/O exit, canonical-address #PF test)
//! are gated with `#[cfg]`.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{print, println};

/// Clear the input line from the terminal.
fn clear_input(input: &str, cursor_pos: usize) {
    let remaining = input.len() - cursor_pos;
    for _ in 0..remaining { print!("\u{1002}"); }
    let len = input.len();
    if len > 0 {
        print!("{}", " ".repeat(len));
        for _ in 0..len { print!("\u{8}"); }
    }
}

/// Process a command line.
fn process_command(args: &[&str]) {
    match args[0] {
        "help" => {
            println!("Available commands:");
            println!("  --- System ---");
            println!("  help, clear, about, status, uptime, peek, poke, exit, crash");
            println!("  --- Debug ---");
            println!("  echo, dmesg, stats, meminfo, devices, ps, linux");
            println!("  --- Filesystem ---");
            println!("  ls, mkdir, touch, cat, rm, write, fat32, run <file.elf>");
        }
        "clear" => { for _ in 0..50 { println!(); } }
        "about" => { println!("Zohara OS - Built with Rust. Dual-Architecture Kernel."); }
        "echo" => { if args.len() > 1 { println!("{}", args[1..].join(" ")); } else { println!(); } }
        "dmesg" => { crate::dmesg::dump(); }
        "stats" => { crate::stats::dump(); }
        "meminfo" => { crate::memdebug::dump(); }
        "devices" => { crate::device::list_devices(); }
        "ps" => { crate::process::list_processes(); }
        "linux" => { crate::linux_compat::dump_stats(); }
        "uptime" => {
            let ms = crate::timer::uptime_ms();
            let secs = ms / 1000;
            println!("Uptime: {}s ({} ms)", secs, ms);
        }
        "status" => {
            println!("OS: Zohara v0.5");
            #[cfg(target_arch = "x86_64")] println!("Arch: x86_64");
            #[cfg(target_arch = "aarch64")] println!("Arch: aarch64");
        }
        "fat32" => {
            if crate::fs::fat32::is_mounted() {
                let fs = crate::fs::fat32::fs().unwrap();
                let d = crate::fs::fat32::disk();
                println!("FAT32: {} clusters, root_cl={}", fs.total_clusters(), fs.root_cluster());
                match fs.readdir(&d, fs.root_cluster()) {
                    Ok(entries) => {
                        for e in &entries {
                            if e.is_dir { println!("  {}/", e.name); }
                            else { println!("  {} ({} bytes)", e.name, e.size); }
                        }
                    }
                    Err(e) => println!("readdir error: {}", e),
                }
            } else {
                println!("FAT32 not mounted");
            }
        }
        "ls" => {
            if crate::fs::fat32::is_mounted() {
                let d = crate::fs::fat32::disk();
                let fs = crate::fs::fat32::fs().unwrap();
                match fs.readdir(&d, fs.root_cluster()) {
                    Ok(entries) => {
                        for e in &entries {
                            if e.is_dir { println!("  {}/", e.name); }
                            else { println!("  {} ({} bytes)", e.name, e.size); }
                        }
                    }
                    Err(e) => println!("ls error: {}", e),
                }
            } else {
                println!("(no filesystem mounted)");
            }
        }
        "mkdir" => {
            if args.len() < 2 { println!("Usage: mkdir <name>"); }
            else if crate::fs::fat32::is_mounted() {
                let d = crate::fs::fat32::disk();
                let fs = crate::fs::fat32::fs().unwrap();
                match fs.create_file_entry(&d, fs.root_cluster(), args[1], true) {
                    Ok(_) => println!("Created directory: {}", args[1]),
                    Err(e) => println!("mkdir failed: {}", e),
                }
            } else { println!("no filesystem mounted"); }
        }
        "touch" => {
            if args.len() < 2 { println!("Usage: touch <name>"); }
            else if crate::fs::fat32::is_mounted() {
                let d = crate::fs::fat32::disk();
                let fs = crate::fs::fat32::fs().unwrap();
                match fs.create_file_entry(&d, fs.root_cluster(), args[1], false) {
                    Ok(_) => println!("Created file: {}", args[1]),
                    Err(e) => println!("touch failed: {}", e),
                }
            } else { println!("no filesystem mounted"); }
        }
        "cat" => {
            if args.len() < 2 { println!("Usage: cat <file>"); }
            else if crate::fs::fat32::is_mounted() {
                let name = args[1];
                let d = crate::fs::fat32::disk();
                let fs = crate::fs::fat32::fs().unwrap();
                match fs.readdir(&d, fs.root_cluster()) {
                    Ok(entries) => {
                        match entries.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
                            Some(entry) if !entry.is_dir && entry.cluster >= 2 => {
                                let mut buf = [0u8; 8192];
                                match fs.read_file_data(&d, entry.cluster, entry.size, &mut buf) {
                                    Ok(n) => {
                                        let text = core::str::from_utf8(&buf[..n]).unwrap_or("<binary>");
                                        print!("{}", text);
                                        if n > 0 && buf[n-1] != b'\n' { println!(); }
                                    }
                                    Err(e) => println!("cat failed: {}", e),
                                }
                            }
                            Some(_) => println!("{}: is a directory", name),
                            None => println!("cat: {}: No such file", name),
                        }
                    }
                    Err(e) => println!("cat error: {}", e),
                }
            } else { println!("no filesystem mounted"); }
        }
        "rm" => {
            if args.len() < 2 { println!("Usage: rm <file>"); }
            else if crate::fs::fat32::is_mounted() {
                let d = crate::fs::fat32::disk();
                let fs = crate::fs::fat32::fs().unwrap();
                match fs.delete_file_entry(&d, fs.root_cluster(), args[1]) {
                    Ok(()) => println!("Removed: {}", args[1]),
                    Err(e) => println!("rm failed: {}", e),
                }
            } else { println!("no filesystem mounted"); }
        }
        "write" => {
            if args.len() < 3 { println!("Usage: write <file> <text>"); }
            else if crate::fs::fat32::is_mounted() {
                let name = args[1];
                let text = args[2..].join(" ");
                let d = crate::fs::fat32::disk();
                let fs = crate::fs::fat32::fs().unwrap();
                let existing = fs.readdir(&d, fs.root_cluster()).ok()
                    .and_then(|e| e.iter().find(|x| x.name.eq_ignore_ascii_case(name)).cloned());
                if let Some(entry) = existing {
                    match fs.write_file_data(&d, entry.cluster, 0, text.as_bytes()) {
                        Ok(n) => println!("Wrote {} bytes to {}", n, name),
                        Err(e) => println!("write failed: {}", e),
                    }
                } else {
                    match fs.create_file_entry(&d, fs.root_cluster(), name, false) {
                        Ok(cluster) => {
                            match fs.write_file_data(&d, cluster, 0, text.as_bytes()) {
                                Ok(n) => println!("Created and wrote {} bytes to {}", n, name),
                                Err(e) => println!("write failed: {}", e),
                            }
                        }
                        Err(e) => println!("write failed: {}", e),
                    }
                }
            } else { println!("no filesystem mounted"); }
        }
        "run" => {
            if args.len() < 2 { println!("Usage: run <file.elf>"); }
            else {
                let path = args[1];
                let mut path_buf = [0u8; 256];
                let pb = path.as_bytes();
                let cl = pb.len().min(255);
                path_buf[..cl].copy_from_slice(&pb[..cl]);
                path_buf[cl] = 0;
                let ret = crate::arch::syscall::raw_syscall(
                    crate::syscall::Syscall::Execve as usize,
                    path_buf.as_ptr() as usize, 0, 0,
                );
                if ret != 0 { println!("execve failed: {}", ret); }
            }
        }
        "peek" => {
            if args.len() == 2 {
                if let Ok(addr) = u64::from_str_radix(args[1].trim_start_matches("0x"), 16) {
                    let val = unsafe { *(addr as *const u8) };
                    println!("[0x{:X}] = 0x{:02X}", addr, val);
                } else { println!("Invalid address."); }
            } else { println!("Usage: peek <hex_address>"); }
        }
        "poke" => {
            if args.len() == 3 {
                if let (Ok(addr), Ok(val)) = (
                    u64::from_str_radix(args[1].trim_start_matches("0x"), 16),
                    u8::from_str_radix(args[2].trim_start_matches("0x"), 16),
                ) {
                    unsafe { *(addr as *mut u8) = val; }
                    println!("Wrote 0x{:02X} to [0x{:X}]", val, addr);
                } else { println!("Invalid args."); }
            } else { println!("Usage: poke <addr> <val>"); }
        }
        #[cfg(target_arch = "x86_64")]
        "exit" => {
            println!("Shutting down...");
            unsafe { core::arch::asm!("out dx, al", in("dx") 0xF4u16, in("al") 0x01u8); }
            loop { unsafe { core::arch::asm!("hlt"); } }
        }
        #[cfg(target_arch = "aarch64")]
        "exit" => {
            println!("Shutting down...");
            unsafe { core::arch::asm!("hvc #0", in("x0") 0x8400_0008u64); }
            crate::arch::halt();
        }
        _ => { println!("Unknown command: {}", args[0]); }
    }
}

/// Start the interactive shell. Does not return.
pub fn start() -> ! {
    println!("=== Zohara Shell v0.5 ===");
    println!("Type 'help' and press Enter. Use Up/Down arrows for history.");
    print!("> ");

    let mut input = String::new();
    let mut cursor: usize = 0;
    let mut history: Vec<String> = Vec::new();
    let mut history_index: usize = 0;

    loop {
        if let Some(c) = crate::arch::try_get_key() {
            match c {
                '\u{8}' => {
                    if cursor > 0 {
                        cursor -= 1;
                        input.remove(cursor);
                        // Erase: BS (back), Space (overwrite char), BS (back again)
                        print!("\u{8} \u{8}");
                        // Redraw the tail after the cursor
                        let tail = &input[cursor..];
                        if !tail.is_empty() {
                            print!("{}", tail);
                            // Move cursor back to end of input
                            for _ in 0..tail.len() { print!("\u{8}"); }
                        }
                    }
                }
                crate::keyboard::KEY_DELETE => {
                    if cursor < input.len() {
                        input.remove(cursor);
                        let tail = &input[cursor..];
                        if !tail.is_empty() {
                            print!("{}", tail);
                            print!(" ");
                            for _ in 0..=tail.len() { print!("\u{8}"); }
                        }
                    }
                }
                '\u{1000}' => {
                    if !history.is_empty() && history_index > 0 {
                        history_index -= 1;
                        clear_input(&input, cursor);
                        input = history[history_index].clone();
                        cursor = input.len();
                        print!("{}", input);
                    }
                }
                '\u{1001}' => {
                    if !history.is_empty() && history_index < history.len() - 1 {
                        history_index += 1;
                        clear_input(&input, cursor);
                        input = history[history_index].clone();
                        cursor = input.len();
                        print!("{}", input);
                    } else if history_index == history.len().saturating_sub(1) {
                        clear_input(&input, cursor);
                        input.clear();
                        cursor = 0;
                    }
                }
                '\u{1002}' => {
                    if cursor < input.len() { cursor += 1; print!("\u{1002}"); }
                }
                '\u{1003}' => {
                    if cursor > 0 { cursor -= 1; print!("\u{1003}"); }
                }
                crate::keyboard::KEY_HOME | crate::keyboard::KEY_CTRL_A => {
                    while cursor > 0 { cursor -= 1; print!("\u{1003}"); }
                }
                crate::keyboard::KEY_END | crate::keyboard::KEY_CTRL_E => {
                    while cursor < input.len() { cursor += 1; print!("\u{1002}"); }
                }
                crate::keyboard::KEY_CTRL_U => {
                    clear_input(&input, cursor);
                    input.clear();
                    cursor = 0;
                }
                crate::keyboard::KEY_CTRL_K => {
                    let tail_len = input.len() - cursor;
                    if tail_len > 0 {
                        print!("{}", " ".repeat(tail_len));
                        for _ in 0..tail_len { print!("\u{8}"); }
                    }
                    input.truncate(cursor);
                }
                crate::keyboard::KEY_CTRL_C => {
                    println!("^C");
                    input.clear();
                    cursor = 0;
                    print!("> ");
                }
                crate::keyboard::KEY_CTRL_L => {
                    for _ in 0..50 { println!(); }
                    print!("> {}", input);
                    let remaining = input.len() - cursor;
                    for _ in 0..remaining { print!("\u{8}"); }
                }
                crate::keyboard::KEY_TAB => {
                    let spaces = "    ";
                    input.insert_str(cursor, spaces);
                    cursor += spaces.len();
                    print!("{}", spaces);
                    let tail = &input[cursor..];
                    if !tail.is_empty() {
                        print!("{}", tail);
                        for _ in 0..tail.len() { print!("\u{8}"); }
                    }
                }
                '\n' => {
                    println!();
                    if !input.is_empty() {
                        history.push(input.clone());
                        history_index = history.len();
                    }
                    let args: Vec<&str> = input.split_whitespace().collect();
                    if !args.is_empty() {
                        process_command(&args);
                    }
                    input.clear();
                    cursor = 0;
                    print!("> ");
                }
                c if c >= 0x20 as char && c <= 0x7E as char => {
                    input.insert(cursor, c);
                    cursor += 1;
                    print!("{}", c);
                    let tail = &input[cursor..];
                    if !tail.is_empty() {
                        print!("{}", tail);
                        for _ in 0..tail.len() { print!("\u{8}"); }
                    }
                }
                _ => {}
            }
        }
        #[cfg(target_arch = "x86_64")]
        unsafe { core::arch::asm!("hlt"); }
        #[cfg(target_arch = "aarch64")]
        core::hint::spin_loop();
    }
}
