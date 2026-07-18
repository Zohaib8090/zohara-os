// src/heap.rs

use core::alloc::Layout;
use crate::allocator::BumpAllocator;

// 1. Define a 64 Kilobyte static array for our heap
#[repr(align(16))]
struct AlignedHeap([u8; 64 * 1024]);

static HEAP_MEMORY: AlignedHeap = AlignedHeap([0; 64 * 1024]);

// 2. Instantiate the allocator
pub static ALLOCATOR: BumpAllocator = BumpAllocator::new();

// 3. Tell Rust to use our allocator globally
#[global_allocator]
static GLOBAL_ALLOCATOR: &BumpAllocator = &ALLOCATOR;

/// Initialize the heap so it knows where our 64KB array lives in memory
pub fn init_heap() {
    unsafe {
        let heap_start = HEAP_MEMORY.0.as_ptr() as usize;
        let heap_size = HEAP_MEMORY.0.len();
        ALLOCATOR.init(heap_start, heap_size);
    }
}

// 4. Define what happens if we run out of memory
#[alloc_error_handler]
fn alloc_error(layout: Layout) -> ! {
    panic!("Out of memory! Allocation of {} bytes failed.", layout.size());
}