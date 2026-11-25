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

/// The risk/reward button, in pieces so the host can ink the risk digit in
/// the stop colour and the reward digit in the target colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatioButton {
    /// `Custom · ` for a plan that is no preset, else empty.
    pub lead: &'static str,
    /// `R/R `.
    pub prefix: &'static str,
    pub risk: &'static str,
    pub colon: &'static str,
    pub reward: String,
}

impl RatioButton {
    /// The button as one string: `R/R 1:2`, `Custom · R/R 1:1.3`.
    #[must_use]
    pub fn text(&self) -> String {
        format!(
            "{}{}{}{}{}",
            self.lead, self.prefix, self.risk, self.colon, self.reward
        )
    }
}

#[must_use]
pub fn ratio_button(ratio: f64, custom: bool) -> RatioButton {
    RatioButton {
        lead: if custom { "Custom · " } else { "" },
        prefix: "R/R ",
        risk: "1",
        colon: ":",
        reward: ratio_number(ratio),
    }
}

/// The plate at the entry: `Long · risk 1 : reward 2 · entry 165461.58`.
#[must_use]
pub fn plan_plate(side: Side, ratio: f64, entry: f64) -> String {
    let side = match side {
        Side::Long => "Long",
        Side::Short => "Short",
    };
    format!(
        "{side} · risk 1 : reward {} · entry {}",
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
