//! Thread-local heap accounting for the moved no-copy fixture and fold tests.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[derive(Clone, Copy, Default)]
pub(crate) struct Tally {
    pub allocs: u64,
    pub realloc_copy_bytes: u64,
    pub live_bytes: i64,
}

impl Tally {
    pub fn since(self, before: Self) -> Self {
        Self {
            allocs: self.allocs - before.allocs,
            realloc_copy_bytes: self.realloc_copy_bytes - before.realloc_copy_bytes,
            live_bytes: self.live_bytes - before.live_bytes,
        }
    }
}

thread_local! {
    static TALLY: Cell<Tally> = const { Cell::new(Tally { allocs: 0, realloc_copy_bytes: 0, live_bytes: 0 }) };
}

pub(crate) fn tally() -> Tally {
    TALLY.try_with(Cell::get).unwrap_or_default()
}

fn record(allocs: u64, copied: u64, delta: i64) {
    let _ = TALLY.try_with(|cell| {
        let mut value = cell.get();
        value.allocs += allocs;
        value.realloc_copy_bytes += copied;
        value.live_bytes += delta;
        cell.set(value);
    });
}

pub(crate) struct Counting;
// SAFETY: the caller's allocation arguments are forwarded unchanged to System.
// Accounting touches only a thread-local Cell, with no allocation of its own.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(1, 0, layout.size() as i64);
        // SAFETY: caller's layout is passed unchanged to System.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(1, 0, layout.size() as i64);
        // SAFETY: caller's layout is passed unchanged to System.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(0, 0, -(layout.size() as i64));
        // SAFETY: caller's pointer and layout are passed unchanged to System.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(
            0,
            layout.size().min(size) as u64,
            size as i64 - layout.size() as i64,
        );
        // SAFETY: caller's pointer, layout and size are passed unchanged.
        unsafe { System.realloc(ptr, layout, size) }
    }
}
