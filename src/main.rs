#![no_std]
#![no_main]

use core::panic::PanicInfo;

// Our custom panic handler since we don't have the standard library
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// Our cross-platform kernel entry point
#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    loop {}
}