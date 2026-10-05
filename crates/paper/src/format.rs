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

/// Exact value, trailing zeros stripped — prices and quantities.
pub fn fmt_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Points rounded to two places for display (the stored value stays exact).
pub fn fmt_points(value: Decimal) -> String {
    value.round_dp(2).normalize().to_string()
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
}
