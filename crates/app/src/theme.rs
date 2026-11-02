//! The chrome's design tokens, named after `docs/ux/ui-design-model.md` §4.
//!
//! One module answers "which colour is this?" for every fixed surface — menu,
//! toolbar, tool rail, dock, status bar — so a new control never invents its
//! own grey. The chart canvas itself stays user-configurable through
//! [`crate::style::CanvasStyle`]; only the chrome is pinned here.
//!
//! [`AMBER`] is reserved for provenance honesty — replay, the backfill
//! divider, inferred data — and is never decoration. This is the UI
//! expression of the project's data-honesty rule. [`PANE_MARK`] is its dimmed
//! relative, a name rather than a warning.
//!
//! Reserved is not the same as compulsory. A provenance mark the trader reads
//! *constantly* rather than acts on is better made quiet: see [`SEAM_LINE`],
//! which marks the venue/prints boundary without amber's alarm.

// The tokens in `palette`, the colours derived from them in `ink`, and the
// one call that dresses egui in `chrome`. The leaves are private, so
// `theme::TEXT_MUTED` stays the address of every token.
mod chrome;
mod ink;
mod palette;
mod resolved;
pub mod store;

pub use resolved::{Palette, get, install_fonts, sync};

pub use chrome::apply;
pub use ink::{active_tint, ink, ink_on, press_tint, side_color};
// The contrast arithmetic is read back by the tests that pin every number a
// layer draws against its background; production code asks `ink_on` instead.
#[cfg(test)]
pub use ink::relative_luminance;
// The anchored VWAP's colour, painted by the headless tool; pinned to it here.
#[cfg(test)]
pub use palette::DRAW_CYAN;
pub use palette::{
    ACCENT, AMBER, BARE_HOVER, BORDER, BUY, CANVAS, CAPSULE_BORDER, CAPSULE_FILL, CAPSULE_SHADOW,
    CASING, CASING_EXTRA_PX, CHIP_INK, CHROME, CONTROL, DAY_RULE, DRAWING_SWATCHES, FLOAT_SHADOW,
    GAP_LABEL, GAP_LINE, INSET, OFF, PANE_ACTION, PANE_MARK, POC, REC, SEAM_LABEL, SEAM_LINE, SELL,
    TAG_BG, TEXT_FAINT, TEXT_MUTED, TEXT_PRIMARY, TEXT_SUPPORT, WARN,
};

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
