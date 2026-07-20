// src/smp.rs

//! Symmetric Multiprocessing — per-core data structures and AP wake.
//!
//! Each core gets a `PerCpuData` struct accessed via GS-base for fast
//! per-core reads (single instruction, no memory lookup). The BSP sets
//! up its own GS-base during boot; each AP does the same in its trampoline.

pub const MAX_CORES: usize = 64;

/// Information about a discovered CPU core.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CpuInfo {
    pub apic_id: u32,
    pub core_id: usize,
    pub is_bsp: bool,
}

/// Per-CPU data — laid out in memory so GS:[offset] can reach each field.
/// The first field MUST be core_id at offset 0 so `gs:[0]` works.
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PerCpuData {
    pub core_id: usize,         // offset 0x00
    pub apic_id: u32,           // offset 0x08
    pub _pad0: u32,             // padding for alignment
    pub current_task: usize,    // offset 0x10
    pub kernel_stack_top: usize,// offset 0x18
}

impl PerCpuData {
    const fn new() -> Self {
        Self {
            core_id: 0,
            apic_id: 0,
            _pad0: 0,
            current_task: 0,
            kernel_stack_top: 0,
        }
    }
}

/// All per-CPU data, statically allocated. Only CPU_COUNT entries are live.
pub static mut PER_CPU_DATA: [PerCpuData; MAX_CORES] = [PerCpuData::new(); MAX_CORES];

/// Number of CPUs discovered at boot (populated by acpi::init).
pub static mut CPU_COUNT: usize = 0;

/// CPU information table (populated by acpi::init from MADT).
pub static mut CPUS: [Option<CpuInfo>; MAX_CORES] = {
    // const initialization: fill with None
    const NONE: Option<CpuInfo> = None;
    [NONE; MAX_CORES]
};

/// Read the current core's ID from GS:[0].
#[inline]
pub fn core_id() -> usize {
    let val: usize;
    unsafe {
        core::arch::asm!("mov {}, gs:[0]", out(reg) val);
    }
    val
}

/// Read current core's current_task from GS:[0x10].
#[inline]
pub fn current_task_per_core() -> usize {
    let val: usize;
    unsafe {
        core::arch::asm!("mov {}, gs:[0x10]", out(reg) val);
    }
    val
}

/// Write current core's current_task via GS:[0x10].
#[inline]
pub fn set_current_task_per_core(task: usize) {
    unsafe {
        core::arch::asm!("mov gs:[0x10], {}", in(reg) task);
    }
}

/// Initialize BSP's per-core data and set GS-base.
///
/// Called after LAPIC init, before APs are woken.
pub fn init_bsp() {
    unsafe {
        let apic_id = crate::apic::lapic_id();
        PER_CPU_DATA[0].core_id = 0;
        PER_CPU_DATA[0].apic_id = apic_id;
        PER_CPU_DATA[0].current_task = crate::task::current_task();
        PER_CPU_DATA[0].kernel_stack_top =
            crate::task::current_task_ref().stack.as_ptr() as usize + 32768;

        let gs_base = &PER_CPU_DATA[0] as *const _ as u64;
        // Write IA32_GS_BASE (MSR 0xC0000101)
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0xC0000101u32,
            in("eax") gs_base as u32,
            in("edx") (gs_base >> 32) as u32,
        );
        // Write IA32_KERNEL_GS_BASE (MSR 0xC0000082) for swapgs
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0xC0000082u32,
            in("eax") gs_base as u32,
            in("edx") (gs_base >> 32) as u32,
        );

        crate::println!("[SMP] BSP core_id=0 apic_id={} GS-base={:#x}", apic_id, gs_base);
    }
}

/// Set GS-base for a given core index. Used by AP trampoline setup.
pub unsafe fn set_gs_base_for_core(core_idx: usize) {
    let gs_base = &PER_CPU_DATA[core_idx] as *const _ as u64;
    core::arch::asm!(
        "wrmsr",
        in("ecx") 0xC0000101u32,
        in("eax") gs_base as u32,
        in("edx") (gs_base >> 32) as u32,
    );
    core::arch::asm!(
        "wrmsr",
        in("ecx") 0xC0000082u32,
        in("eax") gs_base as u32,
        in("edx") (gs_base >> 32) as u32,
    );
}

