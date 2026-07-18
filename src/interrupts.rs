// src/interrupts.rs

use core::ptr::addr_of_mut;

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_mid: u16,
    offset_high: u32,
    zero: u32,
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

const fn blank_entry() -> IdtEntry {
    IdtEntry {
        offset_low: 0, selector: 0, ist: 0, attributes: 0, offset_mid: 0, offset_high: 0, zero: 0,
    }
}

static mut IDT: [IdtEntry; 256] = [blank_entry(); 256];

#[repr(C)]
pub struct InterruptStackFrame;

// Tell Rust our assembly handler exists
extern "C" {
    fn timer_irq_handler();
}

extern "x86-interrupt" fn serial_handler(_stack_frame: &mut InterruptStackFrame) {
    let data: u8;
    unsafe {
        core::arch::asm!(
        "in al, dx",
        in("dx") 0x3F8u16,
        out("al") data,
        );
    }
    crate::keyboard::push_key(data);

    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x20u8);
    }
}

unsafe fn set_handler(index: usize, handler: extern "x86-interrupt" fn(&mut InterruptStackFrame)) {
    let addr = handler as usize;
    IDT[index].offset_low = (addr & 0xFFFF) as u16;
    IDT[index].offset_mid = ((addr >> 16) & 0xFFFF) as u16;
    IDT[index].offset_high = ((addr >> 32) & 0xFFFFFFFF) as u32;
    IDT[index].selector = 0x08;
    IDT[index].attributes = 0x8E;
    IDT[index].zero = 0;
    IDT[index].ist = 0;
}

/// Register an interrupt handler that receives a CPU error code (e.g. #PF, #GP).
///
/// The IDT entry is identical to `set_handler`; the error-code handling is
/// implicit in the CPU/interrupt-frame ABI — the CPU pushes the error code
/// onto the stack before the interrupt frame, and `x86-interrupt` ABI makes it
/// a second function parameter.
pub unsafe fn set_handler_with_error_code(
    index: usize,
    handler: extern "x86-interrupt" fn(&mut InterruptStackFrame, u64),
) {
    let addr = handler as usize;
    IDT[index].offset_low = (addr & 0xFFFF) as u16;
    IDT[index].offset_mid = ((addr >> 16) & 0xFFFF) as u16;
    IDT[index].offset_high = ((addr >> 32) & 0xFFFFFFFF) as u32;
    IDT[index].selector = 0x08;
    IDT[index].attributes = 0x8E; // interrupt gate
    IDT[index].zero = 0;
    IDT[index].ist = 0;
}

/// Register a raw assembly syscall handler at IDT vector 0x80 (128).
///
/// Unlike `set_handler`, this accepts a raw address (not an
/// `extern "x86-interrupt"` fn) because the syscall stub is hand-written
/// assembly that pushes/pops registers and calls `dispatch` directly — it
/// cannot conform to the `x86-interrupt` ABI.
pub unsafe fn set_syscall_handler(stub_addr: usize) {
    IDT[0x80].offset_low = (stub_addr & 0xFFFF) as u16;
    IDT[0x80].offset_mid = ((stub_addr >> 16) & 0xFFFF) as u16;
    IDT[0x80].offset_high = ((stub_addr >> 32) & 0xFFFFFFFF) as u32;
    IDT[0x80].selector = 0x08;
    IDT[0x80].attributes = 0xEE; // P=1, DPL=3, S=0, Type=E (64-bit interrupt gate, Ring 3 accessible)
    IDT[0x80].zero = 0;
    IDT[0x80].ist = 0;
}

unsafe fn remap_pic() {
    core::arch::asm!("out dx, al", in("dx") 0x20u16, in("al") 0x11u8);
    core::arch::asm!("out dx, al", in("dx") 0xA0u16, in("al") 0x11u8);
    core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0x20u8);
    core::arch::asm!("out dx, al", in("dx") 0xA1u16, in("al") 0x28u8);
    core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0x04u8);
    core::arch::asm!("out dx, al", in("dx") 0xA1u16, in("al") 0x02u8);
    core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0x01u8);
    core::arch::asm!("out dx, al", in("dx") 0xA1u16, in("al") 0x01u8);

    // MASK ALL EXCEPT COM1 (IRQ4) AND TIMER (IRQ0)
    // Master mask: 0xEC (1110 1100) -> Bit 0 (Timer) and Bit 4 (Serial) are 0
    core::arch::asm!("out dx, al", in("dx") 0x21u16, in("al") 0xECu8);
    core::arch::asm!("out dx, al", in("dx") 0xA1u16, in("al") 0xFFu8);
}

pub fn init_idt() {
    unsafe {
        remap_pic();

        // Set Timer IRQ0 (Vector 32) to our raw assembly handler
        let addr = timer_irq_handler as usize;
        IDT[32].offset_low = (addr & 0xFFFF) as u16;
        IDT[32].offset_mid = ((addr >> 16) & 0xFFFF) as u16;
        IDT[32].offset_high = ((addr >> 32) & 0xFFFFFFFF) as u32;
        IDT[32].selector = 0x08;
        IDT[32].attributes = 0x8E;
        IDT[32].zero = 0;
        IDT[32].ist = 0;

        // Set Serial IRQ4 (Vector 36) to our Rust handler
        set_handler(36, serial_handler);

        let idt_ptr = IdtPointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: addr_of_mut!(IDT) as u64,
        };

        core::arch::asm!(
        "lidt [{}]",
        in(reg) &idt_ptr,
        );

        // Configure COM1 to send interrupts
        core::arch::asm!("out dx, al", in("dx") 0x3F9u16, in("al") 0x01u8);

        // Start the hardware timer!
        crate::arch::init_timer();
    }
}