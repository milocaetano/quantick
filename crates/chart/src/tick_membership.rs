//! Exact retained ordinal ranges from the canonical tick builder's decisions.
use std::ops::Range;

#[derive(Debug, Default)]
pub struct TickMembership {
    closed: Vec<Range<usize>>,
    start: usize,
    next: usize,
    openings: quantick_orderflow::history::RecordedOpenings,
    opening_anchors: quantick_orderflow::history::RecordedOpeningAnchors,
    /// Timestamp bounds follow canonical candle membership, including late prints.
    times: Vec<(i64, i64)>,
    latest: Option<i64>,
    unordered_times: bool,
}

impl TickMembership {
    /// Whether bars of `spec` keep exact membership: only fixed tick bars,
    /// which admit every print.
    #[must_use]
    pub fn applies_to(spec: &quantick_engine::bar_registry::BarConfiguration) -> bool {
        spec.id() == "tick"
    }

    pub(crate) fn observe(
        &mut self,
        admitted: bool,
        closed: bool,
        included: bool,
        trade: &quantick_engine::Trade,
    ) {
        if trade.quantity > rust_decimal::Decimal::ZERO {
            self.openings.observe(trade.timestamp_ms);
            self.opening_anchors.observe(trade.timestamp_ms, self.next);
        }
        // This owner is installed only for fixed tick bars, which admit every print.
        debug_assert!(admitted);
        if closed && !included {
            self.closed.push(self.start..self.next);
            self.start = self.next;
        }
        if let Some(bounds) = self.times.get_mut(self.closed.len()) {
            bounds.0 = bounds.0.min(trade.timestamp_ms);
            bounds.1 = bounds.1.max(trade.timestamp_ms);
        } else {
            self.times.push((trade.timestamp_ms, trade.timestamp_ms));
        }
        self.latest = Some(
            self.latest
                .map_or(trade.timestamp_ms, |latest| latest.max(trade.timestamp_ms)),
        );
        if self.times.len() > 1 {
            let previous = self.times[self.times.len() - 2];
            let current = self.times[self.times.len() - 1];
            self.unordered_times |= current.0 < previous.0 || current.1 < previous.1;
        }
        self.next += 1;
        if closed && included {
            self.closed.push(self.start..self.next);
            self.start = self.next;
        }
    }

    /// Canonical slot and accepted ordinal for one retained source ordinal.
    #[must_use]
    pub fn locate(&self, ordinal: usize) -> Option<(usize, usize)> {
        let slot = self.closed.partition_point(|range| range.end <= ordinal);
        let range = self.range(slot)?;
        range
            .contains(&ordinal)
            .then_some((slot, ordinal - range.start))
    }

    pub fn opening_windows(&self) -> &[i64] {
        self.openings.windows()
    }

    /// Time extrema of whole candles; source ordinals need not be time-sorted.
    pub fn time_bounds(&self, slots: Range<usize>) -> Option<(i64, i64)> {
        let times = self.times.get(slots)?;
        if !self.unordered_times {
            return Some((times.first()?.0, times.last()?.1));
        }
        times
            .iter()
            .copied()
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }

    /// Conservative whole-candle admission for a half-open time interval.
    pub fn slots_at_times(&self, from: i64, until: i64) -> Option<Range<usize>> {
        if !self.unordered_times {
            let first = self.times.partition_point(|bounds| bounds.1 < from);
            let end = self.times.partition_point(|bounds| bounds.0 < until);
            return (first < end).then_some(first..end);
        }
        let mut matches = self
            .times
            .iter()
            .enumerate()
            .filter_map(|(slot, &(low, high))| (high >= from && low < until).then_some(slot));
        let first = matches.next()?;
        Some(first..matches.next_back().unwrap_or(first) + 1)
    }

    pub fn latest_timestamp_ms(&self) -> Option<i64> {
        self.latest
    }

