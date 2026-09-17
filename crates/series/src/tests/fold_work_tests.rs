use crate::{SeriesFold, work_meter};
use quantick_engine::bar_registry::{BarDefinition, BarRegistry, definitions::TICK};
use quantick_engine::{
    Bar, BarBuilder, BarBuilderDiagnostics, BarSpec, Side, TickBarBuilder, Trade,
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

thread_local! {
    static DIAGNOSTICS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

static PROBE: BarDefinition = BarDefinition {
    id: "diagnostic_probe",
    factory: |value, _| Box::new(Probe(TickBarBuilder::new(value.to_u64().unwrap()))),
    ..TICK
};
struct Probe(TickBarBuilder);
impl BarBuilder for Probe {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        self.0.push(trade)
    }
    fn partial(&self) -> Option<&Bar> {
        self.0.partial()
    }
    fn diagnostics(&self) -> BarBuilderDiagnostics {
        DIAGNOSTICS.set(DIAGNOSTICS.get() + 1);
        BarBuilderDiagnostics::default()
    }
}
pub(super) fn print(id: u64) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: id as i64,
        price: Decimal::from(100 + id % 16),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

#[test]
fn disabled_capture_adds_no_diagnostic_calls_or_per_print_allocations() {
    let registry = BarRegistry::new([&PROBE]).unwrap();
    let mut fold = SeriesFold::new(registry.parse("diagnostic_probe:4").unwrap());
    DIAGNOSTICS.set(0);
    let before = work_meter::tally();
    for id in 0..10_000 {
        std::hint::black_box(fold.push(&print(id)));
    }
    let work = work_meter::tally().since(before);
    assert_eq!(DIAGNOSTICS.get(), 0);
    assert_eq!(work.allocs, 0);
    assert_eq!(work.realloc_copy_bytes, 0);
    assert_eq!(work.live_bytes, 0);
    assert!(!fold.footprint_enabled());
    assert!(fold.partial_footprint().is_none());
}

#[test]
fn enabled_capture_retains_only_the_current_ladder_and_reads_borrow_it() {
    let mut fold = SeriesFold::with_footprints(BarSpec::Tick(4), Decimal::ONE);
    for id in 0..3 {
        std::hint::black_box(fold.push(&print(id)));
    }
    let before = work_meter::tally();
    for id in 3..4_003 {
        std::hint::black_box(fold.push(&print(id)));
    }
    let first = work_meter::tally().since(before);
    for id in 4_003..8_003 {
        std::hint::black_box(fold.push(&print(id)));
    }
    let second = work_meter::tally().since(before);
    assert_eq!(first.live_bytes, 0);
    assert_eq!(
        second.live_bytes, 0,
        "closed ladders leave with their returned bars"
    );
    assert!(
        first.allocs > 0,
        "enabled BTreeMap nodes are honestly counted"
    );
    let before_reads = work_meter::tally();
    let ladder = fold.partial_footprint().unwrap();
    assert_eq!(ladder.levels().len(), 3);
    assert!(std::ptr::eq(ladder, fold.partial_footprint().unwrap()));
    assert!(std::ptr::eq(
        fold.partial().unwrap(),
        fold.partial().unwrap()
    ));
    assert_eq!(work_meter::tally().since(before_reads).allocs, 0);
}
