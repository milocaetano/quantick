//! Context-local appearance adapters. Retain a palette at each rendering boundary.
mod chrome;
mod ink;
#[cfg(test)]
mod palette;
mod resolved;
pub mod store;

pub use resolved::{Palette, get, install_fonts, sync};

pub use chrome::apply;
pub use ink::{active_tint, press_tint};
#[cfg(test)]
pub use ink::{ink, side_color};
// The contrast arithmetic is read back by the tests that pin every number a
// layer draws against its background; production code asks `ink_on` instead.
#[cfg(test)]
pub use ink::relative_luminance;
// The anchored VWAP's colour, painted by the headless tool; pinned to it here.
#[cfg(test)]
pub use palette::{
    ACCENT, BORDER, BUY, CANVAS, CASING, CHIP_INK, CONTROL, DRAW_CYAN, POC, SELL, TEXT_MUTED,
    TEXT_PRIMARY,
};
pub use quantick_appearance::palette::CASING_EXTRA_PX;

#[cfg(test)]
use eframe::egui::Color32;

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
