//! Exact retained ordinal ranges from the canonical tick builder's decisions.
use std::ops::Range;

#[derive(Debug, Default)]
pub struct TickMembership {
    closed: Vec<Range<usize>>,
    start: usize,
    next: usize,
    openings: quantick_orderflow::history::RecordedOpenings,
}

impl TickMembership {
    pub(crate) fn observe(
        &mut self,
        admitted: bool,
        closed: bool,
        included: bool,
        timestamp_ms: i64,
    ) {
        self.openings.observe(timestamp_ms);
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
        state.set_footprint_enabled(true);
        assert_eq!(state.tick_membership().unwrap().range(1), Some(2..4));
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.tick_membership().unwrap().range(1), Some(3..5));
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
    }
}
