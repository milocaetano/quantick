//! The chart's per-bar footprint ladders, kept index-aligned with its bars.
//!
//! [`ChartState`](crate::state::ChartState) owns one [`FootprintSeries`] and
//! feeds it the same trades, in the same order, as its [`BarBuilder`] — the
//! ladder in `closed()[i]` describes exactly the trades summarised by bar `i`,
//! and `partial()` describes the forming bar. The engine's
//! [`FootprintBuilder`] knows nothing about bar boundaries; this type supplies
//! them.
//!
//! The one subtlety is the closing trade. Builders differ on whether the trade
//! that closes a bar belongs to it: a tick or volume bar closes *with* its
//! final trade, while a time bar closes when a trade beyond the bucket arrives
//! — that trade opens the next bar instead. Rather than encode per-builder
//! knowledge (a second copy of every closing rule, drifting from the first),
//! the series counts the trades it fed since the last close and reads the
//! answer off the closed bar's own `trade_count`: `pending + 1` means the
//! closing trade is inside, `pending` means it opens the next ladder.

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
    base_group: Decimal,
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
            base_group,
            closed: Vec::new(),
            pending: 0,
        }
    }

    /// The row width ladders are captured at (before any per-bar level-cap
    /// coarsening, which each ladder reports itself).
    #[must_use]
    pub fn base_group(&self) -> Decimal {
        self.base_group
    }

    /// Drop everything and start over at `base_group` — the rebuild
    /// counterpart of [`ChartState::rebuild`](crate::state::ChartState).
    pub fn reset(&mut self, base_group: Decimal) {
        self.builder = FootprintBuilder::new(base_group, DEFAULT_LEVEL_CAP);
        self.base_group = base_group;
        self.closed.clear();
        self.pending = 0;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{BarSpec, ChartState};
    use quantick_engine::{ImbalanceUnit, Side};
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn trade(agg_id: u64, timestamp_ms: i64, price: &str, quantity: &str, side: Side) -> Trade {
        Trade {
            agg_id,
            timestamp_ms,
            price: dec(price),
            quantity: dec(quantity),
            side,
        }
    }

    fn ladder_trades(fp: &BarFootprint) -> u64 {
        fp.levels().values().map(|l| l.trade_count).sum()
    }

    /// A chart with footprint accumulation switched on, the way the pane
    /// does it when the layer is shown.
    fn chart(spec: BarSpec) -> ChartState {
        let mut chart = ChartState::new(spec);
        chart.set_footprint_enabled(true);
        chart
    }

    /// The closing-trade inference must hold for *every* bar rule, not just
    /// the two with obvious boundaries: each closed ladder carries exactly
    /// its bar's trades and re-adds to its bar's buy/sell split. This is the
    /// test that fails if a future builder changes which side of the close
    /// its final trade lands on.
    #[test]
    fn every_bar_spec_keeps_ladders_aligned_with_bars() {
        let specs = [
            BarSpec::Tick(3),
            BarSpec::Volume(dec("4")),
            BarSpec::Dollar(dec("500")),
            BarSpec::Time(1000),
            BarSpec::Imbalance(ImbalanceUnit::Trades, 3),
        ];
        for spec in specs {
            let label = format!("{spec:?}");
            let mut chart = chart(spec);
            for i in 0..24u64 {
                chart.ingest_live(&trade(
                    i,
                    i as i64 * 300,
                    &format!("10{}.5", i % 5),
                    if i % 4 == 0 { "2.5" } else { "0.5" },
                    if i % 3 == 0 { Side::Sell } else { Side::Buy },
                ));
            }
            assert!(!chart.bars().is_empty(), "{label}: fixture closed no bar");
            assert_eq!(
                chart.bar_footprints().len(),
                chart.bars().len(),
                "{label}: ladder/bar count drifted"
            );
            for (bar, fp) in chart.bars().iter().zip(chart.bar_footprints()) {
                assert_eq!(ladder_trades(fp), bar.trade_count, "{label}");
                let buy: Decimal = fp.levels().values().map(|l| l.buy).sum();
                let sell: Decimal = fp.levels().values().map(|l| l.sell).sum();
                assert_eq!(buy, bar.buy_volume, "{label}");
                assert_eq!(sell, bar.sell_volume, "{label}");
            }
            let partial_trades = chart.partial_footprint().map(ladder_trades);
            assert_eq!(
                partial_trades,
                chart.partial().map(|bar| bar.trade_count),
                "{label}: partial ladder drifted from the partial bar"
            );
        }
    }

    /// Accumulation off is the default and costs nothing; switching it on
    /// mid-session refolds the retained trades, and switching it off drops
    /// the ladders.
    #[test]
    fn enabling_late_refolds_and_disabling_drops() {
        let mut chart = ChartState::new(BarSpec::Tick(2));
        for i in 0..5u64 {
            chart.ingest_live(&trade(i, 1000 + i as i64, "100.5", "1", Side::Buy));
        }
        assert!(chart.bar_footprints().is_empty(), "off by default");
        assert!(chart.partial_footprint().is_none());

        chart.set_footprint_enabled(true);
        assert_eq!(chart.bar_footprints().len(), chart.bars().len());
        assert_eq!(ladder_trades(chart.partial_footprint().unwrap()), 1);

        chart.set_footprint_enabled(false);
        assert!(chart.bar_footprints().is_empty());
    }

    /// Tick bars close *with* their final trade: each ladder must carry all N
    /// trades of its bar, and the ladder volumes must re-add to the bar's own
    /// buy/sell split.
    #[test]
    fn tick_bars_keep_ladders_aligned_and_volumes_exact() {
        let mut chart = chart(BarSpec::Tick(2));
        let trades = [
            trade(0, 1000, "100.0", "1.0", Side::Buy),
            trade(1, 1100, "100.5", "2.0", Side::Sell),
            trade(2, 1200, "101.0", "0.5", Side::Buy),
            trade(3, 1300, "100.5", "1.5", Side::Buy),
            trade(4, 1400, "99.5", "3.0", Side::Sell),
        ];
        for t in &trades {
            chart.ingest_live(t);
        }

        assert_eq!(chart.bars().len(), 2);
        assert_eq!(chart.bar_footprints().len(), 2);
        for (bar, fp) in chart.bars().iter().zip(chart.bar_footprints()) {
            assert_eq!(ladder_trades(fp), bar.trade_count);
            let buy: Decimal = fp.levels().values().map(|l| l.buy).sum();
            let sell: Decimal = fp.levels().values().map(|l| l.sell).sum();
            assert_eq!(buy, bar.buy_volume);
            assert_eq!(sell, bar.sell_volume);
        }
        // The trailing fifth trade is the forming bar's ladder.
        assert_eq!(ladder_trades(chart.partial_footprint().unwrap()), 1);
    }

    /// Time bars close on the boundary-crossing trade, which belongs to the
    /// *next* bar: the closed ladder must exclude it and the partial must
    /// start from it.
    #[test]
    fn time_bars_exclude_the_boundary_trade_from_the_closed_ladder() {
        let mut chart = chart(BarSpec::Time(1000));
        chart.ingest_live(&trade(0, 0, "100.0", "1.0", Side::Buy));
        chart.ingest_live(&trade(1, 500, "100.5", "2.0", Side::Sell));
        chart.ingest_live(&trade(2, 1500, "101.0", "4.0", Side::Buy));

        assert_eq!(chart.bars().len(), 1);
        assert_eq!(chart.bar_footprints().len(), 1);
        let closed = &chart.bar_footprints()[0];
        assert_eq!(ladder_trades(closed), 2);
        let closed_buy: Decimal = closed.levels().values().map(|l| l.buy).sum();
        assert_eq!(closed_buy, dec("1.0"), "the 4.0 buy is the next bar's");

        let partial = chart.partial_footprint().unwrap();
        assert_eq!(ladder_trades(partial), 1);
        let partial_buy: Decimal = partial.levels().values().map(|l| l.buy).sum();
        assert_eq!(partial_buy, dec("4.0"));
    }

    /// Switching the bar spec rebuilds bars and ladders through the same
    /// replay: alignment and totals survive.
    #[test]
    fn rebuild_keeps_ladders_aligned_with_bars() {
        let mut chart = chart(BarSpec::Tick(2));
        for i in 0..7u64 {
            chart.ingest_live(&trade(
                i,
                1000 + i as i64 * 100,
                &format!("10{}.5", i % 4),
                "1.0",
                if i % 3 == 0 { Side::Sell } else { Side::Buy },
            ));
        }
        chart.set_spec(BarSpec::Tick(3));
        assert_eq!(chart.bars().len(), 2);
        assert_eq!(chart.bar_footprints().len(), 2);
        for (bar, fp) in chart.bars().iter().zip(chart.bar_footprints()) {
            assert_eq!(ladder_trades(fp), bar.trade_count);
        }
        assert_eq!(ladder_trades(chart.partial_footprint().unwrap()), 1);
    }

    /// A finer footprint group refolds the retained trades without touching
    /// what the bars say.
    #[test]
    fn regrouping_refolds_ladders_from_the_retained_trades() {
        let mut chart = chart(BarSpec::Tick(2));
        chart.ingest_live(&trade(0, 1000, "100.25", "1.0", Side::Buy));
        chart.ingest_live(&trade(1, 1100, "100.75", "1.0", Side::Sell));

        // Default 0.01 rows: two distinct levels.
        assert_eq!(chart.bar_footprints()[0].levels().len(), 2);

        // One-point rows: both prints share bucket 100.
        chart.set_footprint_group(dec("1"));
        assert_eq!(chart.footprint_group(), dec("1"));
        assert_eq!(chart.bars().len(), 1, "bars are untouched by regrouping");
        let fp = &chart.bar_footprints()[0];
        assert_eq!(fp.levels().len(), 1);
        assert_eq!(fp.levels()[&100].trade_count, 2);

        // Same group again is a no-op (no rebuild churn).
        let revision = chart.timeline_revision();
        chart.set_footprint_group(dec("1"));
        assert_eq!(chart.timeline_revision(), revision);
    }

    /// Backfill and live ingestion land in the same aligned series.
    #[test]
    fn backfill_then_live_stays_aligned() {
        let mut chart = chart(BarSpec::Tick(2));
        chart.ingest_backfill(&[
            trade(0, 1000, "100.0", "1.0", Side::Buy),
            trade(1, 1100, "100.1", "1.0", Side::Sell),
            trade(2, 1200, "100.2", "1.0", Side::Buy),
        ]);
        chart.ingest_live(&trade(3, 1300, "100.3", "1.0", Side::Sell));
        assert_eq!(chart.bars().len(), 2);
        assert_eq!(chart.bar_footprints().len(), 2);
        assert!(chart.partial_footprint().is_none());
    }
}
