//! The human half of the setup: a price region the machine never invents.

use rust_decimal::Decimal;

/// A price band, drawn by a human on the chart (or fixed by a backtest
/// config). The kernel only ever *tests* against it; deciding where it sits
/// is the judgement this crate exists to leave with the trader.
///
/// Time validity is deliberately not here: whether the drawing still covers
/// "now" is a question about the drawing's anchors, which the caller answers
/// per closed bar (`region_active`). Price containment is the half that is
/// identical for chart and backtest, so it lives in the kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    low: Decimal,
    high: Decimal,
}

impl Region {
    /// Build from any two edge prices; the order is normalised so a
    /// rectangle drawn upward and one drawn downward mean the same band.
    #[must_use]
    pub fn new(a: Decimal, b: Decimal) -> Self {
        Self {
            low: a.min(b),
            high: a.max(b),
        }
    }

    #[must_use]
    pub fn low(&self) -> Decimal {
        self.low
    }

    #[must_use]
    pub fn high(&self) -> Decimal {
        self.high
    }

    /// Inclusive containment: a close sitting exactly on the drawn edge is
    /// inside the region the human drew.
    #[must_use]
    pub fn contains(&self, price: Decimal) -> bool {
        price >= self.low && price <= self.high
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(v: i64) -> Decimal {
        Decimal::from(v)
    }

    #[test]
    fn edges_are_inside_and_order_does_not_matter() {
        let drawn_down = Region::new(dec(110), dec(100));
        let drawn_up = Region::new(dec(100), dec(110));
        assert_eq!(drawn_down, drawn_up);
        assert!(drawn_up.contains(dec(100)));
        assert!(drawn_up.contains(dec(110)));
        assert!(drawn_up.contains(dec(105)));
        assert!(!drawn_up.contains(dec(99)));
        assert!(!drawn_up.contains(dec(111)));
    }
}