// --- AP Trampoline Management ---

/// Physical address where the trampoline is placed (512 KiB).
const AP_TRAMPOLINE_PA: usize = 0x8000;

/// The AP trampoline as raw bytes.
/// MINIMAL TEST: 16-bit real mode only — writes 0xAA to 0x9000, then CLI+HLT.
/// Purpose: prove the AP starts executing at all after INIT-SIPI-SIPI.
mod trampoline_blob {
    pub static AP_TRAMPOLINE: [u8; 9] = [
        // MOV BYTE [0x9000], 0xAA  (5 bytes)
        0xC6, 0x06, 0x00, 0x90, 0xAA,
        // CLI                       (1 byte)
        0xFA,
        // HLT                       (1 byte)
        0xF4,
        // JMP $ (infinite loop)     (2 bytes)
        0xEB, 0xFE,
    ];
}
use trampoline_blob::AP_TRAMPOLINE;

/// Copy the trampoline code to low memory and patch the data fields.
///
/// Fixed layout at physical 0x8000:
///   0x8000: code (186 bytes)
///   0x8080: cr3 (8 bytes)
///   0x8088: stack (8 bytes)
///   0x8090: entry (8 bytes)
///   0x8098: gdt64_ptr (6 bytes)
///   0x80A0: 32-bit GDT (24 bytes)
///   0x80B8: gdt32_ptr (6 bytes)
pub unsafe fn setup_trampoline(cr3: usize, stack_top: usize, entry: usize, gdt: usize) {
    let len = AP_TRAMPOLINE.len();
    let dst = AP_TRAMPOLINE_PA as *mut u8;

    // Copy the trampoline code to low memory
    core::ptr::copy_nonoverlapping(AP_TRAMPOLINE.as_ptr(), dst, len);

    // Patch data at fixed offsets
    let base = AP_TRAMPOLINE_PA as *mut u8;
    *(base.add(0x80) as *mut u64) = cr3 as u64;
    *(base.add(0x88) as *mut u64) = stack_top as u64;
    *(base.add(0x90) as *mut u64) = entry as u64;

    // Patch 64-bit GDT base in LGDT structure at 0x8098
    // Structure: limit(u16 at +0) = 55, base(u32 at +2) = gdt_addr
    *(base.add(0x9A) as *mut u32) = gdt as u32;

    // Set up 32-bit GDT at 0x80A0 and its pointer at 0x80B8
    core::ptr::write_bytes(base.add(0xA0), 0, 8);  // null entry
    *(base.add(0xA8) as *mut u64) = 0x00CF9A000000FFFFu64;  // code
    *(base.add(0xB0) as *mut u64) = 0x00CF92000000FFFFu64;  // data
    *(base.add(0xB8) as *mut u16) = 23;  // limit
    *(base.add(0xBA) as *mut u32) = 0x80A0;  // base

    crate::println!("[SMP] Trampoline at {:#x}, len={}, cr3={:#x}, entry={:#x}, gdt={:#x}",
        AP_TRAMPOLINE_PA, len, cr3, entry, gdt);

    // Verify the trampoline was copied correctly
    let first_byte = *(AP_TRAMPOLINE_PA as *const u8);
    let cli_byte = AP_TRAMPOLINE[0];
    crate::println!("[SMP] Verify: first byte at 0x8000 = 0x{:02X} (expected 0x{:02X})",
        first_byte, cli_byte);
}

/// AP entry point — called from trampoline after transitioning to long mode.
///
/// Each AP starts here with its own stack. The GS-base is NOT yet set up
/// (the trampoline doesn't touch MSRs). We use CPUID to get our APIC ID,
/// look up our core index, and set GS-base before doing anything else.
#[no_mangle]
pub extern "C" fn ap_entry() -> ! {
    unsafe {
        // Simple test: just print something as early as possible
        crate::println!("[SMP-TRACE] AP entered!");

        // Read APIC ID via CPUID leaf 1, EBX bits 24-31
        let apic_id: u32;
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "shr ebx, 24",
            "mov {out:e}, ebx",
            "pop rbx",
            out = out(reg) apic_id,
        );
        crate::println!("[SMP-TRACE] AP cpuid apic_id={}", apic_id);

        let mut core_idx = 0usize;
        for i in 0..crate::acpi::smp_cpu_count() {
            if let Some(cpu) = crate::acpi::smp_cpu(i) {
                if cpu.apic_id == apic_id {
                    core_idx = i;
                    break;
                }
            }
        }
        crate::println!("[SMP-TRACE] AP core_idx={}", core_idx);

        set_gs_base_for_core(core_idx);
        let id = core_id();

        crate::paging::map_page(crate::acpi::MADT_LOCAL_APIC_ADDR as usize & !0xFFF);
        crate::apic::init_local_apic();

        crate::println!("[SMP] AP core_id={} online, apic_id={}",
            id, PER_CPU_DATA[id].apic_id);

        crate::smp_test::contention_worker(id);

        loop {
            core::arch::asm!("sti");
            core::arch::asm!("hlt");
        }
    }
}

