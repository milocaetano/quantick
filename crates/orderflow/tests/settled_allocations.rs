//! Separate executable: its allocator wrapper is absent from all timing runs.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

#[path = "support/fixtures.rs"]
mod fixtures;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    allocs: usize,
    reallocs: usize,
    deallocs: usize,
    allocated_bytes: usize,
    reallocated_bytes: usize,
    deallocated_bytes: usize,
}

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<Counts> = const { Cell::new(Counts {
        allocs: 0, reallocs: 0, deallocs: 0,
        allocated_bytes: 0, reallocated_bytes: 0, deallocated_bytes: 0,
    }) };
}

fn record(update: impl FnOnce(&mut Counts)) {
    if ENABLED.try_with(Cell::get).unwrap_or(false) {
        let _ = COUNTS.try_with(|counter| {
            let mut counts = counter.get();
            update(&mut counts);
            counter.set(counts);
        });
    }
}

struct CountingAllocator;

// SAFETY: every request is forwarded unchanged to System; bookkeeping uses
// constant-initialized thread-local Cells and cannot allocate or unwind.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|c| {
            c.allocs += 1;
            c.allocated_bytes += layout.size();
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|c| {
            c.allocs += 1;
            c.allocated_bytes += layout.size();
        });
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(|c| {
            c.deallocs += 1;
            c.deallocated_bytes += layout.size();
        });
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(|c| {
            c.reallocs += 1;
            c.reallocated_bytes += new_size;
        });
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measure(operation: impl FnOnce()) -> Counts {
    struct Disable;
    impl Drop for Disable {
        fn drop(&mut self) {
            ENABLED.set(false);
        }
    }
    assert!(!ENABLED.get(), "allocation measurement cannot nest");
    COUNTS.set(Counts::default());
    ENABLED.set(true);
    let guard = Disable;
    operation();
    drop(guard);
    COUNTS.get()
}

#[test]
fn allocation_counter_measures_all_entrypoints_and_resets() {
    let counts = measure(|| unsafe {
        let first = Layout::from_size_align(16, 8).unwrap();
        let second = Layout::from_size_align(40, 8).unwrap();
        let ptr = std::hint::black_box(std::alloc::alloc(first));
        assert!(!ptr.is_null());
        let ptr = std::hint::black_box(std::alloc::realloc(ptr, first, 40));
        assert!(!ptr.is_null());
        std::alloc::dealloc(ptr, second);
        let zero = std::hint::black_box(std::alloc::alloc_zeroed(first));
        assert!(!zero.is_null());
        assert_eq!(*zero, 0);
        std::alloc::dealloc(zero, first);
    });
    assert_eq!(
        counts,
        Counts {
            allocs: 2,
            reallocs: 1,
            deallocs: 2,
            allocated_bytes: 32,
            reallocated_bytes: 40,
            deallocated_bytes: 56,
        }
    );
    assert_eq!(measure(|| {}), Counts::default());
}

#[test]
fn allocation_counter_is_thread_local_and_unwind_safe() {
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| measure(|| drop(std::hint::black_box(Box::new([7_u8; 128])))));
        assert_eq!(measure(|| {}), Counts::default());
        let counted = worker.join().unwrap();
        assert_eq!(counted.allocs, 1);
        assert_eq!(counted.allocated_bytes, 128);
    });
    let result = std::panic::catch_unwind(|| measure(|| panic!("counter reset proof")));
    assert!(result.is_err());
    assert!(!ENABLED.get());
    assert_eq!(measure(|| {}), Counts::default());
}

#[test]
#[ignore]
fn allocations_for_frozen_projection_workloads() {
    use quantick_orderflow::{project_live, project_settled};
    for workload in 1..=3 {
        let fixture = fixtures::dense(workload);
        // W2's settled half is outside measurement, exactly as in its timer.
        let settled = project_settled(&fixture.history, &fixture.timeline, fixture.prices);
        let operation = || match workload {
            1 => drop(std::hint::black_box(fixtures::whole(&fixture))),
            2 => drop(std::hint::black_box(project_live(
                &fixture.history,
                &fixture.timeline,
                fixture.prices,
                &settled,
            ))),
            3 => drop(std::hint::black_box(project_settled(
                &fixture.history,
                &fixture.timeline,
                fixture.prices,
            ))),
            _ => unreachable!(),
        };
        for _ in 0..3 {
            operation();
        }
        let baseline = measure(operation);
        for _ in 0..4 {
            assert_eq!(
                measure(operation),
                baseline,
                "allocation counts must be deterministic"
            );
        }
        // Includes projection and result destruction. Excludes fixture,
        // warmup, this formatting, signature capture and the test harness.
        eprintln!("ALLOC W{workload} {baseline:?}");
    }
}
