//! Frozen pre-extraction oracle used only by the retained tape tests.
use quantick_engine::{Bar, BarFootprint, DEFAULT_LEVEL_CAP, FootprintBuilder, Trade};
use rust_decimal::Decimal;

/// Row width used until the feed reports the instrument's real `price_step` —
/// the same fallback the order-flow capture grid uses, so the two ladders
/// agree on rows out of the box.
pub fn default_group() -> Decimal {
    Decimal::new(1, 2) // 0.01
}

/// Per-bar footprint ladders for one chart. See the [module docs](self).
pub struct FootprintSeries {
    builder: FootprintBuilder,
    closed: Vec<BarFootprint>,
    /// Trades fed since the last close — the counter the closing-trade
    /// question is answered against.
    pending: u64,
}

impl FootprintSeries {
    /// An empty series bucketing prices into rows `base_group` wide.
    #[must_use]
    pub fn new(base_group: Decimal) -> Self {
        Self {
            builder: FootprintBuilder::new(base_group, DEFAULT_LEVEL_CAP),
            closed: Vec::new(),
            pending: 0,
        }
    }

    /// Fold the trade the bar builder just consumed, `closed` being what that
    /// same `push` returned. Must be called for every trade, in order.
    pub fn observe(&mut self, trade: &Trade, closed: Option<&Bar>) {
        let Some(bar) = closed else {
            self.builder.push(trade);
            self.pending = self.pending.saturating_add(1);
            return;
        };

        let closing_trade_included = bar.trade_count == self.pending.saturating_add(1);
        debug_assert!(
            closing_trade_included || bar.trade_count == self.pending,
            "footprint trade counter drifted from the bar builder's"
        );
        if closing_trade_included {
            self.builder.push(trade);
        }
        match self.builder.close() {
            Some(ladder) => self.closed.push(ladder),
            None => {
                // Unreachable through ChartState — a closed bar summarises at
                // least one trade — but index alignment with `bars` is the
                // invariant everything downstream indexes by, so restore it
                // from the only trade at hand instead of panicking mid-feed.
                self.builder.push(trade);
                self.closed
                    .push(self.builder.close().expect("pushed just above"));
                self.pending = 0;
                return;
            }
        }
        if closing_trade_included {
            self.pending = 0;
        } else {
            // The bar closed on its boundary; this trade opens the next one.
            self.builder.push(trade);
            self.pending = 1;
        }
    }

    /// Close the ladder for `bar` without folding `trade`: the print that
    /// ended it belongs to no bar — a deal builder left it uncounted — so
    /// the ladder closes on what it held and nothing opens the next one.
    pub fn close_without(&mut self, bar: &Bar) {
        debug_assert!(
            bar.trade_count == self.pending,
            "footprint trade counter drifted from the bar builder's"
        );
        if let Some(ladder) = self.builder.close() {
            self.closed.push(ladder);
        }
        self.pending = 0;
    }

    /// One ladder per closed bar, same indices as `ChartState::bars()`.
    #[must_use]
    pub fn closed(&self) -> &[BarFootprint] {
        &self.closed
    }

    /// The forming bar's ladder, if any trade arrived since the last close.
    #[must_use]
    pub fn partial(&self) -> Option<&BarFootprint> {
        self.builder.partial()
    }
}

pub(super) fn fold_print<B: quantick_engine::BarBuilder + ?Sized>(
    builder: &mut B,
    footprints: &mut FootprintSeries,
    footprint_enabled: bool,
    trade: &Trade,
) -> Option<Bar> {
    let uncounted_before = builder.diagnostics().uncounted_trades;
    let closed = builder.push(trade);
    if footprint_enabled {
        let uncounted = builder.diagnostics().uncounted_trades != uncounted_before;
        match (&closed, uncounted) {
            (_, false) => footprints.observe(trade, closed.as_ref()),
            // A rollover ended the bar and this print counts for nothing:
            // the ladder closes on what it held, the print folds nowhere.
            (Some(bar), true) => footprints.close_without(bar),
            (None, true) => {}
        }
    }
    closed
}

pub(super) fn seed_deal_counter(
    builder: &mut dyn quantick_engine::BarBuilder,
    samples: &[quantick_engine::DealSample],
) {
    let Some(input) = builder.deal_counter_input() else {
        return;
    };
    for sample in samples {
        input.observe(*sample);
    }
}
