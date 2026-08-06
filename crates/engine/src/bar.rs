//! The [`Bar`] output type.

use rust_decimal::Decimal;

use crate::{Side, Trade};

/// A completed — or in-progress — alternative bar.
///
/// A bar summarises the run of trades that fell into one sampling bucket,
/// whether that bucket is "N trades" (tick), "N units" (volume), "N notional"
/// (dollar) or "N milliseconds" (time). The bucketing rule lives in the bar
/// *builder*; the shape of the summary is the same for all of them.
///
/// The same type represents both a **closed** bar (returned by a builder when a
/// bucket fills) and the **in-progress** bar a builder is still accumulating
/// (its `partial()`): the fields simply reflect the trades seen so far. Whether
/// a given `Bar` is closed is known from *where* you obtained it, not from the
/// value itself — a builder returns closed bars and exposes the partial
/// separately.
///
/// [`buy_volume`](Bar::buy_volume) and [`sell_volume`](Bar::sell_volume) are
/// stored separately so order-flow signals ([`delta`](Bar::delta), CVD) stay
/// exact; [`volume`](Bar::volume) and [`delta`](Bar::delta) are derived from
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    /// Timestamp of the first trade in the bar (epoch ms).
    pub open_time: i64,
    /// Timestamp of the last trade in the bar (epoch ms).
    pub close_time: i64,
    /// Price of the first trade.
    pub open: Decimal,
    /// Highest trade price in the bar.
    pub high: Decimal,
    /// Lowest trade price in the bar.
    pub low: Decimal,
    /// Price of the last trade.
    pub close: Decimal,
    /// Quantity traded on the buy (taker-buy) side.
    pub buy_volume: Decimal,
    /// Quantity traded on the sell (taker-sell) side.
    pub sell_volume: Decimal,
    /// Number of trades aggregated into the bar.
    pub trade_count: u64,
}

impl Bar {
    /// Open a fresh one-trade bar on `trade`.
    ///
    /// This and [`extend`](Bar::extend) are how *every* builder accumulates —
    /// the bucketing rule decides when a bar closes, never how it summarises.
    /// They are public because the summary of a run of trades is a fact about
    /// the trades, not a private detail of one builder: a consumer that needs
    /// the bar a prefix of the tape would have formed (the chart's live lane
    /// asking an indicator what it would have shown mid-bar) must reach the
    /// same numbers the builder does, and a second implementation of this
    /// fold is a second answer waiting to drift.
    #[must_use]
    pub fn opened_by(trade: &Trade) -> Self {
        let (buy_volume, sell_volume) = split_volume(trade);
        Self {
            open_time: trade.timestamp_ms,
            close_time: trade.timestamp_ms,
            open: trade.price,
            high: trade.price,
            low: trade.price,
            close: trade.price,
            buy_volume,
            sell_volume,
            trade_count: 1,
        }
    }

    /// Fold `trade` into this in-progress bar. See [`opened_by`](Bar::opened_by).
    pub fn extend(&mut self, trade: &Trade) {
        self.high = self.high.max(trade.price);
        self.low = self.low.min(trade.price);
        self.close = trade.price;
        self.close_time = trade.timestamp_ms;
        // Saturating for the same reason as a builder's accumulator: untrusted
        // feed quantities must not panic if a running side total overflows.
        match trade.side {
            Side::Buy => self.buy_volume = self.buy_volume.saturating_add(trade.quantity),
            Side::Sell => self.sell_volume = self.sell_volume.saturating_add(trade.quantity),
        }
        self.trade_count += 1;
    }

    /// Total traded quantity: `buy_volume + sell_volume`.
    ///
    /// Saturates rather than panicking on the (physically impossible) overflow,
    /// so a corrupt or adversarial feed can never crash a caller reading a bar.
    #[must_use]
    pub fn volume(&self) -> Decimal {
        self.buy_volume.saturating_add(self.sell_volume)
    }

    /// Order-flow delta: `buy_volume - sell_volume`.
    ///
    /// Positive when buyers were the net aggressors over the bar. Saturates
    /// rather than panicking on overflow (see [`volume`](Bar::volume)).
    #[must_use]
    pub fn delta(&self) -> Decimal {
        self.buy_volume.saturating_sub(self.sell_volume)
    }
}

/// A trade contributes its quantity to exactly one side.
fn split_volume(trade: &Trade) -> (Decimal, Decimal) {
    match trade.side {
        Side::Buy => (trade.quantity, Decimal::ZERO),
        Side::Sell => (Decimal::ZERO, trade.quantity),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BarBuilder as _;
    use core::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn volume_and_delta_are_derived() {
        let bar = Bar {
            open_time: 1_700_000_000_000,
            close_time: 1_700_000_000_500,
            open: dec("36000.0"),
            high: dec("36010.0"),
            low: dec("35990.0"),
            close: dec("36005.0"),
            buy_volume: dec("1.5"),
            sell_volume: dec("0.4"),
            trade_count: 7,
        };
        assert_eq!(bar.volume(), dec("1.9"));
        assert_eq!(bar.delta(), dec("1.1"));
    }

    fn trade(agg_id: u64, price: &str, quantity: &str, side: Side) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1_700_000_000_000 + agg_id as i64 * 10,
            price: dec(price),
            quantity: dec(quantity),
            side,
        }
    }

    /// The public fold and the builders' own accumulation are the same fold.
    ///
    /// This is the guard behind [`Bar::opened_by`]'s promise: a consumer that
    /// summarises a prefix of the tape itself must land on the numbers the
    /// builder would have produced, bar type and all. Every builder folds
    /// through these two methods, so the assertion is that no builder grew a
    /// private variant of them.
    #[test]
    fn folding_trades_by_hand_reaches_the_builders_own_partial() {
        let trades = [
            trade(0, "36000.0", "1.0", Side::Buy),
            trade(1, "36010.5", "0.25", Side::Sell),
            trade(2, "35990.25", "2.5", Side::Buy),
            trade(3, "36005.0", "0.75", Side::Sell),
        ];

        // A threshold high enough that nothing closes: the builder's partial
        // is the whole run.
        let mut builder = crate::TickBarBuilder::new(100);
        for t in &trades {
            assert!(
                builder.push(t).is_none(),
                "the fixture must not close a bar"
            );
        }

        let mut folded: Option<Bar> = None;
        for t in &trades {
            match &mut folded {
                None => folded = Some(Bar::opened_by(t)),
                Some(bar) => bar.extend(t),
            }
        }

        assert_eq!(folded.as_ref(), builder.partial());
    }

    /// Side totals saturate rather than panicking: the quantities come from an
    /// untrusted feed, and the same rule the builders' accumulators follow.
    #[test]
    fn extending_saturates_instead_of_overflowing_a_side() {
        let mut bar = Bar::opened_by(&trade(0, "1.0", "1.0", Side::Buy));
        bar.buy_volume = Decimal::MAX;
        bar.extend(&trade(1, "1.0", "1.0", Side::Buy));
        assert_eq!(bar.buy_volume, Decimal::MAX);
        assert_eq!(bar.trade_count, 2);
    }
}
