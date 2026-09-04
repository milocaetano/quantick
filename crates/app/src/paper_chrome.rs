//! The vocabulary every paper-trading surface shares: how a simulated
//! number, label, colour and journal folder are spelled.
//!
//! This module exists because two modules above it needed the same eleven
//! items and reached for each other to get them. `paper_report` — the
//! reading half — imported them from `paper_trading` — the writing half —
//! while `paper_trading` re-exported the ledger's own types back, and the
//! two became a cycle: neither could be read, moved or tested without the
//! other. The same shape `plot_area` was carved out of `pane` to fix.
//!
//! So the rule here is the one `paper_calendar` and `plot_area` already
//! follow, and it is the whole reason the module is worth its own file:
//!
//! - **It never reaches up.** Nothing here names `PaperTrading`,
//!   `ReportState` or any surface that draws. The imports below are the
//!   proof, and they are meant to stay that short.
//! - **It never reaches sideways either.** `theme` is the only module of
//!   this crate it imports. `paper_calendar` sits *above* it, painting its
//!   day cells with `points_color` and `fmt_signed_points` — which is
//!   exactly why `fmt_offset_minute` and `today` live there rather than
//!   here. Both need `CivilDate`, and hosting them here would have pointed
//!   this module back at the calendar that imports it: one cycle traded for
//!   another, in the module built to prevent it.
//! - **Presentation only.** Nothing here holds session state, opens a
//!   file picker, places an order or reads a clock. [`PositionSummary`] is
//!   the one type, and it is a read-only snapshot the host hands out.
//!
//! `crates/guards/src/cycle.rs` now fails the build if a future edit
//! points one of these modules back at another, which is what stops the
//! third cycle from being found by hand like the first two.

use std::path::Path;

use eframe::egui;
use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::theme;

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

/// Uppercase section caption — the ledger's group labels and column header.
pub(crate) fn caption(text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .size(10.0)
        .color(theme::TEXT_FAINT)
}

/// A pill-shaped toggle chip: accent fill while on, quiet control while off.
pub(crate) fn pill_toggle(ui: &mut egui::Ui, label: &str, on: bool, hover: &str) -> egui::Response {
    let (fill, ink, stroke) = if on {
        (theme::ACCENT, theme::CHIP_INK, egui::Stroke::NONE)
    } else {
        (
            theme::CONTROL,
            theme::TEXT_MUTED,
            egui::Stroke::new(1.0_f32, theme::BORDER),
        )
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(ink).small())
            .fill(fill)
            .stroke(stroke)
            .rounding(egui::Rounding::same(9.0)),
    )
    .on_hover_text(hover)
}

/// `38s`, `4m 18s`, `1h 02m` — a trade's age in venue time.
pub(crate) fn fmt_duration_ms(ms: i64) -> String {
    let seconds = (ms / 1000).max(0);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h {:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

/// `LONG`/`SHORT` — shared with the HUD so every surface uses one register.
pub(crate) fn position_word(side: Side) -> &'static str {
    match side {
        Side::Buy => "LONG",
        Side::Sell => "SHORT",
    }
}

/// Green gains, red losses, muted zero — shared with the HUD.
pub(crate) fn points_color(points: Decimal) -> egui::Color32 {
    match points.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => theme::BUY,
        std::cmp::Ordering::Less => theme::SELL,
        std::cmp::Ordering::Equal => theme::TEXT_MUTED,
    }
}

/// Exact value, trailing zeros stripped — prices and quantities.
pub(crate) fn fmt_decimal(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Points rounded to two places for display (the stored value stays exact).
pub(crate) fn fmt_points(value: Decimal) -> String {
    value.round_dp(2).normalize().to_string()
}

/// Signed points: an explicit `+` on gains so a green `12` can never be
/// misread as a count.
pub(crate) fn fmt_signed_points(value: Decimal) -> String {
    if value > Decimal::ZERO {
        format!("+{}", fmt_points(value))
    } else {
        fmt_points(value)
    }
}

/// Keep the characters real venue symbols use (`WDO$`, `WIN@N`… stay
/// recognizable); anything else becomes `_` so a symbol can never traverse
/// paths.
pub(crate) fn sanitize_symbol(symbol: &str) -> String {
    let cleaned: String = symbol
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "-_.$#".contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "_".to_owned()
    } else {
        cleaned
    }
}

/// The symbol folders under the history dir, for the report's combo box.
pub(crate) fn list_symbol_folders(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut folders: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    folders.sort();
    folders
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_sanitize_without_losing_venue_spellings() {
        assert_eq!(sanitize_symbol("WDO$"), "WDO$");
        assert_eq!(sanitize_symbol("BTCUSDT"), "BTCUSDT");
        assert_eq!(sanitize_symbol("../evil"), ".._evil");
        assert_eq!(sanitize_symbol(""), "_");
    }

    #[test]
    fn signed_points_always_carry_their_sign() {
        assert_eq!(fmt_signed_points(Decimal::from(12)), "+12");
        assert_eq!(fmt_signed_points(Decimal::from(-3)), "-3");
        assert_eq!(fmt_signed_points(Decimal::ZERO), "0");
    }

    #[test]
    fn durations_format_by_magnitude() {
        assert_eq!(fmt_duration_ms(38_000), "38s");
        assert_eq!(fmt_duration_ms(258_000), "4m 18s");
        assert_eq!(fmt_duration_ms(3_720_000), "1h 02m");
        assert_eq!(fmt_duration_ms(-5), "0s", "clock skew never explodes");
    }
}
