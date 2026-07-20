// src/panic.rs
use core::panic::PanicInfo;

/// Stack frame for backtrace (frame pointer based).
#[repr(C)]
struct StackFrame {
    next: *const StackFrame,
    return_addr: usize,
}

/// Attempt a frame-pointer-based stack backtrace.
/// Returns the number of frames dumped.
unsafe fn dump_stack_trace(max_frames: usize) -> usize {
    let mut rbp: *const StackFrame;
    core::arch::asm!("mov {}, rbp", out(reg) rbp);
    let mut count = 0;
    while !rbp.is_null() && count < max_frames {
        let frame = &*rbp;
        let ret = frame.return_addr;
        // Only print addresses in reasonable kernel/user ranges
        if ret == 0 { break; }
        crate::println!("  [{:>2}] {:#018X}", count, ret);
        rbp = frame.next;
        count += 1;
        // Sanity check: frame pointer should advance (not loop)
        if frame.next <= rbp && count > 1 { break; }
    }
    count
}

/// Enhanced panic handler with register dump, uptime, memory stats,
/// and stack backtrace.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::stats::count_panic();

    crate::println!();
    crate::println!("========================================");
    crate::println!("          KERNEL PANIC");
    crate::println!("========================================");

    // Reason
    crate::println!("Reason: {}", info);

    // Task info
    let pid = crate::task::current_task();
    let uid = crate::task::current_user_id();
    crate::println!("PID: {} (UID: {})", pid, uid);

    // Uptime
    let uptime = crate::timer::uptime_ms();
    let ticks = crate::timer::ticks();
    crate::println!("Uptime: {} ms ({} ticks)", uptime, ticks);

    // Registers
    #[cfg(target_arch = "x86_64")]
    {
        let rip: u64;
        let rsp: u64;
        let rbp: u64;
        let rax: u64;
        let rbx: u64;
        let cr2: u64;
        unsafe {
            core::arch::asm!("lea {}, [rip+0]", out(reg) rip);
            core::arch::asm!("mov {}, rsp", out(reg) rsp);
            core::arch::asm!("mov {}, rbp", out(reg) rbp);
            core::arch::asm!("mov {}, rax", out(reg) rax);
            core::arch::asm!("mov {}, rbx", out(reg) rbx);
            core::arch::asm!("mov {}, cr2", out(reg) cr2);
        }
        crate::println!("RIP: {:#018X}", rip);
        crate::println!("RSP: {:#018X}", rsp);
        crate::println!("RBP: {:#018X}", rbp);
        crate::println!("RAX: {:#018X}", rax);
        crate::println!("RBX: {:#018X}", rbx);
        crate::println!("CR2: {:#018X} (last page fault addr)", cr2);
    }

    // Memory
    let free = crate::frame::free_frame_count();
    let total = crate::frame::total_ram() / crate::frame::FRAME_SIZE;
    crate::println!("Memory: {}/{} frames free ({}/{} KiB)",
        free, total, free * 4, total * 4);

    // Statistics snapshot
    let s = &crate::stats::STATS;
    crate::println!("Stats: {} ctx_sw, {} ticks, {} syscalls, {} pf_user",
        s.context_switches.load(core::sync::atomic::Ordering::Relaxed),
        s.timer_ticks.load(core::sync::atomic::Ordering::Relaxed),
        s.syscalls_total.load(core::sync::atomic::Ordering::Relaxed),
        s.page_faults_user.load(core::sync::atomic::Ordering::Relaxed),
    );

    // Stack backtrace
    crate::println!("Stack trace:");
    unsafe {
        let frames = dump_stack_trace(16);
        if frames == 0 {
            crate::println!("  (no frames captured)");
        }
    }

    crate::println!("========================================");
    crate::println!("         SYSTEM HALTED");
    crate::println!("========================================");

    loop {
        crate::arch::halt();
    }
}
