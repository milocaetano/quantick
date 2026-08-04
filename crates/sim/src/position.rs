//! The one net position the simulator tracks.

use quantick_engine::Side;
use rust_decimal::Decimal;

/// The open net position (netting model, like a futures account): entries on
/// the same side average the price up or down, entries on the opposite side
/// close quantity first and open the remainder. There is never more than one
/// position per simulator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    /// `Buy` is a long position, `Sell` a short.
    pub side: Side,
    pub quantity: Decimal,
    /// Volume-weighted average entry price across the fills that built the
    /// position.
    pub avg_price: Decimal,
    /// Venue time of the print that opened the position.
    pub opened_ms: i64,
    /// Protective stop price; exits the whole position at the print that
    /// trades at or through it.
    pub stop_loss: Option<Decimal>,
    /// Protective limit price; exits the whole position at this price when a
    /// print trades at or through it.
    pub take_profit: Option<Decimal>,
}

impl Position {
    /// Signed open profit at `mark`, in points (price units × quantity).
    /// Saturates instead of panicking — prices come from an untrusted feed.
    #[must_use]
    pub fn open_points(&self, mark: Decimal) -> Decimal {
        signed_points(self.side, self.avg_price, mark, self.quantity)
    }
}

/// Profit in points for closing `quantity` opened at `entry` and exited at
/// `exit`, signed by position side. Saturating, never panicking.
#[must_use]
pub(crate) fn signed_points(
    side: Side,
    entry: Decimal,
    exit: Decimal,
    quantity: Decimal,
) -> Decimal {
    let per_unit = match side {
        Side::Buy => exit.saturating_sub(entry),
        Side::Sell => entry.saturating_sub(exit),
    };
    per_unit.saturating_mul(quantity)
}
