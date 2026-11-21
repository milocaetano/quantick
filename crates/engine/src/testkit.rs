//! Compact valid values for tests outside the engine.

use rust_decimal::Decimal;

use crate::{Side, Trade};

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
    pub fn side(mut self, value: Side) -> Self {
        self.0.side = value;
        self
    }
    #[must_use]
    pub fn build(&self) -> Trade {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_builder_is_valid_and_overrideable() {
        let builder = TradeBuilder::new(7).side(Side::Sell);
        assert_eq!(builder.build().agg_id, 7);
        assert_eq!(builder.build().side, Side::Sell);
    }
}
