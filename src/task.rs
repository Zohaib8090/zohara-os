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


pub const MAX_TASKS: usize = 256;
const STACK_SIZE: usize = 32768; // 32 KB per task
const MAX_USER_FRAMES: usize = 32; // max user data frames tracked per task
pub const MAX_FDS: usize = 32; // max file descriptors per task

// Re-export timer APIs so existing callers (main.rs, etc.) still work.
pub use crate::timer::{ticks, uptime_ms};

/// Get a task's state by index (for syscall_info, etc.).
pub fn get_task_state(idx: usize) -> TaskState {
    unsafe { TASKS[idx].state }
}

/// Get a task's user_id by index.
pub fn get_task_uid(idx: usize) -> u32 {
    unsafe { TASKS[idx].user_id }
}

#[derive(Clone, Copy, PartialEq, Debug)]
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
    /// User ID. 0 = kernel/privileged, 1000 = normal user.
    pub user_id: u32,
    // fd table fields temporarily removed for debugging — will re-add after page fault is resolved
    // pub fd_nodes: [i32; MAX_FDS],
    // pub fd_offsets: [usize; MAX_FDS],
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
    user_id: 1000,
    // fd_nodes: [-1; MAX_FDS],
    // fd_offsets: [0; MAX_FDS],
};

static mut TASKS: [Task; MAX_TASKS] = [UNUSED_TASK; MAX_TASKS];

pub static mut CURRENT_TASK: usize = 0;

