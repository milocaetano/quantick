//! Exact opening candidates with their original date and bar-slot provenance.
//! Earlier arrivals retract only the former candidate for that UTC date.
//! Historical provenance lives as long as the series; readback keeps its
//! existing bounded window list. No trades or candle boundaries are replayed.
use std::collections::{BTreeMap, BTreeSet};

use quantick_engine::{BarFootprint, DEFAULT_LEVEL_CAP, FootprintBuilder, ProfileFold, Trade};
use quantick_orderflow::history::RecordedOpenings;
use rust_decimal::Decimal;

const UTC_DAY_MS: i64 = 86_400_000;

struct DateOpening {
    window: i64,
    slots: BTreeSet<usize>,
}

pub(super) struct OpeningSeries {
    group: Decimal,
    dates: BTreeMap<i64, DateOpening>,
    by_slot: BTreeMap<usize, BTreeMap<i64, BarFootprint>>,
    partial_dates: BTreeMap<i64, FootprintBuilder>,
    partial: FootprintBuilder,
    closed: BTreeMap<usize, BarFootprint>,
    recorded: RecordedOpenings,
}

impl OpeningSeries {
    pub fn new(group: Decimal) -> Self {
        Self {
            group,
            dates: BTreeMap::new(),
            by_slot: BTreeMap::new(),
            partial_dates: BTreeMap::new(),
            partial: FootprintBuilder::new(group, DEFAULT_LEVEL_CAP),
            closed: BTreeMap::new(),
            recorded: RecordedOpenings::default(),
        }
    }

    pub fn observe(&mut self, trade: &Trade) {
        self.recorded.observe(trade.timestamp_ms);
        let window = RecordedOpenings::window_start(trade.timestamp_ms);
        let day = window.div_euclid(UTC_DAY_MS);
        let date = self.dates.entry(day).or_insert_with(|| DateOpening {
            window,
            slots: BTreeSet::new(),
        });
        if window < date.window {
            date.window = window;
            let slots = std::mem::take(&mut date.slots);
            self.retract(day, slots);
        }
        if self.dates[&day].window != window {
            return;
        }
        self.partial.push(trade);
        self.partial_dates
            .entry(day)
            .or_insert_with(|| FootprintBuilder::new(self.group, DEFAULT_LEVEL_CAP))
            .push(trade);
    }

    fn retract(&mut self, day: i64, slots: BTreeSet<usize>) {
        for slot in slots {
            let contributions = self
                .by_slot
                .get_mut(&slot)
                .expect("opening slot provenance");
            contributions.remove(&day);
            let mut rebuilt = Self::refold(self.group, contributions.values());
            if let Some(ladder) = rebuilt.close() {
                self.closed.insert(slot, ladder);
            } else {
                self.closed.remove(&slot);
                self.by_slot.remove(&slot);
            }
        }
        if self.partial_dates.remove(&day).is_some() {
            self.partial = Self::refold(
                self.group,
                self.partial_dates
                    .values()
                    .filter_map(FootprintBuilder::partial),
            );
        }
    }

    fn refold<'a>(
        group: Decimal,
        ladders: impl IntoIterator<Item = &'a BarFootprint>,
    ) -> FootprintBuilder {
        let mut fold = ProfileFold::new(group, DEFAULT_LEVEL_CAP);
        for ladder in ladders {
            assert!(
                fold.push_ladder(ladder),
                "opening groups share the series grid"
            );
        }
        fold.into_footprint_builder()
    }

    pub fn close(&mut self, slot: usize) {
        let Some(ladder) = self.partial.close() else {
            return;
        };
        self.closed.insert(slot, ladder);
        let contributions = std::mem::take(&mut self.partial_dates)
            .into_iter()
            .filter_map(|(day, mut builder)| builder.close().map(|ladder| (day, ladder)))
            .collect::<BTreeMap<_, _>>();
        for day in contributions.keys() {
            self.dates
                .get_mut(day)
                .expect("opening date provenance")
                .slots
                .insert(slot);
        }
        self.by_slot.insert(slot, contributions);
    }

    pub fn closed(&self) -> &BTreeMap<usize, BarFootprint> {
        &self.closed
    }

    pub fn partial(&self) -> Option<&BarFootprint> {
        self.partial.partial()
    }

    pub fn recorded_windows(&self) -> &[i64] {
        self.recorded.windows()
    }
}
