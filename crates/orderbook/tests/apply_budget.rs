//! `OrderBook::apply_delta`'s heap work, counted and held to a budget.
//!
//! Every live depth event goes through `apply_delta`, so a per-event clone of
//! the book is paid on the feed's hot path for as long as the book is deep.
//! This binary applies a deterministic stream of depth events to a deep book
//! and counts the heap work of the applies alone: allocations, bytes
//! allocated and bytes a reallocation may copy, per event, after a warm-up.
//! Counts do not move with the host's load, so the budget holds in the
//! ordinary `cargo test`. The counter is local because `orderbook` may not
//! depend on `engine`, whose `work_meter` the engine's budgets use.
//!
//! The budget is declared first, with the calibration it was set from, and
//! never read back from a measurement.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use quantick_orderbook::{
    ApplyOutcome, BookCoverage, BookDelta, BookLevel, BookSnapshot, OrderBook,
};
use rust_decimal::Decimal;

/// Heap work done on one thread.
#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    allocs: u64,
    alloc_bytes: u64,
    realloc_copy_bytes: u64,
}

impl Tally {
    fn since(self, earlier: Self) -> Self {
        Self {
            allocs: self.allocs - earlier.allocs,
            alloc_bytes: self.alloc_bytes - earlier.alloc_bytes,
            realloc_copy_bytes: self.realloc_copy_bytes - earlier.realloc_copy_bytes,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            allocs: self.allocs + other.allocs,
            alloc_bytes: self.alloc_bytes + other.alloc_bytes,
            realloc_copy_bytes: self.realloc_copy_bytes + other.realloc_copy_bytes,
        }
    }
}

thread_local! {
    // `const`-initialised and free of destructors, so the allocator can read
    // it without allocating.
    static TALLY: Cell<Tally> = const {
        Cell::new(Tally { allocs: 0, alloc_bytes: 0, realloc_copy_bytes: 0 })
    };
}

fn tally() -> Tally {
    TALLY.try_with(Cell::get).unwrap_or_default()
}

fn record(update: impl FnOnce(&mut Tally)) {
    let _ = TALLY.try_with(|cell| {
        let mut tally = cell.get();
        update(&mut tally);
        cell.set(tally);
    });
}

/// The system allocator, tallying per thread.
struct Counting;

// SAFETY: every call is forwarded unchanged to `System`; the tally only reads
// sizes and touches a thread-local `Cell` that never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|tally| {
            tally.allocs += 1;
            tally.alloc_bytes += layout.size() as u64;
        });
        // SAFETY: forwarded with the caller's own layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, i.e. from `System`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(|tally| tally.realloc_copy_bytes += layout.size().min(new_size) as u64);
        // SAFETY: `ptr` and `layout` came from this allocator, i.e. `System`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Per-event ceilings of one `apply_delta`.
#[derive(Clone, Copy, Debug)]
struct Budget {
    /// Allocations per event.
    allocs: f64,
    /// Bytes allocated per event.
    bytes: f64,
    /// Bytes reallocation may copy per event.
    copy_bytes: f64,
}

/// Events that rewrite quantities at stored prices, both sides, sorted as a
/// venue sends them. Calibrated at `75279a76` with the per-event book clone:
/// 42 allocations and 15,680 bytes per event; without it 0, 0 and 0. Declared
/// at twice the calibration: zero, so the first apply that allocates fails.
const REWRITE: Budget = Budget {
    allocs: 0.0,
    bytes: 0.0,
    copy_bytes: 0.0,
};

/// Levels per side of the installed book: deep enough that a clone of it is
/// many tree nodes, not one.
const LEVELS_PER_SIDE: i64 = 200;
/// Levels each event rewrites per side.
const LEVELS_PER_EVENT: i64 = 10;
/// Events applied before counting starts.
const WARMUP_EVENTS: u64 = 1_000;
/// Events counted.
const MEASURED_EVENTS: u64 = 10_000;

/// Bids at 1..=200 and asks at 201..=400, one unit each.
fn deep_snapshot() -> BookSnapshot {
    let level = |price: i64| BookLevel::new(Decimal::from(price), Decimal::ONE).unwrap();
    BookSnapshot::new(
        0,
        (1..=LEVELS_PER_SIDE).map(level).collect(),
        (LEVELS_PER_SIDE + 1..=2 * LEVELS_PER_SIDE)
            .map(level)
            .collect(),
        BookCoverage::Full,
    )
}

/// Event `id`: rewrites the bids nearest the touch in descending order and
/// the asks nearest it in ascending order, the shape a venue sends.
fn rewrite_event(id: u64) -> BookDelta {
    let quantity = Decimal::from(id % 7 + 1);
    let level = |price: i64| BookLevel::new(Decimal::from(price), quantity).unwrap();
    BookDelta::new(
        id,
        id,
        (LEVELS_PER_SIDE - LEVELS_PER_EVENT + 1..=LEVELS_PER_SIDE)
            .rev()
            .map(level)
            .collect(),
        (LEVELS_PER_SIDE + 1..=LEVELS_PER_SIDE + LEVELS_PER_EVENT)
            .map(level)
            .collect(),
    )
}

#[test]
fn rewriting_stored_levels_holds_to_budget() {
    let budget = REWRITE;
    let mut book = OrderBook::new();
    book.install_snapshot(deep_snapshot()).unwrap();
    // Built before counting: constructing an event is the feed's work.
    let events: Vec<BookDelta> = (1..=WARMUP_EVENTS + MEASURED_EVENTS)
        .map(rewrite_event)
        .collect();

    let mut work = Tally::default();
    let mut first_moved: Option<(u64, Tally)> = None;
    for event in &events {
        let before = tally();
        let outcome = book.apply_delta(std::hint::black_box(event));
        let spent = tally().since(before);
        assert!(
            matches!(outcome, Ok(ApplyOutcome::Applied { .. })),
            "event {} applies: {outcome:?}",
            event.final_update_id()
        );
        if event.final_update_id() > WARMUP_EVENTS {
            work = work.add(spent);
            if first_moved.is_none() && (spent.allocs > 0 || spent.realloc_copy_bytes > 0) {
                first_moved = Some((event.final_update_id(), spent));
            }
        }
    }
    assert_eq!(book.bid_count(), LEVELS_PER_SIDE as usize);
    assert_eq!(book.ask_count(), LEVELS_PER_SIDE as usize);

    let per = |total: u64| total as f64 / MEASURED_EVENTS as f64;
    let measured = (
        per(work.allocs),
        per(work.alloc_bytes),
        per(work.realloc_copy_bytes),
    );
    println!(
        "orderbook.apply_delta.rewrite: {:.4} allocs, {:.2} bytes, {:.2} copy bytes per event over {MEASURED_EVENTS} events",
        measured.0, measured.1, measured.2
    );
    assert!(
        measured.0 <= budget.allocs
            && measured.1 <= budget.bytes
            && measured.2 <= budget.copy_bytes,
        "apply_delta over budget per event: {:.4} allocs (budget {}), {:.2} bytes (budget {}), \
         {:.2} copy bytes (budget {}); totals over {MEASURED_EVENTS} events: {work:?}; \
         first event with heap work: {first_moved:?}",
        measured.0,
        budget.allocs,
        measured.1,
        budget.bytes,
        measured.2,
        budget.copy_bytes,
    );
}
