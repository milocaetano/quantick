//! Context-local appearance adapters. Retain a palette at each rendering boundary.
pub(crate) mod choice;
mod chrome;
mod ink;
#[cfg(test)]
mod palette;
mod resolved;
pub mod store;

pub use resolved::{Palette, axes, bookmap, bookmap_scope, get, install_fonts, sync};

pub use chrome::apply;
pub use ink::{active_tint, press_tint, with_opacity};
#[cfg(test)]
pub use ink::{ink, side_color};
// The contrast arithmetic is read back by the tests that pin every number a
// layer draws against its background; production code asks `ink_on` instead.
#[cfg(test)]
pub use ink::relative_luminance;
// The anchored VWAP's colour, painted by the headless tool; pinned to it here.
#[cfg(test)]
pub use palette::{
    ACCENT, AMBER, BARE_HOVER, BORDER, BUY, CANVAS, CASING, CHIP_INK, CHROME, CONTROL, DRAW_CYAN,
    FLOW_KEY_DOWN, FLOW_KEY_DOWN_RIM, FLOW_KEY_UP_BOTTOM, FLOW_KEY_UP_LIP, FLOW_KEY_UP_SHADOW,
    FLOW_KEY_UP_TOP, FLOW_SURFACE, POC, SELL, TEXT_MUTED, TEXT_PRIMARY, WARN,
};
pub use quantick_appearance::palette::CASING_EXTRA_PX;

// Layout-branch inks with no appearance token yet: fixed Dark values until the
// colour pass gives them a `Token`. Named here so the pass finds every one.
/// A market tab playing a recording: its label and status dot.
pub const REPLAY: eframe::egui::Color32 = eframe::egui::Color32::from_rgb(0xC5, 0x8C, 0xFF);
/// The logo's teal, sampled from `assets/icon.png`: the "tick" of the wordmark.
pub const BRAND: eframe::egui::Color32 = eframe::egui::Color32::from_rgb(0x3A, 0xF0, 0xB2);
/// The wordmark's "uan" at rest; white when the Home tab is lit or active.
pub const HOME_INK: eframe::egui::Color32 = eframe::egui::Color32::from_rgb(0xDD, 0xE4, 0xEC);

/// The toolbar's market button: bare at rest (border and fill on hover) when
/// true, bordered like History when false. Kept for a side-by-side look.
pub const MARKET_BUTTON_QUIET: bool = false;

#[cfg(test)]
use eframe::egui::Color32;

/// Single-line text with an explicit width budget; truncation remains visible.
pub fn text_fitted(
    painter: &eframe::egui::Painter,
    at: eframe::egui::Pos2,
    align: eframe::egui::Align2,
    text: impl Into<String>,
    font: eframe::egui::FontId,
    color: eframe::egui::Color32,
    width: f32,
) {
    let mut job = eframe::egui::text::LayoutJob::simple_singleline(text.into(), font, color);
    job.wrap.max_width = width.max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(at, galley.size());
    painter.galley(rect.min, galley, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_tint_is_the_accent_at_22_percent() {
        // Compare through the same constructor: egui stores premultiplied
        // colour, so the raw channel accessors do not round-trip the input.
        let tint = active_tint(ACCENT);
        assert_eq!(tint, Color32::from_rgba_unmultiplied(0x8A, 0xB4, 0xF8, 56));
        assert_eq!(tint.a(), 56);
    }

    #[test]
    fn press_tint_is_one_step_deeper_than_active() {
        let tint = press_tint(ACCENT);
        assert_eq!(tint, Color32::from_rgba_unmultiplied(0x8A, 0xB4, 0xF8, 84));
        assert!(
            tint.a() > active_tint(ACCENT).a(),
            "a press on an armed button must be distinguishable"
        );
    }
}
