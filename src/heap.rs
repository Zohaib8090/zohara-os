// src/heap.rs

use core::alloc::Layout;
use crate::allocator::FixedBlockAllocator; // <--- Changed

#[repr(align(16))]
struct AlignedHeap([u8; 64 * 1024]);

static HEAP_MEMORY: AlignedHeap = AlignedHeap([0; 64 * 1024]);

#[global_allocator]
static ALLOCATOR: FixedBlockAllocator = FixedBlockAllocator::new(); // <--- Changed

pub fn init_heap() {
    unsafe {
        let heap_start = HEAP_MEMORY.0.as_ptr() as usize;
        let heap_size = HEAP_MEMORY.0.len();
        ALLOCATOR.init(heap_start, heap_size);
    }
}

#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    panic!("Out of memory! Allocation of {} bytes failed.", layout.size());
}