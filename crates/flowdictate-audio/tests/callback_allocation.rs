//! Allocation regression test for the public callback write seam.
#![allow(unsafe_code, clippy::expect_used, clippy::float_cmp)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use flowdictate_audio::bounded_audio_ring;

struct CountingAllocator;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: Every allocation and deallocation is forwarded unchanged to the
// process system allocator. The counter is independent bookkeeping only.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: `layout` is forwarded unchanged as required by GlobalAlloc.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` and `layout` came from the corresponding system allocation.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[test]
fn warmed_callback_write_does_not_allocate() {
    let (mut producer, _consumer) = bounded_audio_ring(32).expect("capacity is non-zero");
    let input = [0_i16; 16];
    let before = ALLOCATIONS.load(Ordering::SeqCst);

    let _outcome = producer.try_push_i16(&input);

    let after = ALLOCATIONS.load(Ordering::SeqCst);
    assert_eq!(after, before);
}
