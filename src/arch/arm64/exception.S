// ARM64 Exception Vector Table
//
// 16 vector entries, 128 bytes each, 2048-byte aligned.
//
// Routing:
//   offset 0x000  Current EL with SP0  -> fatal (should never happen)
//   offset 0x200  Current EL with SPx  -> SVC: svc_handler, IRQ: irq_handler
//   offset 0x400  Lower EL with SP0    -> SVC: user_svc_handler, IRQ: irq_handler
//                                         Other sync: user exception (kill task)
//   offset 0x600  Lower EL with SPx    -> fatal (should never happen)

.macro VECTOR_BRANCH target
    .align 7
    b \target
.endm

.section .text

.macro SAVE_REGS
    sub sp, sp, #256
    stp x0, x1, [sp, #0]
    stp x2, x3, [sp, #16]
    stp x4, x5, [sp, #32]
    stp x6, x7, [sp, #48]
    stp x8, x9, [sp, #64]
    stp x10, x11, [sp, #80]
    stp x12, x13, [sp, #96]
    stp x14, x15, [sp, #112]
    stp x16, x17, [sp, #128]
    stp x18, x19, [sp, #144]
    stp x20, x21, [sp, #160]
    stp x22, x23, [sp, #176]
    stp x24, x25, [sp, #192]
    stp x26, x27, [sp, #208]
    stp x28, x29, [sp, #224]
    str x30, [sp, #240]
.endm

.macro RESTORE_REGS
    ldp x0, x1, [sp, #0]
    ldp x2, x3, [sp, #16]
    ldp x4, x5, [sp, #32]
    ldp x6, x7, [sp, #48]
    ldp x8, x9, [sp, #64]
    ldp x10, x11, [sp, #80]
    ldp x12, x13, [sp, #96]
    ldp x14, x15, [sp, #112]
    ldp x16, x17, [sp, #128]
    ldp x18, x19, [sp, #144]
    ldp x20, x21, [sp, #160]
    ldp x22, x23, [sp, #176]
    ldp x24, x25, [sp, #192]
    ldp x26, x27, [sp, #208]
    ldp x28, x29, [sp, #224]
    ldr x30, [sp, #240]
    add sp, sp, #256
.endm

// ---- Fatal: print + halt (kernel-mode unrecoverable) ----
fatal_handler:
    SAVE_REGS
    mov x0, sp
    bl handle_exception
    RESTORE_REGS
    eret

// ---- SVC from EL1 (kernel mode) ----
svc_handler:
    SAVE_REGS
    mov x0, sp
    bl handle_syscall
    str x0, [sp]
    RESTORE_REGS
    eret

// ---- IRQ: timer/scheduler (shared by EL0 and EL1) ----
//
// When entered from "Lower EL with SP0", the hardware placed the exception
// frame on SP_EL0 (the user stack). We immediately switch to SP_EL1 (kernel
// stack) via `msr spsel, #1` so the save area and all Rust code run on a
// trusted kernel stack. The saved user SP is preserved in SP_EL0 which we
// read/save/restore in the trap frame just like the EL1 IRQ path.
irq_handler:
    // If we entered from EL0 (SP0 group), switch to the kernel stack.
    // We detect this by checking SPSR_EL1.M[3:0]: 0000 = EL0t.
    mrs x0, SPSR_EL1
    and x0, x0, #0xF
    cbnz x0, 1f              // if mode != EL0t, already on kernel stack
    msr spsel, #1             // switch to SP_EL1 (kernel stack)
1:
    SAVE_REGS

    sub sp, sp, #24

    mrs x0, SP_EL0
    str x0, [sp, #0]
    mrs x0, SPSR_EL1
    str x0, [sp, #8]
    mrs x0, ELR_EL1
    str x0, [sp, #16]

    bl gic_acknowledge
    bl timer_handle_irq
    mov x0, sp
    bl timer_handler_rust
    mov sp, x0

    ldr x0, [sp, #16]
    msr ELR_EL1, x0
    ldr x0, [sp, #8]
    msr SPSR_EL1, x0
    ldr x0, [sp, #0]
    msr SP_EL0, x0

    add sp, sp, #24
    RESTORE_REGS
    eret

// ---- SVC from EL0 (user mode): syscall dispatch ----
//
// Entered via "Lower EL with SP0, Synchronous". Hardware uses SP_EL0, so we
// first switch to SP_EL1 (kernel stack) and save the user's registers there.
user_svc_handler:
    // Switch to kernel stack
    msr spsel, #1
    SAVE_REGS

    mov x0, sp
    bl handle_syscall
    str x0, [sp]        // return value into saved x0
    RESTORE_REGS
    eret

// ---- Other sync exceptions from EL0 (page fault, etc.) ----
//
// Kills the offending user task rather than halting the kernel.
user_sync_handler:
    msr spsel, #1
    SAVE_REGS
    mov x0, sp
    bl handle_user_exception
    // handle_user_exception kills the task (loops forever in exit_current_task).
    // Unreachable, but for safety:
    RESTORE_REGS
    eret

// ---- The vector table ----

.global exception_vector_base
.balign 2048
exception_vector_base:
    // Current EL with SP0 (should never fire in normal operation)
    VECTOR_BRANCH fatal_handler        // 0x000 Sync
    VECTOR_BRANCH fatal_handler        // 0x080 IRQ
    VECTOR_BRANCH fatal_handler        // 0x100 FIQ
    VECTOR_BRANCH fatal_handler        // 0x180 SError

    // Current EL with SPx (kernel exceptions/syscalls/IRQs)
    VECTOR_BRANCH svc_handler          // 0x200 Sync ← SVC from EL1
    VECTOR_BRANCH irq_handler          // 0x280 IRQ   ← timer from EL1
    VECTOR_BRANCH fatal_handler        // 0x300 FIQ
    VECTOR_BRANCH fatal_handler        // 0x380 SError

    // Lower EL with SP0 (user-mode exceptions/syscalls/IRQs)
    VECTOR_BRANCH user_sync_handler    // 0x400 Sync ← SVC/abort from EL0
    VECTOR_BRANCH irq_handler          // 0x480 IRQ   ← timer from EL0
    VECTOR_BRANCH fatal_handler        // 0x500 FIQ
    VECTOR_BRANCH fatal_handler        // 0x580 SError

    // Lower EL with SPx (should never fire)
    VECTOR_BRANCH fatal_handler        // 0x600 Sync
    VECTOR_BRANCH fatal_handler        // 0x680 IRQ
    VECTOR_BRANCH fatal_handler        // 0x700 FIQ
    VECTOR_BRANCH fatal_handler        // 0x780 SError
