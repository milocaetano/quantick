//! Exact retained ordinal ranges from the canonical tick builder's decisions.
use std::ops::Range;

#[derive(Debug, Default)]
pub struct TickMembership {
    closed: Vec<Range<usize>>,
    start: usize,
    next: usize,
    openings: quantick_orderflow::history::RecordedOpenings,
    opening_anchors: quantick_orderflow::history::RecordedOpeningAnchors,
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
