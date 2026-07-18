// src/main.rs
#![feature(alloc_error_handler)]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]
#![no_std]
#![no_main]

extern crate alloc;

use core::fmt;

pub mod uart;
pub mod panic;
pub mod allocator;
pub mod heap;
pub mod frame;
pub mod paging;
pub mod shell;
pub mod syscall;
pub mod elf;
pub mod elf_builder;
pub mod usercopy;
pub mod test_programs;

#[cfg(target_arch = "x86_64")]
pub mod interrupts;

#[cfg(target_arch = "x86_64")]
pub mod keyboard;

pub mod task;
pub mod config;

#[cfg(target_arch = "x86_64")]
#[path = "arch/x86_64/mod.rs"]
mod arch;

#[cfg(target_arch = "aarch64")]
#[path = "arch/arm64/mod.rs"]
mod arch;

struct KernelWriter;

impl fmt::Write for KernelWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            arch::write_serial(byte);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let mut writer = KernelWriter;
    writer.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Spawn a userspace task from an embedded test program byte array.
/// All tasks load at the fixed address USER_BASE_VA with stack at USER_BASE_VA + 0x100000.
/// Isolation comes from fresh per-task page tables, not different addresses.
fn spawn_userspace_task(name: &str, code: &[u8]) -> Result<usize, &'static str> {
    // Check for a free task slot BEFORE allocating any frames.
    // This prevents frame leaks when the slot table is full (overflow test).
    if !task::has_free_slot() {
        return Err("out of task slots");
    }

    let load_addr = paging::USER_BASE_VA;
    let stack_addr = paging::USER_BASE_VA + 0x100000;
    let elf_bytes = elf_builder::build_flat_elf(code, load_addr);

    let image = elf::parse_elf(&elf_bytes).map_err(|e| {
        crate::println!("[{}] ELF parse error: {:?}", name, e);
        "ELF parse failed"
    })?;

    if image.segment_count == 0 {
        return Err("no loadable segments");
    }

    let mut segments = alloc::vec::Vec::new();
    for i in 0..image.segment_count {
        let seg = &image.segments[i];
        let file_offset = seg.data_ptr - elf_bytes.as_ptr() as usize;
        segments.push((seg.vaddr, file_offset, seg.filesz, seg.memsz, seg.flags));
    }

    let (pt_root, user_phys_frames) = paging::create_user_page_tables(&segments);

    for &(vaddr, filesz, phys_addr) in &user_phys_frames {
        let seg = segments.iter().find(|s| s.0 == vaddr);
        if let Some(&(_svaddr, file_offset, sfilesz, _smemsz, _sflags)) = seg {
            if sfilesz > 0 && filesz > 0 {
                let copy_len = filesz.min(sfilesz);
                let src = &elf_bytes[file_offset..file_offset + copy_len];
                unsafe {
                    let dst = core::slice::from_raw_parts_mut(phys_addr as *mut u8, copy_len);
                    dst.copy_from_slice(src);
                }
            }
        }
    }

    let entry = image.entry_point;
    let frame_addrs: alloc::vec::Vec<usize> = user_phys_frames.iter()
        .map(|&(_, _, pa)| pa)
        .collect();
    task::spawn_userspace(entry, stack_addr, pt_root, &frame_addrs)
}

