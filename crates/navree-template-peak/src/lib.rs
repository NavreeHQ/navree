//! A counting allocator for the tests of this crate: the live bytes, and the
//! most that were live since the last [`reset_peak`].

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

/// Counts what goes through the system allocator.
pub struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call forwards to `System` with the caller's arguments; the
// counters do not touch the memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract is the same.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let live = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(live, Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is the same.
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        // SAFETY: the caller's contract is the same.
        let moved = unsafe { System.realloc(pointer, layout, size) };
        if !moved.is_null() {
            // The old block counts until the new one exists: a copy holds both.
            let live = LIVE.fetch_add(size, Relaxed) + size;
            PEAK.fetch_max(live, Relaxed);
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        moved
    }
}

/// Starts a new measurement.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Relaxed), Relaxed);
}

/// The most bytes live since [`reset_peak`], beyond what was live then.
pub fn peak_above(base: usize) -> usize {
    PEAK.load(Relaxed).saturating_sub(base)
}

/// The bytes live now.
pub fn live() -> usize {
    LIVE.load(Relaxed)
}
