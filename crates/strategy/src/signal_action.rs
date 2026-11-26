//! What an armed script does when its signal fires: ring, or place a paper
//! order of a set size with fixed protective distances.
//!
//! The script answers only *when* — it returns a signal and never an order
//! (the dialect refuses `strategy.*`). Size and protection are the trader's
//! settings on the armed instance, carried here, and the kernel turns them
//! into [`crate::StrategyParams`]: an [`SignalAction::Order`] arms
//! [`crate::Execution::Paper`] with a [`PriceBracket`] in place of the
//! projected multiples.

use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignalAction {
    /// Ring the alarm and place nothing. The default.
    #[default]
    Alarm,
    /// Place a paper market order, protected as the order says.
    Order(SignalOrder),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalOrder {
    pub quantity: OrderQuantity,
    /// Stop distance from the entry, in price; zero places no stop.
    pub stop: Decimal,
    /// Take-profit distance from the entry, in price; zero places no target.
    pub gain: Decimal,
}

impl SignalOrder {
    #[must_use]
    pub fn bracket(&self) -> PriceBracket {
        PriceBracket {
            stop: self.stop,
            gain: self.gain,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderQuantity {
    Fixed(Decimal),
    /// Whatever the trader's ticket enters with when the bar closes; the
    /// consumer refreshes it before each closed bar.
    Ticket,
}

/// Fixed protective distances from the entry, in price units. The order is
/// placed bracketed off the price the kernel enters from — the trigger bar's
/// reference for a market entry, the cut edge for a retest limit — and both
/// legs are re-priced off the fill when it lands. A distance of zero or less
/// places no leg on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceBracket {
    pub stop: Decimal,
    pub gain: Decimal,
}
