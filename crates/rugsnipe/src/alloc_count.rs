//! Counting global allocator for the zero-allocation check of
//! `rugsnipe-replay` (docs/06 section 14 item 5). Counting is per thread:
//! only threads that called [`set_counting(true)`](set_counting) are
//! counted, so the rslog/state/sender threads do not pollute the hot
//! thread's number. A binary opts in with
//! `#[global_allocator] static A: CountingAlloc = CountingAlloc;`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
}

/// Counts `alloc`/`realloc` calls made by counting threads.
#[derive(Debug, Default, Clone, Copy)]
pub struct CountingAlloc;

#[inline]
fn counting() -> bool {
    COUNTING.try_with(|c| c.get()).unwrap_or(false)
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if counting() {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if counting() {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` was allocated by `System` with `layout`.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if counting() {
            ALLOCS.fetch_add(1, Relaxed);
            BYTES.fetch_add(new_size as u64, Relaxed);
        }
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Count (or stop counting) allocations made by the calling thread.
pub fn set_counting(on: bool) {
    let _ = COUNTING.try_with(|c| c.set(on));
}

/// `(allocations, bytes)` counted since the last [`reset`].
pub fn counts() -> (u64, u64) {
    (ALLOCS.load(Relaxed), BYTES.load(Relaxed))
}

pub fn reset() {
    ALLOCS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
}