pub fn init_scheduler() {
    unsafe {
        TASKS[0].state = TaskState::Running;
        TASKS[0].user_id = 0; // kernel/privileged
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

/// Return the page table root for a specific task by index.
pub fn task_page_table_root(idx: usize) -> usize {
    unsafe { TASKS[idx].page_table_root }
}

/// Return the current task's page table root physical address.
pub fn current_page_table_root() -> usize {
    unsafe { TASKS[CURRENT_TASK].page_table_root }
}

/// Return the current task's user ID.
pub fn current_user_id() -> u32 {
    unsafe { TASKS[CURRENT_TASK].user_id }
}

/// Get the current task's state.
pub fn current_state() -> TaskState {
    unsafe { TASKS[CURRENT_TASK].state }
}

/// Set the current task's state (used by Yield to mark self Ready).
pub fn set_state(state: TaskState) {
    unsafe { TASKS[CURRENT_TASK].state = state; }
}

/// Set a task's wake_tick by index.
pub fn set_task_wake_tick(idx: usize, tick: usize) {
    unsafe { TASKS[idx].wake_tick = tick; }
}

// fd helper functions temporarily disabled — will re-add after page fault is resolved
// These require fd_nodes and fd_offsets fields in the Task struct.
/*
pub fn fd_open(vfs_node_idx: usize) -> Result<usize, &'static str> { ... }
pub fn fd_close(fd: usize) -> Result<(), &'static str> { ... }
pub fn fd_node(fd: usize) -> Result<usize, &'static str> { ... }
pub fn fd_offset(fd: usize) -> usize { ... }
pub fn fd_set_offset(fd: usize, offset: usize) { ... }
*/

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

/// Return the number of active non-kernel tasks (Ready + Running + Sleeping).
pub fn active_task_count() -> usize {
    unsafe {
        let mut count = 0;
        for i in 1..MAX_TASKS {
            if TASKS[i].state != TaskState::Unused {
                count += 1;
            }
        }
        count
    }
}

/// Put the current task to sleep for `duration_ms` milliseconds.
/// Delegates to the timer module.
pub fn sleep(duration_ms: usize) {
    crate::timer::sleep_ms(duration_ms);
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
                crate::stats::count_task_spawn();
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
                crate::stats::count_task_spawn();
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
    crate::timer::tick();
    crate::stats::count_tick();
    let current_tick = crate::timer::ticks();

    // Wake sleeping tasks whose wake time has arrived.
    unsafe {
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Sleeping && current_tick >= TASKS[i].wake_tick {
                TASKS[i].state = TaskState::Ready;
            }
        }
    }

    if current_tick == 0 || current_tick % 10 != 0 {
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

        // If we fell back to the same task (no real switch needed), skip CR3 swap.
        // This prevents loading an uninitialized page_table_root (0) during
        // early boot when no userspace tasks exist yet.
        if next_task == CURRENT_TASK {
            TASKS[next_task].state = TaskState::Running;
            return saved_sp;
        }

        CURRENT_TASK = next_task;
        TASKS[next_task].state = TaskState::Running;
        crate::stats::count_context_switch();

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


/// Replace a task's address space with a new ELF image (for execve).
/// Returns the new user stack pointer (RSP) on success.
pub fn replace_address_space(task_idx: usize, image: &crate::elf::ElfImage, elf_data: &[u8]) -> Result<usize, &'static str> {
    if task_idx == 0 || task_idx >= MAX_TASKS {
        return Err("invalid task index");
    }

    unsafe {
        let task = &mut TASKS[task_idx];
        let kernel_cr3 = TASKS[0].page_table_root;

        // Step 1: Free old user pages (but NOT kernel mappings)
        if task.page_table_root != kernel_cr3 && task.page_table_root != 0 {
            // Reclaim user data frames first
            for i in 0..task.user_data_frame_count {
                let pa = task.user_data_frames[i];
                if pa != 0 {
                    crate::frame::deallocate_frame(crate::frame::PhysFrame::from_addr(pa));
                }
            }
            task.user_data_frame_count = 0;

            // Free page tables
            crate::paging::free_user_page_tables(task.page_table_root);
        }

        // Step 2: Extract segments from parsed ELF
        let mut segments: alloc::vec::Vec<(usize, usize, usize, usize, u32)> = alloc::vec::Vec::new();
        for i in 0..image.segment_count {
            let seg = &image.segments[i];
            segments.push((seg.vaddr, seg.data_ptr, seg.filesz, seg.memsz, seg.flags));
        }

        // Step 3: Create new page tables
        let (new_cr3, user_phys_frames) = crate::paging::create_user_page_tables(&segments);

        // Step 4: Copy ELF data into mapped pages
        let mut frame_idx = 0;
        for seg_i in 0..image.segment_count {
            let seg = &image.segments[seg_i];
            let seg_data = &elf_bytes_for_seg(elf_data, seg);

            // Copy data for each frame in this segment
            while frame_idx < user_phys_frames.len() {
                let (vaddr, _size, phys_addr) = user_phys_frames[frame_idx];
                if vaddr < seg.vaddr || vaddr >= seg.vaddr + seg.memsz {
                    break; // This frame belongs to the next segment
                }

                let offset_in_seg = vaddr - seg.vaddr;
                if offset_in_seg < seg.filesz {
                    // This frame has file data
                    let copy_start = seg.data_ptr + offset_in_seg;
                    let copy_len = core::cmp::min(4096, seg.filesz - offset_in_seg);
                    if copy_start + copy_len <= elf_data.len() {
                        let dst = core::slice::from_raw_parts_mut(phys_addr as *mut u8, 4096);
                        dst[..copy_len].copy_from_slice(&elf_data[copy_start..copy_start + copy_len]);
                        // Zero the rest (BSS)
                        if copy_len < 4096 {
                            dst[copy_len..].fill(0);
                        }
                    }
                }
                // else: Pure BSS — already zeroed by frame allocator

                // Track frame for cleanup
                if task.user_data_frame_count < MAX_USER_FRAMES {
                    task.user_data_frames[task.user_data_frame_count] = phys_addr;
                    task.user_data_frame_count += 1;
                }

                frame_idx += 1;
            }
        }

        // Step 5: Setup user stack
        let stack_page = if frame_idx < user_phys_frames.len() {
            user_phys_frames[frame_idx].2
        } else {
            // Need a fresh stack frame
            match crate::frame::allocate_frame() {
                Some(f) => {
                    if task.user_data_frame_count < MAX_USER_FRAMES {
                        task.user_data_frames[task.user_data_frame_count] = f.start_address();
                        task.user_data_frame_count += 1;
                    }
                    f.start_address()
                }
                None => return Err("no free frames for stack"),
            }
        };

        // Map the stack page in the new PML4
        let stack_va = crate::paging::USER_BASE_VA + 0x100000;
        #[cfg(target_arch = "x86_64")]
        {
            crate::arch::paging::map_user_page(new_cr3, stack_va);
            // The map_user_page function breaks a 2MiB block, we need the actual mapping
            // The stack page physical address is stack_page, map it to stack_va
            let pt_pa = crate::arch::paging::map_user_page(new_cr3, stack_va);
            // Write the actual PTE
            let pt = pt_pa as *mut crate::arch::paging::Table;
            let pt_index = (stack_va >> 12) & 0x1FF;
            (*pt).entries[pt_index] = (stack_page as u64) | 0x07; // Present | Write | User
        }

        // Write argc=0 to stack bottom (mimicking Linux execve: argc=0 means no args passed from shell)
        let stack_bottom = stack_page as *mut u8;
        core::ptr::write_bytes(stack_bottom, 0, 4096);
        // argc = 0
        *(stack_bottom as *mut u64) = 0;

        // User stack pointer: top of stack page (stacks grow down)
        let user_sp = stack_va + 4096;

        // Step 6: Rebuild the IRETQ frame
        let saved_sp = rebuild_userspace_frame(task_idx, image.entry_point, user_sp);

        // Update TCB
        task.page_table_root = new_cr3;
        task.saved_sp = saved_sp;

        // Load new CR3 so subsequent memory accesses use the new page tables
        let new_cr3_reg = new_cr3 as u64;
        core::arch::asm!("mov cr3, {}", in(reg) new_cr3_reg);

        Ok(user_sp)
    }
}

/// Get the ELF data slice for a segment. The data_ptr is an offset into elf_data.
unsafe fn elf_bytes_for_seg<'a>(elf_data: &'a [u8], seg: &crate::elf::ElfSegment) -> &'a [u8] {
    if seg.data_ptr + seg.filesz <= elf_data.len() {
        &elf_data[seg.data_ptr..seg.data_ptr + seg.filesz]
    } else {
        &[]
    }
}

