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
    /// Aggregate id of the print that opened the position — the audit trail
    /// back to the tape, carried into every [`ClosedTrade`] it produces.
    ///
    /// [`ClosedTrade`]: crate::ClosedTrade
    pub opened_agg_id: u64,
    /// Lowest price the position has been exposed to: its entry fills, every
    /// mark while it was open, and its exit fills. Together with
    /// `high_price` this yields the MAE/MFE recorded on close.
    pub low_price: Decimal,
    /// Highest price the position has been exposed to (see `low_price`).
    pub high_price: Decimal,
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

    /// Fold `price` into the exposure range the excursions are measured on.
    pub(crate) fn observe(&mut self, price: Decimal) {
        self.low_price = self.low_price.min(price);
        self.high_price = self.high_price.max(price);
    }

    /// Per-unit excursions against the average entry once `exit` joins the
    /// exposure range: `(adverse, favorable)`, both clamped at zero. The
    /// adverse side is where the position loses (below entry for a long,
    /// above it for a short); the favorable side is where it wins.
    #[must_use]
    pub(crate) fn excursions(&self, exit: Decimal) -> (Decimal, Decimal) {
        let low = self.low_price.min(exit);
        let high = self.high_price.max(exit);
        let (adverse, favorable) = match self.side {
            Side::Buy => (
                self.avg_price.saturating_sub(low),
                high.saturating_sub(self.avg_price),
            ),
            Side::Sell => (
                high.saturating_sub(self.avg_price),
                self.avg_price.saturating_sub(low),
            ),
        };
        (adverse.max(Decimal::ZERO), favorable.max(Decimal::ZERO))
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
