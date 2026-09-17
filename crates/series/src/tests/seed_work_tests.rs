//! SER1-PERF-001: lifecycle seeding acquires the optional input once, not per reading.
use super::fold_work_tests::print;
use crate::RetainedSeries;
use quantick_engine::bar_registry::{BarDefinition, BarRegistry, definitions::TICK};
use quantick_engine::{Bar, BarBuilder, DealCounterInput, DealSample, TickBarBuilder, Trade};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::cell::{Cell, RefCell};

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Reading(DealSample),
    Print(u64),
}
thread_local! {
    static LOOKUPS: Cell<usize> = const { Cell::new(0) };
    static EVENTS: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

struct SeedProbe {
    ticks: TickBarBuilder,
    accepts_readings: bool,
}
impl DealCounterInput for SeedProbe {
    fn observe(&mut self, sample: DealSample) {
        EVENTS.with_borrow_mut(|events| events.push(Event::Reading(sample)));
    }
}
impl BarBuilder for SeedProbe {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        EVENTS.with_borrow_mut(|events| events.push(Event::Print(trade.agg_id)));
        self.ticks.push(trade)
    }
    fn partial(&self) -> Option<&Bar> {
        self.ticks.partial()
    }
    fn deal_counter_input(&mut self) -> Option<&mut dyn DealCounterInput> {
        LOOKUPS.set(LOOKUPS.get() + 1);
        if self.accepts_readings {
            Some(self)
        } else {
            None
        }
    }
}

static ABSENT: BarDefinition = BarDefinition {
    id: "seed_absent",
    factory: |value, _| {
        Box::new(SeedProbe {
            ticks: TickBarBuilder::new(value.to_u64().unwrap()),
            accepts_readings: false,
        })
    },
    ..TICK
};
static SUPPORTED: BarDefinition = BarDefinition {
    id: "seed_supported",
    factory: |value, _| {
        Box::new(SeedProbe {
            ticks: TickBarBuilder::new(value.to_u64().unwrap()),
            accepts_readings: true,
        })
    },
    ..TICK
};

const fn reading(time_ms: i64, session_deals: u64) -> DealSample {
    DealSample {
        time_ms,
        session_deals,
    }
}
// Stable order within a millisecond, deduplication and rollover are observable
// through the real retained batch path, not a direct call to the future helper.
const BATCH: [DealSample; 5] = [
    reading(20, 20),
    reading(10, 10),
    reading(20, 18),
    reading(20, 20),
    reading(30, 1),
];
const ORDERED: [DealSample; 4] = [
    reading(10, 10),
    reading(20, 20),
    reading(20, 18),
    reading(30, 1),
];

#[derive(Clone, Copy, Debug)]
enum Lifecycle {
    Reset,
    Rebuild,
    Refold,
}

fn lifecycle_seed_queries_once(lifecycle: Lifecycle, supported: bool) {
    let definition = if supported { &SUPPORTED } else { &ABSENT };
    let registry = BarRegistry::new([definition]).unwrap();
    let spec = registry.parse(&format!("{}:2", definition.id)).unwrap();
    let mut counts = Vec::new();
    for batch in [&[][..], &BATCH[..]] {
        let mut series = RetainedSeries::new(spec);
        if matches!(lifecycle, Lifecycle::Refold) {
            series.set_footprint_enabled(true);
        }
        series.observe_deals_batch(batch);
        series.ingest_backfill(&[print(0), print(1)]);
        LOOKUPS.set(0);
        EVENTS.with_borrow_mut(Vec::clear);
        match lifecycle {
            Lifecycle::Reset => series.reset_series(spec),
            Lifecycle::Rebuild => series.rebuild_bars(),
            Lifecycle::Refold => series.set_footprint_group(Decimal::ONE),
        }
        counts.push(LOOKUPS.get());
        let expected_readings = if batch.is_empty() {
            &[][..]
        } else {
            &ORDERED[..]
        };
        assert_eq!(series.deal_samples(), expected_readings);
        let mut expected = Vec::new();
        if supported {
            expected.extend(expected_readings.iter().copied().map(Event::Reading));
        }
        if !matches!(lifecycle, Lifecycle::Reset) {
            expected.extend([Event::Print(0), Event::Print(1)]);
        }
        EVENTS.with_borrow(|events| assert_eq!(*events, expected, "{lifecycle:?}"));
    }
    assert_eq!(
        counts,
        [1, 1],
        "{lifecycle:?}: empty and nonempty batches each query once"
    );
}

#[test]
fn reset_queries_absent_input_once() {
    lifecycle_seed_queries_once(Lifecycle::Reset, false);
}
#[test]
fn reset_queries_supported_input_once_in_retained_order() {
    lifecycle_seed_queries_once(Lifecycle::Reset, true);
}
#[test]
fn rebuild_queries_absent_input_once() {
    lifecycle_seed_queries_once(Lifecycle::Rebuild, false);
}
#[test]
fn rebuild_queries_supported_input_once_before_prints() {
    lifecycle_seed_queries_once(Lifecycle::Rebuild, true);
}
#[test]
fn refold_queries_absent_input_once() {
    lifecycle_seed_queries_once(Lifecycle::Refold, false);
}
#[test]
fn refold_queries_supported_input_once_before_prints() {
    lifecycle_seed_queries_once(Lifecycle::Refold, true);
}
