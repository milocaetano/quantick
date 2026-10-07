//! A deterministic snapshot of a history recut. The caller owns scheduling.

use std::sync::Arc;

use super::ChartState;
use crate::footprint_series::{FootprintSeries, fold_print, seed_deal_counter};
use quantick_engine::Trade;
const CANCELLATION_CHECK_TRADES: usize = 4096;

/// The old tape shares immutable chunks. Scheduling this work copies at most
/// its append tail; recutting and joining the older pages happen in `run`.
pub struct HistoryRebuild {
    state: ChartState,
    pages: Vec<Arc<Vec<Trade>>>,
}

impl ChartState {
    /// Prepare a recut from newest-first older pages without changing this chart.
    #[must_use]
    pub fn history_rebuild(&self, pages: Vec<Arc<Vec<Trade>>>) -> HistoryRebuild {
        let mut state = Self::new(self.spec);
        state.trades = self.trades.clone();
        state.backfill_trade_count = self.backfill_trade_count;
        state.backfill_done = self.backfill_done;
        state.deal_samples.clone_from(&self.deal_samples);
        state.price_grid = self.price_grid.clone();
        state.tape_reference_price = self.tape_reference_price;
        state.footprint_enabled = self.footprint_enabled;
        state.footprints = FootprintSeries::for_chart(self.footprints.base_group(), &self.spec);
        state.timeline_revision = self.timeline_revision;
        state.series_revision = self.series_revision;
        state.venue_lead = self.venue_lead.carried();
        HistoryRebuild { state, pages }
    }

    /// Make an installed recut newer than every projection of the displayed chart.
    pub fn advance_history_revision(&mut self, displayed: &Self) {
        self.timeline_revision = displayed.timeline_revision.saturating_add(1);
        self.series_revision = displayed.series_revision.saturating_add(1);
    }
}

impl HistoryRebuild {
    /// Run beside the caller's frame. A cancelled result is never publishable.
    pub fn run(mut self, mut cancelled: impl FnMut() -> bool) -> Option<ChartState> {
        if cancelled() {
            return None;
        }
        let mut tape = quantick_engine::trade_tape::TradeTape::new();
        for page in self.pages.iter().rev() {
            for chunk in page.chunks(CANCELLATION_CHECK_TRADES) {
                if cancelled() {
                    return None;
                }
                tape.extend_from_slice(chunk);
                for trade in chunk {
                    self.state.observe_price(trade.price);
                }
            }
            self.state.backfill_trade_count += page.len();
        }
        for chunk in self.state.trades.slices(..) {
            if cancelled() {
                return None;
            }
            tape.extend_from_slice(chunk);
        }
        self.state.trades = tape;
        rebuild_until(&mut self.state, cancelled).then_some(self.state)
    }
}

pub(super) fn rebuild_until(state: &mut ChartState, mut cancelled: impl FnMut() -> bool) -> bool {
    state.readings_held = false;
    let mut builder = state.spec.build();
    // Readings first, prints after: the builder joins each print to the
    // newest reading strictly before it, so the order between the two
    // streams is immaterial as long as every reading is in hand.
    seed_deal_counter(&mut *builder, &state.deal_samples);
    let mut bars = Vec::new();
    let mut boundary = None;
    state.footprints.reset(state.footprints.base_group());
    state.footprints.reset_membership(&state.spec);
    let (ladders, trades) = (&mut state.footprints, &state.trades);
    let enabled = state.footprint_enabled;
    for i in 0..trades.len() {
        if i.is_multiple_of(CANCELLATION_CHECK_TRADES) && cancelled() {
            return false;
        }
        if state.backfill_done && i == state.backfill_trade_count {
            boundary = Some(bars.len());
        }
        let first = bars.len();
        let late = fold_print(&mut *builder, ladders, enabled, true, trades, i, &mut bars);
        // Live ingest's rule: a live print's late bars are history.
        if late > 0 && (!state.backfill_done || i >= state.backfill_trade_count) {
            boundary = Some(first + late);
        }
    }
    // Backfill covered every retained trade (no live yet): boundary is the
    // end of the bar list.
    if state.backfill_done && boundary.is_none() {
        boundary = Some(bars.len());
    }
    state.builder = builder;
    state.bars = bars;
    state.venue_lead.rebuilt();
    state.refresh_partial();
    state.backfill_boundary = boundary;
    state.bump_series_revision();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::{BarSpec, Side};
    use rust_decimal::Decimal;

    fn trade(id: u64) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms: id as i64 * 1000,
            price: Decimal::from(100 + id % 5),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    #[test]
    fn recut_matches_the_serial_engine_with_footprints_and_a_live_tail() {
        for spec in [
            BarSpec::Tick(3),
            BarSpec::Time(5000),
            BarSpec::Volume(Decimal::from(3)),
        ] {
            let mut displayed = ChartState::new(spec);
            displayed.set_footprint_enabled(true);
            displayed.ingest_backfill(&(10..20).map(trade).collect::<Vec<_>>());
            displayed.ingest_live(&trade(20));
            let pages = vec![
                Arc::new((5..10).map(trade).collect()),
                Arc::new((1..5).map(trade).collect()),
            ];
            let mut recut = displayed.history_rebuild(pages).run(|| false).unwrap();
            displayed.prepend_history(&(1..10).map(trade).collect::<Vec<_>>());
            recut.ingest_live(&trade(21));
            displayed.ingest_live(&trade(21));
            assert_eq!(recut.bars(), displayed.bars());
            assert_eq!(recut.partial(), displayed.partial());
            assert_eq!(recut.bar_footprints(), displayed.bar_footprints());
            assert_eq!(recut.backfill_boundary(), displayed.backfill_boundary());
            assert_eq!(
                recut.trades().iter().collect::<Vec<_>>(),
                displayed.trades().iter().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn cancellation_discards_a_partial_recut_without_touching_the_display() {
        let mut displayed = ChartState::new(BarSpec::Tick(3));
        displayed.ingest_backfill(&(10..20).map(trade).collect::<Vec<_>>());
        let mut polls = 0;
        assert!(
            displayed
                .history_rebuild(vec![Arc::new(vec![trade(9)])])
                .run(|| {
                    polls += 1;
                    polls == 4
                })
                .is_none()
        );
        assert_eq!(displayed.trades().len(), 10);
        assert_eq!(displayed.trades()[0].agg_id, 10);
    }
}
