use quantick_alerts::*;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
struct Counter;
fn note() {
    TRACK.with(|track| {
        if track.get() {
            ALLOCATIONS.with(|n| n.set(n.get() + 1));
        }
    });
}
// SAFETY: every operation delegates the exact pointer/layout contract to System.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller supplied a valid allocation layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller supplies the pointer and original layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        note();
        // SAFETY: the caller supplies a live allocation and valid new size.
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
#[test]
fn headless_extension_and_budget() {
    let market = MarketRef {
        source: SourceContext::Live {
            feed: "fixture".into(),
        },
        symbol: "BTC".into(),
        aggregation: "tick:1".into(),
    };
    let spec = AlertSpec {
        version: FORMAT_VERSION,
        id: AlertId(0),
        name: "budget".into(),
        market: market.clone(),
        conditions: vec![
            Predicate {
                operator: Operator::GreaterThan,
                target: TargetRef::Value(100.)
            };
            MAX_CONDITIONS
        ],
        frequency: Frequency::PerBar,
        expiration: Expiration::Never,
        message: String::new(),
        notification: Notification {
            popup: false,
            toast: false,
            sound: None,
            sound_seconds: None,
        },
    };
    let mut book = AlertBook::default();
    for _ in 0..MAX_ALERTS {
        book.command(AlertCommand::Create(spec.clone()), 0).unwrap();
    }
    let mut sink = BoundedEvents::new(1);
    let mut input = AlertInput {
        market: &market,
        utc_ms: 0,
        session_ms: 0,
        bar: BarIdentity {
            epoch: 1,
            sequence: 0,
        },
        kind: ObservationKind::Trade,
        coordinate: 0.,
        price: 99.,
        availability: Ok(()),
        drawings: &[],
    };
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
    let start = std::time::Instant::now();
    for sequence in 0..1000 {
        input.bar.sequence = sequence;
        book.evaluate(std::hint::black_box(&input), &mut sink);
    }
    let elapsed = start.elapsed();
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert!(sink.is_empty());
    eprintln!(
        "fixed scenario: 1000 observations x 256 alerts x 8 conditions: {elapsed:?}; allocations=0"
    );
    input.price = 101.;
    book.evaluate(&input, &mut sink);
    assert_eq!(sink.len(), 1);
    let log = book.history(0, MAX_HISTORY).unwrap();
    assert_eq!(log.len(), MAX_ALERTS);
    assert_eq!(
        log.iter()
            .filter(|e| e.delivery == DeliveryOutcome::Suppressed)
            .count(),
        MAX_ALERTS - 1
    );
    for sequence in 1000..1004 {
        input.bar.sequence = sequence;
        book.evaluate(&input, &mut sink);
    }
    assert_eq!(book.history(0, usize::MAX).unwrap().len(), MAX_HISTORY);
    assert_eq!(quantick_alerts::operators::OPERATORS.len(), 9);
}
