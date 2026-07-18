// src/task.rs

//! Cooperative round-robin preemptive scheduler with userspace support.
//!
//! The scheduling logic is arch-agnostic: task table, state machine,
//! round-robin selection, the `timer_handler_rust` tick function. The
//! architecture-specific parts are:
//!
//! - `build_initial_frame`: constructs the first exception-return frame so
//!   `iretq` / `eret` drops into the task at the right privilege level.
//! - Context switch: on x86_64, CR3 is swapped alongside registers and the
//!   TSS RSP0 is updated so the next Ring 3 → Ring 0 transition uses the
//!   correct kernel stack.
//! - Each `Task` stores its own page table root (CR3 / TTBR0) so different
//!   tasks can have different virtual address spaces.

use core::sync::atomic::{AtomicUsize, Ordering};

pub const MAX_TASKS: usize = 256;
const STACK_SIZE: usize = 32768; // 32 KB per task
const MAX_USER_FRAMES: usize = 32; // max user data frames tracked per task

#[derive(Clone, Copy, PartialEq)]
pub enum TaskState {
    Unused,
    Ready,
    Running,
    Sleeping,
}

#[repr(C, align(16))]
pub struct Task {
    pub saved_sp: usize,
    _pad: usize,
    pub stack: [u8; STACK_SIZE],
    pub state: TaskState,
    /// Physical address of this task's top-level page table (CR3 / TTBR0).
    pub page_table_root: usize,
    /// Physical addresses of user data frames allocated for this task.
    /// These are freed on task exit to reclaim memory.
    pub user_data_frames: [usize; MAX_USER_FRAMES],
    pub user_data_frame_count: usize,
    /// Tick count at which a sleeping task should be woken.
    pub wake_tick: usize,
}

const UNUSED_TASK: Task = Task {
    saved_sp: 0,
    _pad: 0,
    stack: [0; STACK_SIZE],
    state: TaskState::Unused,
    page_table_root: 0,
    user_data_frames: [0; MAX_USER_FRAMES],
    user_data_frame_count: 0,
    wake_tick: 0,
};

static mut TASKS: [Task; MAX_TASKS] = [UNUSED_TASK; MAX_TASKS];

pub static mut CURRENT_TASK: usize = 0;
static TICKS: AtomicUsize = AtomicUsize::new(0);

pub fn init_scheduler() {
    unsafe {
        TASKS[0].state = TaskState::Running;
        // Task 0 uses the kernel's page tables — get the current CR3.
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3);
        TASKS[0].page_table_root = cr3 as usize;
    }
}

/// Return the current task index.
pub fn current_task() -> usize {
    unsafe { CURRENT_TASK }
}

/// Return a reference to the current task.
pub fn current_task_ref() -> &'static Task {
    unsafe { &TASKS[CURRENT_TASK] }
}

/// Return the current task's page table root physical address.
pub fn current_page_table_root() -> usize {
    unsafe { TASKS[CURRENT_TASK].page_table_root }
}

/// Return true if at least one unused task slot is available (indices 1..MAX_TASKS).
pub fn has_free_slot() -> bool {
    unsafe {
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Unused {
                return true;
            }
        }
        false
    }
}

/// Return the number of active non-kernel tasks (Ready + Running).
pub fn active_task_count() -> usize {
    unsafe {
        let mut count = 0;
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Ready || TASKS[i].state == TaskState::Running {
                count += 1;
            }
        }
        count
    }
}

/// PIT frequency is ~100 Hz (divisor 11932). Each tick ≈ 10 ms.
const TICKS_PER_MS: usize = 100;

/// Return the raw tick count since boot.
pub fn ticks() -> usize {
    TICKS.load(Ordering::SeqCst)
}

/// Return elapsed time since boot in milliseconds.
pub fn uptime_ms() -> usize {
    ticks() / TICKS_PER_MS
}

/// Put the current task to sleep for `duration_ms` milliseconds.
/// Called from the Sleep syscall handler.
pub fn sleep(duration_ms: usize) {
    unsafe {
        let wake = ticks() + (duration_ms * TICKS_PER_MS);
        TASKS[CURRENT_TASK].wake_tick = wake;
        TASKS[CURRENT_TASK].state = TaskState::Sleeping;
        // Halt until the next timer interrupt wakes us.
        core::arch::asm!("sti");
        loop {
            core::arch::asm!("hlt");
        }
    }
}