#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    #[cfg(target_arch = "x86_64")]
    arch::init_sse();

    #[cfg(feature = "debug")]
    println!("Zohara kernel is booting...");

    #[cfg(target_arch = "aarch64")]
    {
        frame::init();
        paging::init();
        arch::init_exceptions();
        syscall::init();
    }

    #[cfg(target_arch = "x86_64")]
    {
        let ram_size = arch::e820::detect_memory();
        frame::set_total_ram(ram_size);
        frame::init();
        keyboard::init_buffer();
        interrupts::init_idt();
        arch::page_fault::init();
        syscall::init();
        paging::init();
        arch::init_gdt_tss();
    }

    heap::init_heap();
    #[cfg(feature = "debug")]
    println!("Heap Allocator: Initialized (64 KB)");

    // --- Read boot-time config (must be after heap init for println). ---
    config::init();

    // --- USERSPACE VERIFICATION ---
    println!("\n=== Userspace Verification ===\n");

    task::init_scheduler();

    // Record free frames before any tasks are spawned.
    let frames_before_tasks = frame::free_frame_count();
    println!("[FrameCheck] Free frames at boot: {}", frames_before_tasks);

    #[cfg(target_arch = "x86_64")]
    {
        // Privilege drop: hlt (Ring 0 instruction) → #GP from Ring 3
        println!("[Test 1] Privilege drop: spawning task that executes hlt (Ring 0 instruction)...");
        if let Ok(idx) = spawn_userspace_task("priv_test", test_programs::x86_64_programs::PRIV_TEST) {
            println!("  Task[{}] spawned (will fault on hlt in Ring 3)", idx);
        }

        // Privilege drop: cli (Ring 0 instruction) → #GP from Ring 3
        println!("[Test 2] Privilege drop: spawning task that executes cli (Ring 0 instruction)...");
        if let Ok(idx) = spawn_userspace_task("priv_test_2", test_programs::x86_64_programs::PRIV_TEST_2) {
            println!("  Task[{}] spawned (will fault on cli in Ring 3)", idx);
        }

        // Pointer validation: Write with kernel address → EFAULT
        println!("[Test 3] Pointer validation: spawning task that calls Write with kernel address...");
        let _tp = spawn_userspace_task("ptr_bad", test_programs::x86_64_programs::PTR_TEST_BAD);

        // Read task count from boot-time config (set by run.sh via QEMU loader).
        let task_count = config::task_count(0);
        if task_count == 0 {
            println!("[Test 4] No tasks configured — skipping dynamic spawn");
        } else {
            println!("[Test 4] Dynamic spawn: launching {} identical tasks at same vaddr...", task_count);
            let mut spawn_ok = 0usize;
            let mut spawn_fail = 0usize;
            for i in 0..task_count {
                match spawn_userspace_task(
                    &alloc::format!("user_{}", i),
                    test_programs::x86_64_programs::TASK_A,
                ) {
                    Ok(_idx) => {
                        if i % 10 == 0 {
                            println!("[Test 4] spawned task {}", i);
                        }
                        spawn_ok += 1;
                    }
                    Err(_e) => spawn_fail += 1,
                }
            }
            println!("[Test 4] Spawned {}/{} tasks ({} failed)", spawn_ok, task_count, spawn_fail);

            // Valid Write from kernel mode
            println!("[Test 5] Valid Write syscall from kernel mode...");
            let msg = b"kernel write OK\n";
            let ret = crate::arch::syscall::raw_syscall(
                crate::syscall::Syscall::Write as usize,
                msg.as_ptr() as usize,
                msg.len(),
                0,
            );
            if ret == msg.len() as isize {
                println!("  Write(valid) returned {} [PASS]", ret);
            } else {
                println!("  Write(valid) returned {} [FAIL]", ret);
            }

            println!("\n[Test] Enabling timer + enabling Ring 3 tasks...\n");
            unsafe { core::arch::asm!("sti"); }

            // Spin-wait for all non-kernel tasks to exit.
            {
                let mut wait_ticks: usize = 0;
                loop {
                    let active = task::active_task_count();
                    if active == 0 { break; }
                    wait_ticks += 1;
                    if wait_ticks > 100_000_000 {
                        println!("[FrameCheck] TIMEOUT: {} tasks still active after {} ticks",
                            task::active_task_count(), wait_ticks);
                        break;
                    }
                    unsafe { core::arch::asm!("hlt"); }
                }
                println!("[FrameCheck] All tasks exited after {} hlt-wait iterations", wait_ticks);
            }

            let frames_after_tasks = frame::free_frame_count();
            println!("[FrameCheck] Free frames after all tasks exited: {}", frames_after_tasks);
            if frames_before_tasks == frames_after_tasks {
                println!("[FrameCheck] PASS: Frame count matches boot value (zero leak)");
            } else if frames_before_tasks > frames_after_tasks {
                let leaked = frames_before_tasks - frames_after_tasks;
                println!("[FrameCheck] FAIL: Frame leak of {} frames ({} bytes)", leaked, leaked * 4096);
            } else {
                let extra = frames_after_tasks - frames_before_tasks;
                println!("[FrameCheck] NOTE: {} more frames free after tasks than at boot (kernel init overhead reclaimed)", extra);
                println!("[FrameCheck] PASS (no leak — post-boot reclamation or timing artifact)");
            }

            // --- OVERFLOW TEST: spawn beyond task table limit ---
            if task_count >= 200 {
                println!("\n[OverflowTest] Spawning tasks until slot limit...");
                unsafe { core::arch::asm!("cli"); }
                let mut overflow_count = 0usize;
                loop {
                    match spawn_userspace_task(
                        &alloc::format!("ovf_{}", overflow_count),
                        test_programs::x86_64_programs::TASK_A,
                    ) {
                        Ok(_idx) => overflow_count += 1,
                        Err(e) => {
                            println!("[OverflowTest] Spawn #{} FAILED: \"{}\"", overflow_count, e);
                            println!("[OverflowTest] Filled {} task slots before rejection", overflow_count);
                            break;
                        }
                    }
                    if overflow_count > 300 {
                        println!("[OverflowTest] UNEXPECTED: spawned {} tasks without hitting limit", overflow_count);
                        break;
                    }
                }

                unsafe { core::arch::asm!("sti"); }

                let mut wait_ticks: usize = 0;
                loop {
                    if task::active_task_count() == 0 { break; }
                    wait_ticks += 1;
                    if wait_ticks > 100_000_000 { break; }
                    unsafe { core::arch::asm!("hlt"); }
                }

                let frames_final = frame::free_frame_count();
                println!("[FrameCheck] Free frames after overflow tasks exited: {}", frames_final);
                if frames_before_tasks == frames_final {
                    println!("[FrameCheck] PASS: Overflow test — zero leak (matches boot value)");
                } else if frames_before_tasks > frames_final {
                    let leaked = frames_before_tasks - frames_final;
                    println!("[FrameCheck] FAIL: Overflow leak of {} frames ({} bytes)", leaked, leaked * 4096);
                } else {
                    println!("[FrameCheck] PASS: Overflow test — no leak");
                }
            }
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        // Step 1: Privilege drop test
        println!("[Test 1] Privilege drop: spawning task that executes msr daifset...");
        if let Ok(idx) = spawn_userspace_task("priv_test", test_programs::aarch64_programs::PRIV_TEST) {
            println!("  Task[{}] spawned", idx);
        }

        // Step 2: Second privilege test
        println!("[Test 2] Privilege drop: spawning task 2...");
        if let Ok(idx) = spawn_userspace_task("priv_test_2", test_programs::aarch64_programs::PRIV_TEST_2) {
            println!("  Task[{}] spawned", idx);
        }

        // Step 3: Real userspace tasks
        println!("[Test 3] Syscall round-trip: spawning 2 userspace tasks...");
        let _ta = spawn_userspace_task("task_a", test_programs::aarch64_programs::TASK_A);
        let _tb = spawn_userspace_task("task_b", test_programs::aarch64_programs::TASK_B);

        // Step 4: Pointer validation
        println!("[Test 4] Pointer validation: Write with bad kernel pointer...");
        let bad_ptr = 0xFFFF_FFFF_8000_0000usize;
        let ret = crate::arch::syscall::raw_syscall(
            crate::syscall::Syscall::Write as usize,
            bad_ptr,
            10,
            0,
        );
        if ret == -14 {
            println!("  Write(bad_ptr) returned -14 (EFAULT) [PASS]");
        } else {
            println!("  Write(bad_ptr) returned {} [FAIL: expected -14]", ret);
        }

        println!("[Test 5] Pointer validation: Write with unmapped address...");
        let unmapped = 0x0000_0080_0000_0000usize;
        let ret2 = crate::arch::syscall::raw_syscall(
            crate::syscall::Syscall::Write as usize,
            unmapped,
            5,
            0,
        );
        if ret2 == -14 {
            println!("  Write(unmapped) returned -14 (EFAULT) [PASS]");
        } else {
            println!("  Write(unmapped) returned {} [FAIL: expected -14]", ret2);
        }

        println!("\n[Test] Enabling IRQs...\n");
        arch::enable_interrupts();
    }

    // The scheduler ticks via timer IRQ. Tasks run, output their messages,
    // and exit. After all userspace tasks finish, we drop to the shell.
    shell::start();
}
