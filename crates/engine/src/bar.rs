//! The [`Bar`] output type.

use rust_decimal::Decimal;

use crate::{Side, Trade};

/// A closed or in-progress bar: the summary of the trades in one sampling bucket.
/// Buy and sell volume are stored apart so delta and CVD stay exact. Closed or partial
/// is known from where the bar came from, not from its value.
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
    /// With [`extend`](Bar::extend) this is the one fold every builder and consumer shares,
    /// so a prefix of the tape summarises to the same numbers everywhere.
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

    /// Fold `later`, the bar that follows this one, into this one.
    /// Keeps the first open and stamp, takes extremes and the later close and stamp, and adds
    /// volumes and counts (saturating).
    pub fn absorb(&mut self, later: &Self) {
        self.high = self.high.max(later.high);
        self.low = self.low.min(later.low);
        self.close = later.close;
        self.close_time = later.close_time;
        self.buy_volume = self.buy_volume.saturating_add(later.buy_volume);
        self.sell_volume = self.sell_volume.saturating_add(later.sell_volume);
        self.trade_count = self.trade_count.saturating_add(later.trade_count);
    }

    /// Total traded quantity: `buy_volume + sell_volume`; saturates instead of panicking.
    #[must_use]
    pub fn volume(&self) -> Decimal {
        self.buy_volume.saturating_add(self.sell_volume)
    }

    /// Order-flow delta: `buy_volume - sell_volume`; saturates instead of panicking.
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

    /// The public fold and the builders' accumulation are the same fold: no private variant.
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

    /// Absorbing a later bar equals the bar a builder cuts from both runs taken as one.
    #[test]
    fn absorbing_a_later_bar_summarises_both_runs_as_one() {
        let trades = [
            trade(0, "36000.0", "1.0", Side::Buy),
            trade(1, "36010.5", "0.25", Side::Sell),
            trade(2, "35990.25", "2.5", Side::Buy),
            trade(3, "36005.0", "0.75", Side::Sell),
        ];
        let fold = |run: &[Trade]| {
            let mut bar = Bar::opened_by(&run[0]);
            for t in &run[1..] {
                bar.extend(t);
            }
            bar
        };
        let mut earlier = fold(&trades[..2]);
        earlier.absorb(&fold(&trades[2..]));
        assert_eq!(earlier, fold(&trades));

        let mut saturated = fold(&trades[..1]);
        saturated.buy_volume = Decimal::MAX;
        saturated.trade_count = u64::MAX;
        saturated.absorb(&fold(&trades[2..]));
        assert_eq!(saturated.buy_volume, Decimal::MAX);
        assert_eq!(saturated.trade_count, u64::MAX);
    }

    /// Side totals saturate: quantities come from an untrusted feed.
    #[test]
    fn extending_saturates_instead_of_overflowing_a_side() {
        let mut bar = Bar::opened_by(&trade(0, "1.0", "1.0", Side::Buy));
        bar.buy_volume = Decimal::MAX;
        bar.extend(&trade(1, "1.0", "1.0", Side::Buy));
        assert_eq!(bar.buy_volume, Decimal::MAX);
        assert_eq!(bar.trade_count, 2);
    }
}