/// Spawn a kernel-mode task (Ring 0, same as before). Task runs `func` in
/// kernel mode — no ELF loading, no userspace.
pub fn spawn(func: extern "C" fn()) {
    unsafe {
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Unused {
                // Kernel tasks share the kernel's page table.
                let cr3: u64;
                core::arch::asm!("mov {}, cr3", out(reg) cr3);
                TASKS[i].page_table_root = cr3 as usize;
                TASKS[i].saved_sp = build_kernel_frame(i, func);
                TASKS[i].state = TaskState::Ready;
                return;
            }
        }
    }
    crate::println!("Scheduler: Failed to spawn task, out of slots!");
}

/// Spawn a userspace task that will run ELF-loaded code at Ring 3 / EL0.
///
/// `user_entry` is the ELF entry point (virtual address in the user's
/// address space). `user_sp` is the initial user stack pointer.
/// `page_table_root` is the physical address of the per-task page table
/// (built by `paging::create_user_page_tables`).
pub fn spawn_userspace(
    user_entry: usize,
    user_sp: usize,
    page_table_root: usize,
    user_data_frames: &[usize],
) -> Result<usize, &'static str> {
    unsafe {
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Unused {
                TASKS[i].page_table_root = page_table_root;
                TASKS[i].saved_sp = build_userspace_frame(i, user_entry, user_sp);
                TASKS[i].state = TaskState::Ready;
                // Record user data frames for cleanup on exit.
                let count = user_data_frames.len().min(MAX_USER_FRAMES);
                for j in 0..count {
                    TASKS[i].user_data_frames[j] = user_data_frames[j];
                }
                TASKS[i].user_data_frame_count = count;
                return Ok(i);
            }
        }
    }
    Err("out of task slots")
}

// ---- x86_64 initial frame builders ----------------------------------------

/// Build an initial IRETQ frame for a kernel-mode task (Ring 0).
#[cfg(target_arch = "x86_64")]
unsafe fn build_kernel_frame(i: usize, func: extern "C" fn()) -> usize {
    let stack_top = TASKS[i].stack.as_ptr() as usize + STACK_SIZE;
    let mut sp = stack_top;

    // IRETQ frame (Ring 0).
    sp -= 8; *(sp as *mut u64) = 0x10;                  // SS (kernel data)
    sp -= 8; *(sp as *mut u64) = stack_top as u64;      // RSP
    sp -= 8; *(sp as *mut u64) = 0x202;                 // RFLAGS (IF=1)
    sp -= 8; *(sp as *mut u64) = 0x08;                  // CS (kernel code)
    sp -= 8; *(sp as *mut u64) = func as usize as u64;  // RIP

    // GP registers (all zero).
    for _ in 0..15 {
        sp -= 8; *(sp as *mut u64) = 0;
    }

    // SSE area (256 bytes).
    sp -= 256;
    for j in 0..32 {
        *(sp as *mut u64).add(j) = 0;
    }

    sp
}

/// Build an initial IRETQ frame that drops the CPU to Ring 3 (user mode).
///
/// The frame layout is identical to what `timer_irq_handler` saves/restores:
/// SS, RSP, RFLAGS, CS, RIP + 14 GP regs + 256 bytes SSE.
///
/// For Ring 3:
///   SS = 0x23 (user data selector, DPL=3)
///   CS = 0x1B (user code selector, DPL=3)
///   RFLAGS = 0x202 (IF=1)
///   RSP = user stack pointer (top of user stack in the user's address space)
///   RIP = user entry point
#[cfg(target_arch = "x86_64")]
unsafe fn build_userspace_frame(i: usize, user_entry: usize, user_sp: usize) -> usize {
    let stack_top = TASKS[i].stack.as_ptr() as usize + STACK_SIZE;
    let mut sp = stack_top;

    // IRETQ frame — Ring 3 selectors.
    sp -= 8; *(sp as *mut u64) = 0x23;                       // SS (user data, RPL=3)
    sp -= 8; *(sp as *mut u64) = user_sp as u64;             // RSP (user stack)
    sp -= 8; *(sp as *mut u64) = 0x202;                      // RFLAGS (IF=1)
    sp -= 8; *(sp as *mut u64) = 0x1B;                       // CS (user code, RPL=3)
    sp -= 8; *(sp as *mut u64) = user_entry as u64;          // RIP (user entry)

    // GP registers — all zero.
    for _ in 0..15 {
        sp -= 8; *(sp as *mut u64) = 0;
    }

    // SSE area (256 bytes).
    sp -= 256;
    for j in 0..32 {
        *(sp as *mut u64).add(j) = 0;
    }

    sp
}

