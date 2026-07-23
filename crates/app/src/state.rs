//! Chart state: trades in (backfill + live), bars out.
//!
//! This is the app's side of the "one engine, three consumers" boundary. It
//! owns a single [`BarBuilder`] and feeds it both the backfilled history and the
//! live trades through the *same* code path — the chart is just another engine
//! consumer. It also records where backfilled data ends and live data begins so
//! the two can be labelled honestly on screen (never silently merged).
//!
//! No egui, no async here, so the ingest logic is unit-tested in CI.

use quantick_engine::{Bar, BarBuilder, TickBarBuilder, Trade};

/// The bars derived from the trade stream, plus the backfill/live boundary.
pub struct ChartState {
    builder: TickBarBuilder,
    bars: Vec<Bar>,
    partial: Option<Bar>,
    /// Number of bars that closed purely from backfilled trades. Bars at this
    /// index and beyond contain at least one live trade. `None` until backfill
    /// has been ingested.
    backfill_boundary: Option<usize>,
}

impl ChartState {
    /// A fresh chart building tick bars of `tick_size` trades each.
    #[must_use]
    pub fn new(tick_size: u64) -> Self {
        Self {
            builder: TickBarBuilder::new(tick_size),
            bars: Vec::new(),
            partial: None,
            backfill_boundary: None,
        }
    }

    /// Ingest the backfilled history as one batch, then mark the boundary.
    ///
    /// The boundary is the count of bars fully formed from backfill. Any bar
    /// still forming when backfill ends will be completed by live trades, so it
    /// counts as live (it sits at or past the boundary).
    pub fn ingest_backfill(&mut self, trades: &[Trade]) {
        for trade in trades {
            self.push(trade);
        }
        self.backfill_boundary = Some(self.bars.len());
        self.refresh_partial();
    }

    /// Ingest one live trade.
    pub fn ingest_live(&mut self, trade: &Trade) {
        self.push(trade);
        self.refresh_partial();
    }

    fn push(&mut self, trade: &Trade) {
        if let Some(bar) = self.builder.push(trade) {
            self.bars.push(bar);
        }
    }

    fn refresh_partial(&mut self) {
        self.partial = self.builder.partial().cloned();
    }

    /// The closed bars.
    #[must_use]
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// The forming (in-progress) bar, if any.
    #[must_use]
    pub fn partial(&self) -> Option<&Bar> {
        self.partial.as_ref()
    }

    /// The number of purely-backfilled bars (the backfill/live divider index).
    #[must_use]
    pub fn backfill_boundary(&self) -> Option<usize> {
        self.backfill_boundary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use rust_decimal::Decimal;

    fn trade(agg_id: u64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    #[test]
    fn backfill_and_live_go_through_the_same_builder() {
        let mut s = ChartState::new(2); // close every 2 trades
        // 3 backfill trades -> 1 closed bar + a 1-trade partial.
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]);
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(1));
        assert!(s.partial().is_some());

        // A live trade extends the partial to 2 trades, closing bar index 1 —
        // the boundary bar, made of backfill trade 3 + live trade 4.
        s.ingest_live(&trade(4));
        assert_eq!(s.bars().len(), 2);
        assert_eq!(
            s.backfill_boundary(),
            Some(1),
            "boundary marks where live begins and does not move"
        );
    }

    #[test]
    fn boundary_is_none_before_backfill() {
        let mut s = ChartState::new(5);
        s.ingest_live(&trade(1));
        assert_eq!(s.backfill_boundary(), None);
    }

    #[test]
    fn empty_backfill_sets_a_zero_boundary() {
        let mut s = ChartState::new(5);
        s.ingest_backfill(&[]);
        assert_eq!(s.backfill_boundary(), Some(0));
        assert!(s.bars().is_empty());
    }
}