/// Wake all application processors discovered via MADT.
///
/// For each non-BSP core:
/// 1. Allocate a kernel stack from the frame allocator
/// 2. Set up per-core data and GS-base
/// 3. Copy and patch the trampoline
/// 4. Send INIT-SIPI-SIPI sequence
/// 5. Wait for the AP to reach `ap_entry()`
pub fn wake_aps() {
    unsafe {
        let cpu_count = crate::acpi::smp_cpu_count();
        if cpu_count <= 1 {
            crate::println!("[SMP] Single core system — no APs to wake");
            return;
        }

        // Get BSP's current CR3 (page tables)
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3);

        for i in 1..cpu_count {
            let cpu = match crate::acpi::smp_cpu(i) {
                Some(c) => c,
                None => continue,
            };

            let apic_id = cpu.apic_id;

            // Allocate a kernel stack for this AP (1 page = 4 KiB)
            let stack_frame = match crate::frame::allocate_frame() {
                Some(f) => f,
                None => {
                    crate::println!("[SMP] WARNING: out of frames for AP core_id={} stack", i);
                    continue;
                }
            };
            let stack_top = stack_frame.start_address() + 4096;

            // Set up per-core data
            PER_CPU_DATA[i].core_id = i;
            PER_CPU_DATA[i].apic_id = apic_id;
            PER_CPU_DATA[i].current_task = 0; // idle task
            PER_CPU_DATA[i].kernel_stack_top = stack_top;

            // Get the virtual address of the BSP's 64-bit GDT.
            // After the trampoline enables paging with the BSP's CR3,
            // the kernel's virtual addresses (0x40000000+) are accessible.
            // So we pass the virtual address directly.
            extern "C" { static gdt64_full: u8; }
            let gdt_virt = &gdt64_full as *const u8 as usize;

            // Copy trampoline to 0x8000 and patch data fields
            setup_trampoline(cr3 as usize, stack_top, ap_entry as usize, gdt_virt);

            // --- Diagnostic: Re-verify trampoline is intact right before SIPI ---
            let pre_first = core::ptr::read_volatile(0x8000 as *const u8);
            crate::println!("[SMP-TEST] Trampoline verify BEFORE SIPI: [0x8000]=0x{:02X} (expect 0xC6)", pre_first);

            // Clear marker
            core::ptr::write_volatile(0x9000 as *mut u8, 0x00);

            // --- Diagnostic: Try to enable AP's LAPIC before SIPI ---
            crate::apic::try_enable_ap_lapic(apic_id as u32);

            // --- Clean 6-step INIT-SIPI-SIPI ---
            crate::apic::send_init_sipi_clean(apic_id as u32);

            // --- Diagnostic: Re-verify trampoline AFTER SIPI ---
            let post_first = core::ptr::read_volatile(0x8000 as *const u8);
            let marker = core::ptr::read_volatile(0x9000 as *const u8);
            crate::println!("[SMP-TEST] Post-SIPI: [0x8000]=0x{:02X} marker=0x{:02X} (expect 0xC6/0xAA)", post_first, marker);

            // Wait for AP to start executing
            for _ in 0..50_000_000u32 {
                core::hint::spin_loop();
            }
            core::arch::asm!("wbinvd");

            let marker_final = core::ptr::read_volatile(0x9000 as *const u8);
            crate::println!("[SMP-TEST] Final: marker=0x{:02X} (expect 0xAA)", marker_final);
        }
    }
}