/// Rebuild the IRETQ frame for a userspace task (used by execve).
/// Returns the new saved_sp value.
pub fn rebuild_userspace_frame(task_idx: usize, entry: usize, user_sp: usize) -> usize {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        let stack_top = TASKS[task_idx].stack.as_ptr() as usize + STACK_SIZE;
        let mut sp = stack_top;

        // IRETQ frame
        sp -= 8; *(sp as *mut u64) = 0x23;                // SS (Ring 3 data)
        sp -= 8; *(sp as *mut u64) = user_sp as u64;      // RSP
        sp -= 8; *(sp as *mut u64) = 0x202;               // RFLAGS (IF=1)
        sp -= 8; *(sp as *mut u64) = 0x1B;                // CS (Ring 3 code)
        sp -= 8; *(sp as *mut u64) = entry as u64;        // RIP

        // 15 GP registers, all zero
        for _ in 0..15 {
            sp -= 8; *(sp as *mut u64) = 0;
        }

        // SSE area (256 bytes)
        sp -= 256;
        for j in 0..32 {
            *(sp as *mut u64).add(j) = 0;
        }

        sp
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let stack_top = TASKS[task_idx].stack.as_ptr() as usize + STACK_SIZE;
        let mut sp = stack_top;

        sp -= 8; *(sp as *mut u64) = entry as u64;         // ELR_EL1
        sp -= 8; *(sp as *mut u64) = 0x0000_0000_0000_0000; // SPSR_EL1 (EL0t)
        sp -= 8; *(sp as *mut u64) = user_sp as u64;       // SP_EL0
        sp -= 256;
        for j in 0..32 {
            *(sp as *mut u64).add(j) = 0;
        }

        sp
    }
}

