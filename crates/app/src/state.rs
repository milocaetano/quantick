//! Chart state: trades in (backfill + live), bars out — for any bar type.
//!
//! This is the app's side of the "one engine, four consumers" boundary. It
//! retains every trade and feeds them through whichever [`BarBuilder`] the user
//! has selected; switching the bar type rebuilds the bars from the retained
//! trades through a freshly configured builder — the same deterministic engine
//! code path, just a different measure. It also records where backfilled data
//! ends and live data begins so the two can be labelled honestly.
//!
//! No egui, no async here, so the ingest, dispatch and rebuild logic is
//! unit-tested in CI.

use quantick_engine::{
    Bar, BarBuilder, DollarBarBuilder, ImbalanceBarBuilder, TickBarBuilder, TimeBarBuilder, Trade,
    VolumeBarBuilder,
};
use rust_decimal::Decimal;

/// Which alternative bar type the chart is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
    /// Close every N trades.
    Tick,
    /// Close every N units of traded quantity.
    Volume,
    /// Close every N notional (price × quantity).
    Dollar,
    /// Close every N milliseconds of trade time.
    Time,
    /// Close when aggressor imbalance beats an adaptive threshold
    /// (López de Prado tick imbalance bars).
    Imbalance,
}

impl BarKind {
    /// All bar kinds, for building a selector.
    pub const ALL: [BarKind; 5] = [
        BarKind::Tick,
        BarKind::Volume,
        BarKind::Dollar,
        BarKind::Time,
        BarKind::Imbalance,
    ];

    /// A short display label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            BarKind::Tick => "tick",
            BarKind::Volume => "volume",
            BarKind::Dollar => "dollar",
            BarKind::Time => "time",
            BarKind::Imbalance => "imbalance",
        }
    }
}

/// A bar type together with its threshold parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarSpec {
    /// N trades per bar.
    Tick(u64),
    /// N units of quantity per bar.
    Volume(Decimal),
    /// N notional per bar.
    Dollar(Decimal),
    /// N milliseconds per bar.
    Time(i64),
    /// Target trades per bar for the adaptive imbalance rule.
    Imbalance(u64),
}

impl BarSpec {
    /// The kind, discarding the parameter.
    #[must_use]
    pub fn kind(&self) -> BarKind {
        match self {
            BarSpec::Tick(_) => BarKind::Tick,
            BarSpec::Volume(_) => BarKind::Volume,
            BarSpec::Dollar(_) => BarKind::Dollar,
            BarSpec::Time(_) => BarKind::Time,
            BarSpec::Imbalance(_) => BarKind::Imbalance,
        }
    }

    /// Construct the matching engine builder. This is the whole "bar type →
    /// builder" dispatch: one place, four consumers of the same engine.
    #[must_use]
    pub fn build(&self) -> Box<dyn BarBuilder> {
        match self {
            BarSpec::Tick(n) => Box::new(TickBarBuilder::new(*n)),
            BarSpec::Volume(units) => Box::new(VolumeBarBuilder::new(*units)),
            BarSpec::Dollar(notional) => Box::new(DollarBarBuilder::new(*notional)),
            BarSpec::Time(ms) => Box::new(TimeBarBuilder::new(*ms)),
            BarSpec::Imbalance(target) => Box::new(ImbalanceBarBuilder::new(*target)),
        }
    }

    /// A human-readable summary, e.g. `tick(50)` or `dollar(500000)`.
    #[must_use]
    pub fn summary(&self) -> String {
        match self {
            BarSpec::Tick(n) => format!("tick({n})"),
            BarSpec::Volume(u) => format!("volume({u})"),
            BarSpec::Dollar(d) => format!("dollar({d})"),
            BarSpec::Time(ms) => format!("time({ms}ms)"),
            BarSpec::Imbalance(target) => format!("imbalance({target})"),
        }
    }
}

/// The bars derived from the retained trade stream, plus the backfill/live
/// boundary, for the currently selected [`BarSpec`].
pub struct ChartState {
    spec: BarSpec,
    builder: Box<dyn BarBuilder>,
    trades: Vec<Trade>,
    backfill_trade_count: usize,
    backfill_done: bool,
    bars: Vec<Bar>,
    partial: Option<Bar>,
    backfill_boundary: Option<usize>,
}

impl ChartState {
    /// A fresh chart building bars per `spec`.
    #[must_use]
    pub fn new(spec: BarSpec) -> Self {
        let builder = spec.build();
        Self {
            spec,
            builder,
            trades: Vec::new(),
            backfill_trade_count: 0,
            backfill_done: false,
            bars: Vec::new(),
            partial: None,
            backfill_boundary: None,
        }
    }

    /// Ingest the backfilled history as one batch (call once, before any live
    /// trades), then mark the boundary.
    pub fn ingest_backfill(&mut self, trades: &[Trade]) {
        self.trades.extend_from_slice(trades);
        self.backfill_trade_count = self.trades.len();
        self.backfill_done = true;
        for trade in trades {
            if let Some(bar) = self.builder.push(trade) {
                self.bars.push(bar);
            }
        }
        self.backfill_boundary = Some(self.bars.len());
        self.refresh_partial();
    }

