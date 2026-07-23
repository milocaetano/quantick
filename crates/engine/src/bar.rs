//! The [`Bar`] output type.

use rust_decimal::Decimal;

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
    /// Total traded quantity: `buy_volume + sell_volume`.
    #[must_use]
    pub fn volume(&self) -> Decimal {
        self.buy_volume + self.sell_volume
    }

    /// Order-flow delta: `buy_volume - sell_volume`.
    ///
    /// Positive when buyers were the net aggressors over the bar.
    #[must_use]
    pub fn delta(&self) -> Decimal {
        self.buy_volume - self.sell_volume
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
