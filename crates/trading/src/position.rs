//! The open net position, and the completed round trip it becomes.

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::events::ExitReason;

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
    pub fn observe(&mut self, price: Decimal) {
        self.low_price = self.low_price.min(price);
        self.high_price = self.high_price.max(price);
    }

    /// Per-unit excursions against the average entry once `exit` joins the
    /// exposure range: `(adverse, favorable)`, both clamped at zero. The
    /// adverse side is where the position loses (below entry for a long,
    /// above it for a short); the favorable side is where it wins.
    #[must_use]
    pub fn excursions(&self, exit: Decimal) -> (Decimal, Decimal) {
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
pub fn signed_points(side: Side, entry: Decimal, exit: Decimal, quantity: Decimal) -> Decimal {
    let per_unit = match side {
        Side::Buy => exit.saturating_sub(entry),
        Side::Sell => entry.saturating_sub(exit),
    };
    per_unit.saturating_mul(quantity)
}

/// One completed round trip: an exit fill closing quantity against the
/// position's average entry at that moment. The unit persisted as history
/// and consumed by the performance report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTrade {
    /// Side of the position that closed: `Buy` was a long, `Sell` a short.
    pub side: Side,
    pub quantity: Decimal,
    /// Average entry price at the moment of the exit.
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    /// Venue time of the print that opened the position.
    pub opened_ms: i64,
    /// Venue time of the print that closed it.
    pub closed_ms: i64,
    /// Profit in points (price units × quantity), signed. Points, not
    /// currency: the workspace knows no per-instrument tick value, and a
    /// number the simulator cannot compute honestly is not shown.
    pub pnl_points: Decimal,
    pub exit_reason: ExitReason,
    /// Aggregate id of the print that opened the position — the audit trail
    /// back to the tape. `None` only on rows loaded from a version-1 history
    /// file, which did not record it; the simulator always fills it.
    pub entry_agg_id: Option<u64>,
    /// Aggregate id of the print that closed this quantity (see
    /// `entry_agg_id` for why it is optional).
    pub exit_agg_id: Option<u64>,
    /// Maximum adverse excursion in points (≥ 0): the worst the position ran
    /// against its average entry over every price it was exposed to — entry
    /// fills, marks while open, the exit fill — scaled by this trade's
    /// closed quantity. Measured against the average entry at close time,
    /// so a position that averaged in reports its excursion against the
    /// final average. `None` only for version-1 history rows: unknown is
    /// not zero.
    pub mae_points: Option<Decimal>,
    /// Maximum favorable excursion in points (≥ 0); see `mae_points`.
    pub mfe_points: Option<Decimal>,
}