// ---- aarch64 initial frame builders ---------------------------------------

#[cfg(target_arch = "aarch64")]
unsafe fn build_kernel_frame(i: usize, func: extern "C" fn()) -> usize {
    let stack_top = TASKS[i].stack.as_ptr() as usize + STACK_SIZE;
    let mut sp = stack_top;

    // ELR_EL1: return address = task entry point.
    sp -= 8; *(sp as *mut u64) = func as usize as u64;
    // SPSR_EL1: EL1h, all interrupts unmasked.
    sp -= 8; *(sp as *mut u64) = 0x0000_0000_0000_03C5;
    // SP_EL0: leave at 0.
    sp -= 8; *(sp as *mut u64) = 0;
    // 31 GP registers x0-x30 (256 bytes), all zeroed.
    sp -= 256;
    for j in 0..32 {
        *(sp as *mut u64).add(j) = 0;
    }
    sp
}

/// Build an initial exception-return frame that drops the CPU to EL0 (user).
///
/// SPSR_EL1 is configured for EL0t: mode bits [3:0] = 0b0000 (EL0t),
/// with all exception masks unmasked (DAIF = 0).
#[cfg(target_arch = "aarch64")]
unsafe fn build_userspace_frame(i: usize, user_entry: usize, user_sp: usize) -> usize {
    let stack_top = TASKS[i].stack.as_ptr() as usize + STACK_SIZE;
    let mut sp = stack_top;

    // ELR_EL1: user entry point.
    sp -= 8; *(sp as *mut u64) = user_entry as u64;
    // SPSR_EL1: EL0t, DAIF=0 (unmasked). Mode bits [3:0]=0000, DAIF[9:6]=0000.
    sp -= 8; *(sp as *mut u64) = 0x0000_0000_0000_0000;
    // SP_EL0: user stack pointer (EL0 uses SP_EL0, not SP_EL1).
    sp -= 8; *(sp as *mut u64) = user_sp as u64;
    // 31 GP registers x0-x30 (256 bytes), all zeroed.
    sp -= 256;
    for j in 0..32 {
        *(sp as *mut u64).add(j) = 0;
    }
    sp
}

// ---- tick / context switch -----------------------------------------------