    /// Prepend older backfilled history to the front of the retained stream.
    ///
    /// `trades` must be strictly older than everything already retained (the
    /// feed guarantees this by paging backward from the earliest known
    /// `agg_id`). Because count-based bars (tick/volume/dollar) are grouped from
    /// the first trade, adding older trades re-aligns every bar — so this rebuilds
    /// the whole series through the same deterministic engine path rather than
    /// pretending the existing bars are untouched (data-honesty rule). The
    /// backfill/live boundary is recomputed. Returns how many net bars were added
    /// so the caller can keep the visible window steady.
    pub fn prepend_history(&mut self, trades: &[Trade]) -> usize {
        if trades.is_empty() {
            return 0;
        }
        let bars_before = self.bars.len();
        let mut combined = Vec::with_capacity(trades.len() + self.trades.len());
        combined.extend_from_slice(trades);
        combined.append(&mut self.trades);
        self.trades = combined;
        self.backfill_trade_count += trades.len();
        self.rebuild();
        self.bars.len().saturating_sub(bars_before)
    }

    /// Ingest one live trade, incrementally (no full rebuild).
    pub fn ingest_live(&mut self, trade: &Trade) {
        self.trades.push(trade.clone());
        if let Some(bar) = self.builder.push(trade) {
            self.bars.push(bar);
        }
        self.refresh_partial();
    }

    /// Switch the bar type/parameter, rebuilding all bars from the retained
    /// trades. A no-op if `spec` is unchanged.
    pub fn set_spec(&mut self, spec: BarSpec) {
        if spec == self.spec {
            return;
        }
        self.spec = spec;
        self.rebuild();
    }

    /// Replay every retained trade through a fresh builder for the current spec,
    /// recomputing the bars and the backfill/live boundary.
    fn rebuild(&mut self) {
        let mut builder = self.spec.build();
        let mut bars = Vec::new();
        let mut boundary = None;
        for (i, trade) in self.trades.iter().enumerate() {
            if self.backfill_done && i == self.backfill_trade_count {
                boundary = Some(bars.len());
            }
            if let Some(bar) = builder.push(trade) {
                bars.push(bar);
            }
        }
        // Backfill covered every retained trade (no live yet): boundary is the
        // end of the bar list.
        if self.backfill_done && boundary.is_none() {
            boundary = Some(bars.len());
        }
        self.partial = builder.partial().cloned();
        self.builder = builder;
        self.bars = bars;
        self.backfill_boundary = boundary;
    }

    fn refresh_partial(&mut self) {
        self.partial = self.builder.partial().cloned();
    }

    /// The current bar spec.
    #[must_use]
    pub fn spec(&self) -> &BarSpec {
        &self.spec
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
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn trade(agg_id: u64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: dec("100"),
            quantity: dec("1.0"),
            side: Side::Buy,
        }
    }

    #[test]
    fn build_dispatches_every_kind() {
        // Tick(1) and Imbalance(1) close on the first trade; the others simply
        // must build and accept a trade without panicking.
        for spec in [
            BarSpec::Tick(1),
            BarSpec::Volume(dec("1.0")),
            BarSpec::Dollar(dec("100")),
            BarSpec::Time(1),
            BarSpec::Imbalance(1),
        ] {
            let kind = spec.kind();
            let mut builder = spec.build();
            let closed = builder.push(&trade(1));
            if matches!(kind, BarKind::Tick | BarKind::Imbalance) {
                assert!(closed.is_some(), "{kind:?}(1) closes immediately");
            }
        }
    }

    #[test]
    fn backfill_and_live_go_through_the_same_builder() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]);
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(1));

        s.ingest_live(&trade(4));
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(1), "boundary does not move");
    }

    #[test]
    fn switching_bar_type_rebuilds_from_retained_trades() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        let trades: Vec<Trade> = (1..=6).map(trade).collect();
        s.ingest_backfill(&trades); // tick(2): 6 trades -> 3 bars
        assert_eq!(s.bars().len(), 3);

        s.set_spec(BarSpec::Tick(3)); // rebuild: 6 trades -> 2 bars
        assert_eq!(s.bars().len(), 2);
        assert_eq!(
            s.backfill_boundary(),
            Some(2),
            "all six are backfill -> boundary at the end"
        );
    }

    #[test]
    fn boundary_is_recomputed_across_a_switch() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]); // 1 bar + partial
        s.ingest_live(&trade(4)); // closes bar 2 (backfill 3 + live 4)
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(1));

        // tick(4): 4 trades -> 1 bar. The first 3 (backfill) close 0 bars.
        s.set_spec(BarSpec::Tick(4));
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(0));
    }

    #[test]
    fn prepend_history_adds_older_bars_and_keeps_boundary() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(5), trade(6), trade(7), trade(8)]); // 2 bars
        s.ingest_live(&trade(9)); // opens a partial, still 2 closed bars
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(2));

        // Pull in the four older trades 1..=4.
        let added = s.prepend_history(&[trade(1), trade(2), trade(3), trade(4)]);
        // tick(2) over 1..=8 backfill = 4 closed bars; trade 9 is the partial.
        assert_eq!(s.bars().len(), 4);
        assert_eq!(added, 2, "two net bars were prepended");
        assert_eq!(
            s.backfill_boundary(),
            Some(4),
            "all eight retained backfill trades are history"
        );
    }

    #[test]
    fn prepend_empty_history_is_a_noop() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2)]);
        let before = s.bars().len();
        let added = s.prepend_history(&[]);
        assert_eq!(added, 0);
        assert_eq!(s.bars().len(), before);
    }

    #[test]
    fn setting_the_same_spec_is_a_noop() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2)]);
        let before = s.bars().len();
        s.set_spec(BarSpec::Tick(2));
        assert_eq!(s.bars().len(), before);
    }
}
