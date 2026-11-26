//! Every word a position says about its ratio. Risk always comes first and
//! is always named: traders say both "risk/reward" and "reward/risk", so a
//! bare `2:1` reads either way and no surface prints one.

use super::position_core::Side;
use super::position_payload::RATIO_PRESETS;

/// `2`, `1.5`, `1.33`: at most two decimals, none when whole.
#[must_use]
pub fn ratio_number(ratio: f64) -> String {
    let mut digits = format!("{ratio:.2}");
    while digits.ends_with('0') {
        digits.pop();
    }
    if digits.ends_with('.') {
        digits.pop();
    }
    digits
}

/// A risk/reward menu row: `Risk 1 : Reward 2`.
#[must_use]
pub fn menu_row(ratio: f64) -> String {
    format!("Risk 1 : Reward {}", ratio_number(ratio))
}

/// The menu's read-only row for a plan that is no preset: `Custom 1:1.3`.
#[must_use]
pub fn custom_row(ratio: f64) -> String {
    format!("Custom 1:{}", ratio_number(ratio))
}

/// The risk/reward button, short so the bar stays small: `R/R 1:2`. Its
/// menu says whether the plan is a preset.
#[must_use]
pub fn ratio_button(ratio: f64) -> String {
    format!("R/R 1:{}", ratio_number(ratio))
}

/// The plate on the entry line, short enough to sit inside the box:
/// `L R/R 1:2 · 165461.58`, `S` for a short.
#[must_use]
pub fn plan_plate(side: Side, ratio: f64, entry: f64) -> String {
    let side = match side {
        Side::Long => "L",
        Side::Short => "S",
    };
    format!(
        "{side} R/R 1:{} · {}",
        ratio_number(ratio),
        quantick_engine::display::display_f64(entry)
    )
}

/// The menu's preset rows, in order.
#[must_use]
pub fn preset_rows() -> [(f64, String); RATIO_PRESETS.len()] {
    RATIO_PRESETS.map(|ratio| (ratio, menu_row(ratio)))
}

#[cfg(test)]
#[path = "position_text_tests.rs"]
mod tests;