/// Timer tick entry from the architecture's IRQ handler.
///
/// Receives the saved stack pointer of the currently-running task and returns
/// the stack pointer to switch to. A context switch happens every 10 ticks.
///
/// On x86_64 this also swaps CR3 and updates TSS.RSP0 — the two new pieces
/// that make per-task address spaces actually work.
#[no_mangle]
pub extern "C" fn timer_handler_rust(saved_sp: usize) -> usize {
    let ticks = TICKS.fetch_add(1, Ordering::SeqCst);
    let current_tick = ticks + 1; // tick we just incremented to

    // Wake sleeping tasks whose wake time has arrived.
    unsafe {
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Sleeping && current_tick >= TASKS[i].wake_tick {
                TASKS[i].state = TaskState::Ready;
            }
        }
    }

    if ticks == 0 || ticks % 10 != 0 {
        return saved_sp;
    }

    unsafe {
        TASKS[CURRENT_TASK].saved_sp = saved_sp;
        if TASKS[CURRENT_TASK].state == TaskState::Running {
            TASKS[CURRENT_TASK].state = TaskState::Ready;
        }

        let start = (CURRENT_TASK + 1) % MAX_TASKS;
        let mut next_task = start;
        loop {
            if TASKS[next_task].state == TaskState::Ready {
                break;
            }
            next_task = (next_task + 1) % MAX_TASKS;
            if next_task == start {
                // No ready tasks — fall back to kernel task 0.
                next_task = 0;
                break;
            }
        }

        CURRENT_TASK = next_task;
        TASKS[next_task].state = TaskState::Running;

        // ---- THE CRITICAL LINES: swap page table on context switch ----
        // This is the highest-risk single line in the entire build. If we
        // load the wrong CR3, one task can corrupt another's memory silently.
        // We swap CR3 BEFORE returning so the assembly restore path runs
        // under the new task's page tables. This is safe because ALL page
        // tables (kernel and per-task) identity-map the kernel's .text/.bss
        // (where this code and the kernel stack live).
        #[cfg(target_arch = "x86_64")]
        {
            let new_cr3 = TASKS[next_task].page_table_root as u64;
            core::arch::asm!("mov cr3, {}", in(reg) new_cr3);

            // Update TSS.RSP0 so the next Ring 3 → Ring 0 transition
            // (int 0x80 or #PF) uses this task's kernel stack.
            // TSS is at symbol `tss64` (defined in boot.S .bss).
            // RSP0 is at TSS offset 0x04.
            extern "C" { static tss64: u8; }
            let tss_base = &tss64 as *const u8 as usize;
            let kernel_stack_top = TASKS[next_task].stack.as_ptr() as usize + STACK_SIZE;
            *((tss_base + 0x04) as *mut u64) = kernel_stack_top as u64;
        }

        #[cfg(target_arch = "aarch64")]
        {
            let new_ttbr0 = TASKS[next_task].page_table_root as u64;
            core::arch::asm!("msr ttbr0_el1, {}", in(reg) new_ttbr0);
            core::arch::asm!("dsb sy");
            core::arch::asm!("tlbi vmalle1");
            core::arch::asm!("dsb sy");
            core::arch::asm!("isb");
        }

        TASKS[next_task].saved_sp
    }
}

/// Terminate the currently running task, reclaiming all allocated memory.
pub fn exit_current_task() -> isize {
    unsafe {
        if CURRENT_TASK == 0 {
            crate::println!("[syscall] Exit: Task 0 (kernel) must not exit");
            return -1;
        }

        let task = &TASKS[CURRENT_TASK];

        // Reclaim user data frames — these are physical frames allocated
        // for the task's ELF code, data, and stack pages.
        for i in 0..task.user_data_frame_count {
            let pa = task.user_data_frames[i];
            if pa != 0 {
                crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(pa));
            }
        }

        // Reclaim the task's page tables (PML4 + intermediate tables).
        // Only free if this task has its own page table (not the kernel's).
        // TASKS[0].page_table_root holds the kernel's CR3 saved at boot.
        #[cfg(target_arch = "x86_64")]
        {
            let kernel_cr3 = TASKS[0].page_table_root;
            if task.page_table_root != kernel_cr3 && task.page_table_root != 0 {
                crate::paging::free_user_page_tables(task.page_table_root);
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // ARM64: free user page tables if they differ from kernel's.
            // The kernel L0 is stored during init_scheduler.
            if task.page_table_root != 0 {
                crate::paging::free_user_page_tables(task.page_table_root);
            }
        }

        crate::println!("Task[{}]: exited", CURRENT_TASK);
        TASKS[CURRENT_TASK].state = TaskState::Unused;

        // CRITICAL: Switch back to kernel page tables BEFORE waiting for
        // the timer to schedule us away.  The user's page tables were just
        // freed — accessing kernel BSS (TASKS[], etc.) through them would
        // read garbage and hang the scheduler.
        #[cfg(target_arch = "x86_64")]
        {
            let kernel_cr3 = TASKS[0].page_table_root as u64;
            core::arch::asm!("mov cr3, {}", in(reg) kernel_cr3);
            core::arch::asm!("sti");
            loop {
                core::arch::asm!("hlt");
            }
        }
        #[cfg(target_arch = "aarch64")]
        loop {
            core::arch::asm!("wfi");
        }
        #[cfg(target_arch = "aarch64")]
        loop {
            core::arch::asm!("wfi");
        }
    }
}
