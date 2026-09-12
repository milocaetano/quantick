//! Heap work, counted per thread — the test binary's allocator.
//!
//! Compiled into the app's test binary only (`main.rs` declares it under
//! `cfg(test)`), where it wraps the system allocator and tallies, for the
//! thread that asked, every allocation, its bytes, every reallocation and the
//! bytes a reallocation may have to copy. Production builds keep the system
//! allocator untouched.
//!
//! Counting is a deterministic measure of work where a stopwatch is not: a
//! path that clones history, or rebuilds a buffer sized by it, shows up as
//! bytes whatever the machine's load — which is what lets
//! `app::tests::session_length_tests` hold per-trade, per-depth and per-frame
//! work to a budget in the ordinary test run without a wall-clock assertion.
//! Per thread, because tests run in parallel: a tally only ever sees the work
//! of the thread it is read on.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Heap work done by one thread since it started.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// Fresh allocations (`alloc`, `alloc_zeroed`).
    pub allocs: u64,
    /// Bytes those allocations asked for.
    pub alloc_bytes: u64,
    /// Reallocations: a buffer that grew or shrank.
    pub reallocs: u64,
    /// Bytes a reallocation may copy: the smaller of the old and new size,
    /// summed. An upper bound — an allocator that grows in place copies none.
    pub realloc_copy_bytes: u64,
    /// The largest single reallocation copy seen: the one stall a growing
    /// buffer can put on its thread.
    pub largest_realloc_copy: u64,
}

impl Tally {
    const ZERO: Self = Self {
        allocs: 0,
        alloc_bytes: 0,
        reallocs: 0,
        realloc_copy_bytes: 0,
        largest_realloc_copy: 0,
    };

    /// The work done between `earlier` and `self`, both read on one thread.
    /// The largest copy is the largest seen since the thread started, so a
    /// caller that needs it per window resets it with [`reset_largest`].
    #[must_use]
    pub(crate) fn since(self, earlier: Self) -> Self {
        Self {
            allocs: self.allocs - earlier.allocs,
            alloc_bytes: self.alloc_bytes - earlier.alloc_bytes,
            reallocs: self.reallocs - earlier.reallocs,
            realloc_copy_bytes: self.realloc_copy_bytes - earlier.realloc_copy_bytes,
            largest_realloc_copy: self.largest_realloc_copy,
        }
    }
}

thread_local! {
    // `const`-initialised and free of destructors, so reading it from inside
    // the allocator never allocates or registers anything.
    static TALLY: Cell<Tally> = const { Cell::new(Tally::ZERO) };
}

/// This thread's heap work so far.
#[must_use]
pub(crate) fn tally() -> Tally {
    TALLY.try_with(Cell::get).unwrap_or_default()
}

/// Forget the largest reallocation this thread has seen, so the next
/// [`tally`] reports the largest within a window.
pub(crate) fn reset_largest() {
    let _ = TALLY.try_with(|cell| {
        let mut tally = cell.get();
        tally.largest_realloc_copy = 0;
        cell.set(tally);
    });
}

fn record(update: impl FnOnce(&mut Tally)) {
    // `try_with`: a thread being torn down may still free memory.
    let _ = TALLY.try_with(|cell| {
        let mut tally = cell.get();
        update(&mut tally);
        cell.set(tally);
    });
}

/// The system allocator, tallying per thread.
pub(crate) struct Counting;

// SAFETY: every call is forwarded unchanged to `System`, which upholds the
// `GlobalAlloc` contract; the tally only reads sizes and touches a
// thread-local `Cell` that never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|tally| {
            tally.allocs += 1;
            tally.alloc_bytes += layout.size() as u64;
        });
        // SAFETY: forwarded with the caller's own layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|tally| {
            tally.allocs += 1;
            tally.alloc_bytes += layout.size() as u64;
        });
        // SAFETY: forwarded with the caller's own layout.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, i.e. from `System`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let copy = layout.size().min(new_size) as u64;
        record(|tally| {
            tally.reallocs += 1;
            tally.realloc_copy_bytes += copy;
            tally.largest_realloc_copy = tally.largest_realloc_copy.max(copy);
        });
        // SAFETY: `ptr` and `layout` came from this allocator, i.e. `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tally_sees_its_own_threads_work_and_no_other() {
        let before = tally();
        let boxed = std::hint::black_box(vec![0_u8; 4_096]);
        let mut grown: Vec<u64> = Vec::with_capacity(4);
        for value in 0..64 {
            grown.push(std::hint::black_box(value));
        }
        let mine = tally().since(before);
        assert!(mine.allocs >= 2, "{mine:?}");
        assert!(mine.alloc_bytes >= 4_096 + 32, "{mine:?}");
        assert!(mine.reallocs >= 1, "{mine:?}");
        assert!(mine.realloc_copy_bytes >= 32, "{mine:?}");
        drop((boxed, grown));

        let before = tally();
        std::thread::spawn(|| std::hint::black_box(vec![0_u8; 1 << 20]))
            .join()
            .expect("the helper thread finishes");
        let mine = tally().since(before);
        assert!(
            mine.alloc_bytes < 1 << 20,
            "another thread's megabyte is not this thread's work: {mine:?}"
        );
    }
}
