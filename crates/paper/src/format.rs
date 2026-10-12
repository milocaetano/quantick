//! How a simulated number, a side and a journal folder are spelled.
//!
//! The words every paper surface shares, and the ones the account itself
//! writes into its acknowledgements and its journal: an exact decimal with
//! its trailing zeros stripped, a signed point count, `LONG`/`SHORT`, and a
//! symbol made safe to be a folder name. `app`'s `paper_chrome` re-exports
//! these beside the colours and widgets it keeps, so the chart and the
//! journal can never spell the same number two ways.
//!
//! Presentation only: nothing here holds state or reads a clock.

use quantick_engine::Side;
use quantick_engine::display::display_decimal;
use quantick_sim::EntryKind;
use rust_decimal::Decimal;

/// The open position, read-only, as every chrome surface reports it — the
/// HUD, the dock badge and the status cell all describe the same trade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionSummary {
    /// Which way the position points.
    pub side: Side,
    /// Contracts/units held.
    pub quantity: Decimal,
    /// Average entry price.
    pub avg_price: Decimal,
    /// Open profit at the current mark; `None` before any mark exists.
    pub open_points: Option<Decimal>,
}

/// `LONG`/`SHORT` — shared with the HUD so every surface uses one register.
pub fn position_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "LONG",
        Side::Sell => "SHORT",
    }
}

/// `BUY`/`SELL` — the register an entry button and an order tag speak.
pub fn side_word_upper(side: Side) -> &'static str {
    match side {
        Side::Buy => "BUY",
        Side::Sell => "SELL",
    }
}

/// The order kind in a sentence: `market`, `limit`, `stop`.
pub fn kind_word(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "market",
        EntryKind::Limit => "limit",
        EntryKind::Stop => "stop",
    }
}

/// Three-letter order kind for the compact chart tags (`LMT`, `STP`, `MKT`).
pub fn kind_short(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::Market => "MKT",
        EntryKind::Limit => "LMT",
        EntryKind::Stop => "STP",
    }
}

/// A value for display: at most two places, trailing zeros stripped —
/// prices and quantities on every paper surface, so an average price of
/// `1.3333333333` reads `1.33`.
pub fn fmt_decimal(value: Decimal) -> String {
    display_decimal(value)
}

/// Exact value, trailing zeros stripped — for the journal and its exports,
/// which are the source of truth and never round.
pub fn fmt_exact(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Points rounded for display (the stored value stays exact).
pub fn fmt_points(value: Decimal) -> String {
    fmt_decimal(value)
}

/// Signed points: an explicit `+` on gains so a green `12` can never be
/// misread as a count.
pub fn fmt_signed_points(value: Decimal) -> String {
    if value > Decimal::ZERO {
        format!("+{}", fmt_points(value))
    } else {
        fmt_points(value)
    }
}

pub use quantick_replay::deals::sanitize_symbol;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_points_always_carry_their_sign() {
        assert_eq!(fmt_signed_points(Decimal::from(12)), "+12");
        assert_eq!(fmt_signed_points(Decimal::from(-3)), "-3");
        assert_eq!(fmt_signed_points(Decimal::ZERO), "0");
    }

    #[test]
    fn display_values_carry_at_most_two_decimals() {
        let third = Decimal::from(4) / Decimal::from(3);
        assert_eq!(fmt_decimal(third), "1.33");
        assert_eq!(fmt_decimal(Decimal::new(-5, 3)), "-0.01");
        assert_eq!(fmt_decimal(Decimal::new(510, 2)), "5.1");
        assert_eq!(fmt_decimal(Decimal::from(12)), "12");
        assert_eq!(fmt_decimal(Decimal::ZERO), "0");
        assert_eq!(fmt_signed_points(third), "+1.33");
    }

    #[test]
    fn exact_values_never_round() {
        assert_eq!(fmt_exact(Decimal::new(13_333, 4)), "1.3333");
        assert_eq!(fmt_exact(Decimal::new(510, 2)), "5.1");
    }
}
