// src/allocator.rs

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr;

/// A simple Bump Allocator (used internally by the Block Allocator)
pub struct BumpAllocator {
    next: UnsafeCell<usize>,
    end: UnsafeCell<usize>,
}

unsafe impl Sync for BumpAllocator {}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self { next: UnsafeCell::new(0), end: UnsafeCell::new(0) }
    }

    pub unsafe fn init(&self, start: usize, size: usize) {
        *self.next.get() = start;
        *self.end.get() = start + size;
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let next = *self.next.get();
        let end = *self.end.get();
        let align = layout.align();
        let size = layout.size();

        let aligned_start = (next + align - 1) & !(align - 1);
        let allocated_end = aligned_start + size;

        if allocated_end > end { return ptr::null_mut(); }

        *self.next.get() = allocated_end;
        aligned_start as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

// --- FIXED-SIZE BLOCK ALLOCATOR ---

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

#[repr(C)]
struct BlockNode {
    next: *mut BlockNode,
}

pub struct FixedBlockAllocator {
    list_heads: [UnsafeCell<*mut BlockNode>; BLOCK_SIZES.len()],
    bump: BumpAllocator,
}

unsafe impl Sync for FixedBlockAllocator {}

impl FixedBlockAllocator {
    pub const fn new() -> Self {
        const NULL: UnsafeCell<*mut BlockNode> = UnsafeCell::new(ptr::null_mut());
        Self {
            list_heads: [NULL; BLOCK_SIZES.len()],
            bump: BumpAllocator::new(),
        }
    }

    pub unsafe fn init(&self, heap_start: usize, heap_size: usize) {
        self.bump.init(heap_start, heap_size);
    }

    fn get_block_index(layout: &Layout) -> Option<usize> {
        let required_size = layout.size().max(layout.align());
        BLOCK_SIZES.iter().position(|&s| s >= required_size)
    }
}

unsafe impl GlobalAlloc for FixedBlockAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match Self::get_block_index(&layout) {
            Some(index) => {
                let head_ptr = self.list_heads[index].get();
                let current_head = *head_ptr;

                if !current_head.is_null() {
                    // 1. We have a free block! Pop it from the list.
                    let next = (*current_head).next;
                    *head_ptr = next;
                    current_head as *mut u8
                } else {
                    // 2. No free blocks, carve a new one from the bump allocator.
                    let block_size = BLOCK_SIZES[index];
                    self.bump.alloc(Layout::from_size_align(block_size, block_size).unwrap())
                }
            }
            None => {
                // 3. Too large for our blocks, fallback to raw bump allocation.
                self.bump.alloc(layout)
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        match Self::get_block_index(&layout) {
            Some(index) => {
                let head_ptr = self.list_heads[index].get();
                let new_node = ptr as *mut BlockNode;

                // Push the freed block onto the front of the list
                (*new_node).next = *head_ptr;
                *head_ptr = new_node;
            }
            None => {
                // Allocated via bump allocator, we can't free it.
                // (This is fine for huge or rare allocations).
            }
        }
    }
}