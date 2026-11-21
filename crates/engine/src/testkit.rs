//! Compact valid values for tests outside the engine.

use rust_decimal::Decimal;

use crate::{Bar, Side, Trade};

#[derive(Clone)]
pub struct TradeBuilder(Trade);

impl TradeBuilder {
    #[must_use]
    pub fn new(agg_id: u64) -> Self {
        Self(Trade {
            agg_id,
            timestamp_ms: agg_id as i64,
            price: Decimal::new(100, 0),
            quantity: Decimal::ONE,
            side: Side::Buy,
        })
    }

    #[must_use]
    pub fn timestamp_ms(mut self, value: i64) -> Self {
        self.0.timestamp_ms = value;
        self
    }
    #[must_use]
    pub fn price(mut self, value: Decimal) -> Self {
        self.0.price = value;
        self
    }
    #[must_use]
    pub fn quantity(mut self, value: Decimal) -> Self {
        self.0.quantity = value;
        self
    }
    #[must_use]
    pub fn side(mut self, value: Side) -> Self {
        self.0.side = value;
        self
    }
    #[must_use]
    pub fn build(&self) -> Trade {
        self.0.clone()
    }
}

#[derive(Clone)]
pub struct BarBuilder(Bar);

impl BarBuilder {
    #[must_use]
    pub fn new(open_time: i64) -> Self {
        let price = Decimal::new(100, 0);
        Self(Bar {
            open_time,
            close_time: open_time,
            open: price,
            high: price,
            low: price,
            close: price,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        })
    }

    #[must_use]
    pub fn close_time(mut self, value: i64) -> Self {
        self.0.close_time = value;
        self
    }
    #[must_use]
    pub fn prices(mut self, open: Decimal, high: Decimal, low: Decimal, close: Decimal) -> Self {
        self.0.open = open;
        self.0.high = high;
        self.0.low = low;
        self.0.close = close;
        self
    }
    #[must_use]
    pub fn volumes(mut self, buy: Decimal, sell: Decimal) -> Self {
        self.0.buy_volume = buy;
        self.0.sell_volume = sell;
        self
    }
    #[must_use]
    pub fn trade_count(mut self, value: u64) -> Self {
        self.0.trade_count = value;
        self
    }
    #[must_use]
    pub fn build(&self) -> Bar {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_are_valid_overrideable_and_independent() {
        let builder = TradeBuilder::new(7).side(Side::Sell);
        assert_eq!(builder.build().agg_id, 7);
        assert_eq!(builder.build().side, Side::Sell);
        let bar = BarBuilder::new(9).trade_count(3).build();
        assert_eq!((bar.open_time, bar.trade_count), (9, 3));
    }
}
