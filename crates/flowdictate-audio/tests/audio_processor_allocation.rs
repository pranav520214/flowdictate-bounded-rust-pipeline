//! Allocation regression test for the warmed worker-side processing seam.
#![allow(unsafe_code, clippy::expect_used, clippy::float_cmp)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use flowdictate_audio::{AudioFormat, AudioProcessor, SegmentEvent, VadConfig};

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
fn warmed_worker_chunk_does_not_allocate() {
    let format = AudioFormat::new(48_000, 2).expect("48 kHz stereo is supported");
    let vad = VadConfig::new(0.6, 0.4, 2, 8, 1_000).expect("bounded VAD policy");
    let mut processor = AudioProcessor::new(format, 768, vad).expect("processor preallocates");
    let input = vec![0.25_f32; 768 * 2];
    let mut events = vec![SegmentEvent::Idle; processor.maximum_events_per_chunk()];

    processor
        .process_interleaved(&input, &mut events)
        .expect("warm-up succeeds");
    let before = ALLOCATIONS.load(Ordering::SeqCst);

    let _report = processor
        .process_interleaved(&input, &mut events)
        .expect("steady-state processing succeeds");

    let after = ALLOCATIONS.load(Ordering::SeqCst);
    assert_eq!(after, before);
}