    pub fn opening_ordinals(&self) -> impl Iterator<Item = usize> + '_ {
        self.opening_anchors.ordinals()
    }

    #[must_use]
    pub fn opening(&self, timestamp_ms: i64) -> bool {
        self.openings.contains(timestamp_ms)
    }

    #[must_use]
    pub fn range(&self, slot: usize) -> Option<Range<usize>> {
        self.closed.get(slot).cloned().or_else(|| {
            (slot == self.closed.len() && self.next > self.start).then_some(self.start..self.next)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{BarSpec, ChartState};
    use quantick_engine::{Side, Trade};
    use rust_decimal::Decimal;

    #[test]
    fn same_millisecond_and_restarted_ids_keep_exact_tick_membership() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        for _ in 0..5 {
            state.ingest_live(&Trade {
                agg_id: 1,
                timestamp_ms: 1000,
                price: Decimal::from(100),
                quantity: Decimal::ONE,
                side: Side::Buy,
            });
        }
        let members = state.tick_membership().unwrap();
        assert_eq!(members.range(0), Some(0..2));
        assert_eq!(members.range(1), Some(2..4));
        assert_eq!(members.range(2), Some(4..5));
        assert_eq!(members.opening_ordinals().collect::<Vec<_>>(), [0]);
        state.set_footprint_enabled(true);
        assert_eq!(state.tick_membership().unwrap().range(1), Some(2..4));
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.tick_membership().unwrap().range(1), Some(3..5));
        assert_eq!(
            state
                .tick_membership()
                .unwrap()
                .opening_ordinals()
                .collect::<Vec<_>>(),
            [0]
        );
    }
    #[test]
    fn partial_close_refold_prepend_and_source_reset_keep_canonical_membership() {
        let print = Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: 100.into(),
            quantity: Decimal::ONE,
            side: Side::Buy,
        };
        let mut state = ChartState::new(BarSpec::Tick(3));
        state.ingest_live(&print);
        state.ingest_live(&print);
        assert_eq!(state.tick_membership().unwrap().locate(1), Some((0, 1)));
        state.ingest_live(&print);
        state.ingest_live(&print);
        assert_eq!(state.tick_membership().unwrap().range(0), Some(0..3));
        assert_eq!(state.tick_membership().unwrap().locate(3), Some((1, 0)));
        state.set_footprint_enabled(true);
        state.set_footprint_enabled(false);
        assert_eq!(state.tick_membership().unwrap().range(1), Some(3..4));
        let epoch = state.series_revision();
        state.prepend_history(std::slice::from_ref(&print));
        assert!(state.series_revision() > epoch);
        assert_eq!(state.tick_membership().unwrap().range(1), Some(3..5));
        state.set_spec(BarSpec::Volume(Decimal::from(3)));
        assert!(state.tick_membership().is_none());
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.tick_membership().unwrap().range(1), Some(3..5));
        let epoch = state.series_revision();
        state.reset_series(BarSpec::Tick(3));
        assert!(state.series_revision() > epoch);
        assert!(state.tick_membership().unwrap().range(0).is_none());
        assert_eq!(
            state.tick_membership().unwrap().opening_ordinals().count(),
            0
        );
    }

    #[test]
    fn earlier_prepended_history_reanchors_the_rebuilt_source_epoch() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        let mut print = Trade {
            agg_id: 1,
            timestamp_ms: 1050,
            price: 100.into(),
            quantity: Decimal::ONE,
            side: Side::Buy,
        };
        state.ingest_live(&print);
        print.timestamp_ms = 1010;
        state.ingest_live(&print);
        assert_eq!(
            state
                .tick_membership()
                .unwrap()
                .opening_ordinals()
                .collect::<Vec<_>>(),
            [1]
        );
        let epoch = state.series_revision();
        print.timestamp_ms = 900;
        state.prepend_history(&[print]);
        assert!(state.series_revision() > epoch);
        assert_eq!(state.tick_membership().unwrap().opening_windows(), &[900]);
        assert_eq!(
            state
                .tick_membership()
                .unwrap()
                .opening_ordinals()
                .collect::<Vec<_>>(),
            [0]
        );
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(
            state
                .tick_membership()
                .unwrap()
                .opening_ordinals()
                .collect::<Vec<_>>(),
            [0]
        );
    }

    #[test]
    fn nonpositive_prefix_keeps_tick_membership_but_first_positive_print_nominates_the_day() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        for (timestamp_ms, quantity) in [(1000, 0), (1001, -2), (1100, 5), (1101, 6)] {
            state.ingest_live(&Trade {
                agg_id: 1,
                timestamp_ms,
                price: 100.into(),
                quantity: quantity.into(),
                side: Side::Buy,
            });
        }
        for tick_size in [2, 3] {
            state.set_spec(BarSpec::Tick(tick_size));
            let membership = state.tick_membership().unwrap();
            assert_eq!(membership.opening_windows(), &[1100]);
            assert_eq!(membership.opening_ordinals().collect::<Vec<_>>(), [2]);
            assert_eq!(membership.locate(0), Some((0, 0)));
            assert!(membership.locate(3).is_some());
        }
    }
}
