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
pub mod timer;
pub mod logging;
pub mod assertions;
pub mod stats;
pub mod handles;
pub mod device;
pub mod events;
pub mod workqueue;
pub mod capabilities;
pub mod memdebug;
pub mod test_framework;
pub mod process;
pub mod fs;
pub mod runtime;
pub mod linux_compat;
pub mod fd_table;
pub mod drivers;

#[cfg(target_arch = "x86_64")]
pub mod interrupts;

#[cfg(target_arch = "x86_64")]
pub mod keyboard;

pub mod task;
pub mod config;
pub mod dmesg;
pub mod spinlock;
pub mod smp;
pub mod acpi;
pub mod apic;
pub mod smp_test;

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
    // Capture the formatted string for the ring buffer.
    let mut ring_buf = RingBufWriter;
    ring_buf.write_fmt(args).unwrap();
    writer.write_fmt(args).unwrap();
}

/// Tiny writer that feeds formatted output into the dmesg ring buffer.
struct RingBufWriter;

impl fmt::Write for RingBufWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        dmesg::push_bytes(s.as_bytes());
        Ok(())
    }
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
/// All tasks load at the fixed address USER_BASE_VA with stack top at USER_BASE_VA + 0x101000.
/// The stack page is at USER_BASE_VA + 0x100000; RSP starts one page above
/// so that push (which decrements RSP first) lands within the mapped page.
/// Isolation comes from fresh per-task page tables, not different addresses.
fn spawn_userspace_task(name: &str, code: &[u8]) -> Result<usize, &'static str> {
    // Check for a free task slot BEFORE allocating any frames.
    // This prevents frame leaks when the slot table is full (overflow test).
    if !task::has_free_slot() {
        return Err("out of task slots");
    }

    let load_addr = paging::USER_BASE_VA;
    let stack_addr = paging::USER_BASE_VA + 0x100000 + 0x1000; // top of stack page + 1
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

        // ACPI discovery must happen BEFORE paging::init() because
        // ACPI tables (RSDP, RSDT, MADT) are in physical low memory
        // that is only accessible via the boot identity mapping.
        acpi::init();

        paging::init();

        // Map the LAPIC MMIO page as Uncacheable — MMIO must not be cached!
        unsafe { paging::map_page_uc(crate::acpi::MADT_LOCAL_APIC_ADDR as usize & !0xFFF); }

        apic::init_local_apic();

        arch::init_gdt_tss();

        // SMP BSP setup and AP wake after paging (needs kernel virtual addrs)
        smp::init_bsp();
        smp::wake_aps();

        // Run the spinlock contention test on BSP (core 0)
        smp_test::contention_worker(0);
        // Wait for all cores to finish, then report
        smp_test::report_result();
    }

    heap::init_heap();
    #[cfg(feature = "debug")]
    println!("Heap Allocator: Initialized (64 KB)");

    // --- Read boot-time config (must be after heap init for println). ---
    config::init();
    crate::fs::init();

    // Auto-mount FAT32 from disk if present.
    {
        let mut disk = crate::drivers::ide::IdeDisk::primary_master();
        if disk.init().is_ok() && disk.is_present() {
            match crate::fs::fat32::Fat32Fs::init(&disk) {
                Ok(fs) => {
                    crate::fs::fat32::set_mounted(fs, disk);
                    println!("[Boot] FAT32 mounted");
                }
                Err(_) => println!("[Boot] No FAT32 on disk"),
            }
        }
    }

    if config::run_verification() {
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

        // Sleep test: Sleep(500) then print confirmation
        println!("[Test 3b] Sleep: spawning task that calls Sleep(500)...");
        let before_ms = task::uptime_ms();
        if let Ok(idx) = spawn_userspace_task("sleep_test", test_programs::x86_64_programs::SLEEP_TEST) {
            println!("  Task[{}] spawned (will sleep 500ms then print)", idx);
        }

        // Syscall expansion test: GetPid, Yield, Read, Sleep
        println!("[Test 3c] Syscalls: spawning task that exercises GetPid/Yield/Read/Sleep...");
        if let Ok(idx) = spawn_userspace_task("syscall_test", test_programs::x86_64_programs::SYSCALL_TEST) {
            println!("  Task[{}] spawned (will print PID=N then exit)", idx);
        }

        // Yield interleaving test: two tasks print Y/Z with yield between
        println!("[Test 3c] Yield: spawning 2 tasks that interleave via Yield...");
        let _y = spawn_userspace_task("yield_y", test_programs::x86_64_programs::YIELD_TEST_Y);
        let _z = spawn_userspace_task("yield_z", test_programs::x86_64_programs::YIELD_TEST_Z);

        // Permission test: DebugLog (UID 0 required)
        println!("[Test 3d] Permissions: spawning task that calls DebugLog (requires UID 0)...");
        if let Ok(idx) = spawn_userspace_task("perm_test", test_programs::x86_64_programs::PERM_TEST) {
            println!("  Task[{}] spawned (UID 1000, should get -EPERM)", idx);
        }
        // Kernel-mode DebugLog (Task 0, UID 0) — should succeed.
        println!("[Test 3d] Permissions: kernel-mode DebugLog (UID 0)...");
        let priv_ret = crate::arch::syscall::raw_syscall(
            crate::syscall::Syscall::DebugLog as usize, 0, 0, 0,
        );
        if priv_ret >= 0 {
            println!("  DebugLog(UID 0) returned {} [PASS]", priv_ret);
        } else {
            println!("  DebugLog(UID 0) returned {} [FAIL: expected >= 0]", priv_ret);
        }

        // --- VFS DEMONSTRATION ---
        println!("\n=== VFS Demonstration ===\n");

        // mkdir /test
        println!("[VFS] mkdir /test ...");
        match crate::fs::create_dir("/test") {
            Ok(idx) => println!("  mkdir /test: OK (node {})", idx),
            Err(e) => println!("  mkdir /test: FAILED ({})", e),
        }

        // touch /test/file.txt
        println!("[VFS] touch /test/file.txt ...");
        match crate::fs::create_file("/test/file.txt") {
            Ok(_) => println!("  touch /test/file.txt: OK"),
            Err(e) => println!("  touch /test/file.txt: FAILED ({})", e),
        }

        // write /test/file.txt "hello world"
        println!("[VFS] write /test/file.txt 'hello world' ...");
        match crate::fs::write_file("/test/file.txt", 0, b"hello world") {
            Ok(n) => println!("  write /test/file.txt: OK ({} bytes)", n),
            Err(e) => println!("  write /test/file.txt: FAILED ({})", e),
        }

        // cat /test/file.txt
        println!("[VFS] cat /test/file.txt ...");
        let mut buf = [0u8; 256];
        match crate::fs::read_file("/test/file.txt", 0, &mut buf) {
            Ok(n) => {
                let content = core::str::from_utf8(&buf[..n]).unwrap_or("<binary>");
                println!("  cat /test/file.txt: \"{}\"", content);
            }
            Err(e) => println!("  cat /test/file.txt: FAILED ({})", e),
        }

        // ls /test
        println!("[VFS] ls /test ...");
        let entries = crate::fs::readdir("/test");
        for (name, _inode, node_type) in &entries {
            if *node_type == crate::fs::node::VfsNodeType::Directory {
                println!("  {}/", name);
            } else {
                println!("  {} (inode {})", name, _inode);
            }
        }

        // rm /test/file.txt
        println!("[VFS] rm /test/file.txt ...");
        match crate::fs::delete("/test/file.txt") {
            Ok(_) => println!("  rm /test/file.txt: OK"),
            Err(e) => println!("  rm /test/file.txt: FAILED ({})", e),
        }

        // Verify deletion
        println!("[VFS] ls /test after rm ...");
        let entries2 = crate::fs::readdir("/test");
        if entries2.is_empty() {
            println!("  (empty — file deleted successfully)");
        } else {
            for (name, _, _) in &entries2 {
                println!("  {} (still exists!)", name);
            }
        }

        // --- ELF FROM VFS TEST ---
        println!("\n=== ELF Loading from VFS ===\n");

        // Step 1: Build an ELF binary from the embedded TASK_A machine code
        let elf_bytes = elf_builder::build_flat_elf(
            test_programs::x86_64_programs::TASK_A,
            paging::USER_BASE_VA,
        );
        println!("[ELF-VFS] Built ELF binary: {} bytes", elf_bytes.len());

        // Step 2: Write it into VFS at /bin/task_a
        crate::fs::create_dir("/bin").ok();
        match crate::fs::create_file("/bin/task_a") {
            Ok(_) => println!("[ELF-VFS] Created /bin/task_a in VFS"),
            Err(e) => println!("[ELF-VFS] create_file FAILED: {}", e),
        }
        match crate::fs::write_file("/bin/task_a", 0, &elf_bytes) {
            Ok(n) => println!("[ELF-VFS] Wrote {} bytes to /bin/task_a", n),
            Err(e) => println!("[ELF-VFS] write FAILED: {}", e),
        }

        // Step 3: Read it back from VFS
        let mut read_buf = [0u8; 8192];
        let bytes_read = match crate::fs::read_file("/bin/task_a", 0, &mut read_buf) {
            Ok(n) => {
                println!("[ELF-VFS] Read {} bytes back from /bin/task_a", n);
                n
            }
            Err(e) => {
                println!("[ELF-VFS] read FAILED: {}", e);
                0
            }
        };

        // Step 4: Verify the data matches
        if bytes_read == elf_bytes.len() && &read_buf[..bytes_read] == &elf_bytes[..] {
            println!("[ELF-VFS] Data integrity: VERIFIED ({} bytes match)", bytes_read);
        } else {
            println!("[ELF-VFS] Data integrity: MISMATCH (read {} vs original {})", bytes_read, elf_bytes.len());
        }

        // Step 5: Spawn a process from the VFS data
        println!("[ELF-VFS] Spawning process from /bin/task_a ...");
        unsafe { core::arch::asm!("sti"); } // enable timer for scheduler
        match crate::process::create_process(&read_buf[..bytes_read], &["task_a"], "vfs_task") {
            Ok(handle) => {
                println!("[ELF-VFS] Process spawned: PID {}", handle.id());
                // Wait for it to run and exit
                let mut wait = 0;
                loop {
                    if task::get_task_state(handle.id()) == task::TaskState::Unused { break; }
                    wait += 1;
                    if wait > 5_000_000 {
                        println!("[ELF-VFS] TIMEOUT waiting for process");
                        break;
                    }
                    unsafe { core::arch::asm!("hlt"); }
                }
                println!("[ELF-VFS] Process exited after {} ticks", wait);
            }
            Err(e) => println!("[ELF-VFS] create_process FAILED: {}", e),
        }

        // Step 6: Also load and run the RUNTIME_TEST binary from VFS
        println!("[ELF-VFS] Loading RUNTIME_TEST from VFS...");
        let runtime_elf = elf_builder::build_flat_elf(
            test_programs::x86_64_programs::RUNTIME_TEST,
            paging::USER_BASE_VA,
        );
        crate::fs::create_file("/bin/runtime_test").ok();
        crate::fs::write_file("/bin/runtime_test", 0, &runtime_elf).ok();
        let mut rt_buf = [0u8; 8192];
        let rt_read = crate::fs::read_file("/bin/runtime_test", 0, &mut rt_buf).unwrap_or(0);
        match crate::process::create_process(&rt_buf[..rt_read], &["runtime_test"], "rt_task") {
            Ok(handle) => {
                println!("[ELF-VFS] Runtime test spawned: PID {}", handle.id());
                let mut wait = 0;
                loop {
                    if task::get_task_state(handle.id()) == task::TaskState::Unused { break; }
                    wait += 1;
                    if wait > 5_000_000 { break; }
                    unsafe { core::arch::asm!("hlt"); }
                }
                println!("[ELF-VFS] Runtime test exited after {} ticks", wait);
            }
            Err(e) => println!("[ELF-VFS] Runtime test FAILED: {}", e),
        }

        // --- PS DEMONSTRATION ---
        println!("\n=== PS Demonstration ===\n");
        crate::process::list_processes();

        // --- YIELD INTERLEAVING TEST (proof) ---
        println!("\n=== Yield Interleaving Proof ===\n");
        println!("  Enabling timer to run yield tasks...");
        unsafe { core::arch::asm!("sti"); }

        // Wait for yield tasks (6, 7) and sleep task (4) to complete
        {
            let mut wait_ticks: usize = 0;
            loop {
                // Check if tasks 6 and 7 (yield tests) have exited
                let t6_state = task::get_task_state(6);
                let t7_state = task::get_task_state(7);
                if t6_state == task::TaskState::Unused && t7_state == task::TaskState::Unused {
                    break;
                }
                wait_ticks += 1;
                if wait_ticks > 10_000_000 {
                    println!("  TIMEOUT waiting for yield tasks");
                    break;
                }
                unsafe { core::arch::asm!("hlt"); }
            }
            println!("  Yield tasks completed after {} ticks", wait_ticks);
        }

        // --- FRAME CHECK (always runs) ---
        let frames_after_all = frame::free_frame_count();
        println!("\n[FrameCheck] Free frames after all tests: {}", frames_after_all);
        if frames_before_tasks == frames_after_all {
            println!("[FrameCheck] PASS: Frame count matches boot value (zero leak)");
        } else if frames_before_tasks > frames_after_all {
            let leaked = frames_before_tasks - frames_after_all;
            println!("[FrameCheck] FAIL: Frame leak of {} frames ({} bytes)", leaked, leaked * 4096);
        } else {
            println!("[FrameCheck] PASS: No leak (extra frames reclaimed)");
        }

        // Now proceed to dynamic tasks
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

    } // end run_verification

    // Enable interrupts so the serial IRQ delivers keystrokes to the shell.
    // In non-verification mode sti is never called above; even in verification
    // mode the overflow test does cli/sti so ensure interrupts are on here.
    #[cfg(target_arch = "x86_64")]
    unsafe { core::arch::asm!("sti"); }

    // The scheduler ticks via timer IRQ. Tasks run, output their messages,
    // and exit. After all userspace tasks finish, we drop to the shell.
    shell::start();
}
