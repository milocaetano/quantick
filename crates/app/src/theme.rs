//! The chrome's design tokens, named after `docs/ux/ui-design-model.md` §4.
//!
//! One module answers "which colour is this?" for every fixed surface — menu,
//! toolbar, tool rail, dock, status bar — so a new control never invents its
//! own grey. The chart canvas itself stays user-configurable through
//! [`crate::style::CanvasStyle`]; only the chrome is pinned here.
//!
//! [`AMBER`] is reserved for provenance honesty — replay, the backfill
//! divider, inferred data — and is never decoration. This is the UI
//! expression of the project's data-honesty rule.
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

pub use chrome::apply;
pub use ink::{active_tint, ink, ink_on, press_tint, side_color};
// The contrast arithmetic is read back by the tests that pin every number a
// layer draws against its background; production code asks `ink_on` instead.
#[cfg(test)]
pub use ink::{contrast_ratio, relative_luminance};
pub use palette::{
    ACCENT, AMBER, BORDER, BUY, CANVAS, CASING, CASING_EXTRA_PX, CHIP_INK, CHROME, CONTROL,
    DRAW_CYAN, DRAWING_SWATCHES, FLOAT_SHADOW, GAP_LABEL, GAP_LINE, INSET, POC, REC,
    SEAM_LABEL, SEAM_LINE, SELL, TAG_BG, TEXT_FAINT, TEXT_MUTED, TEXT_PRIMARY, TEXT_SUPPORT, WARN,
};

#[cfg(test)]
use eframe::egui::Color32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_match_the_design_model() {
        // The values documented in docs/ux/ui-design-model.md §4, verbatim.
        assert_eq!(CANVAS, Color32::from_rgb(19, 23, 34));
        assert_eq!(CHROME, Color32::from_rgb(23, 27, 38));
        assert_eq!(INSET, Color32::from_rgb(16, 20, 29));
        assert_eq!(CONTROL, Color32::from_rgb(35, 41, 54));
        assert_eq!(BORDER, Color32::from_rgb(46, 54, 72));
        assert_eq!(TEXT_PRIMARY, Color32::from_rgb(210, 218, 226));
        assert_eq!(TEXT_MUTED, Color32::from_rgb(150, 160, 175));
        assert_eq!(TEXT_FAINT, Color32::from_rgb(110, 120, 135));
        assert_eq!(BUY, Color32::from_rgb(38, 166, 154));
        assert_eq!(SELL, Color32::from_rgb(239, 83, 80));
        assert_eq!(ACCENT, Color32::from_rgb(138, 180, 248));
        assert_eq!(AMBER, Color32::from_rgb(240, 185, 11));
        assert_eq!(WARN, Color32::from_rgb(255, 99, 71));
        assert_eq!(TAG_BG, Color32::from_rgb(55, 63, 80));
    }

    #[test]
    fn canvas_default_and_grid_agree_with_the_style_module() {
        // The user-editable canvas defaults must start on the tokens, or the
        // first paint would disagree with this palette.
        let canvas = crate::style::CanvasStyle::default();
        assert_eq!(
            canvas.background_rgba(),
            [CANVAS.r(), CANVAS.g(), CANVAS.b(), 255]
        );
        let grid = canvas.grid_rgba().expect("grid on by default");
        assert_eq!(&grid[..3], &[CONTROL.r(), CONTROL.g(), CONTROL.b()]);
    }

    #[test]
    fn default_candles_agree_with_the_buy_sell_tokens() {
        // The Order-flow preset's fills are the token pair; a drifted preset
        // would break the "one deliberate visual signature" rule quietly.
        let candles = crate::style::CandlePreset::OrderFlow.style();
        assert_eq!(candles.bull_fill, [BUY.r(), BUY.g(), BUY.b()]);
        assert_eq!(candles.bear_fill, [SELL.r(), SELL.g(), SELL.b()]);
    }

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

    #[test]
    fn support_text_matches_the_redesign_spec() {
        assert_eq!(TEXT_SUPPORT, Color32::from_rgb(0x86, 0x92, 0xA4));
    }

    /// The promise this rule makes, and the only one it can: whichever ink is
    /// handed out is the better of the two on that fill.
    ///
    /// Not "always 4.5:1" — with `CHIP_INK` and `TEXT_PRIMARY` as the pair,
    /// fills in a narrow mid band clear neither, and a test asserting the
    /// stronger claim passes only for as long as nobody picks a colour inside
    /// it. Widening the guarantee needs a third ink, not a moved threshold.
    #[test]
    fn the_ink_is_always_the_better_of_the_two() {
        for fill in [
            Color32::from_rgb(0x8A, 0xB4, 0xF8), // the stock drawing blue
            Color32::from_rgb(0xFF, 0xE0, 0x66), // a pale yellow
            Color32::from_rgb(0x0B, 0x1B, 0x3A), // dark navy, a legal pick
            Color32::from_rgb(0x7F, 0x7F, 0x7F), // mid grey, inside the band
            Color32::from_rgb(0x6B, 0x6B, 0x2E), // olive, likewise
            Color32::WHITE,
            Color32::BLACK,
            WARN,
            ACCENT,
        ] {
            let ink = ink_on(fill);
            let other = if ink == CHIP_INK {
                TEXT_PRIMARY
            } else {
                CHIP_INK
            };
            assert!(
                contrast_ratio(fill, ink) >= contrast_ratio(fill, other),
                "{fill:?} took {ink:?} at {:.2}:1 when the other gave {:.2}:1",
                contrast_ratio(fill, ink),
                contrast_ratio(fill, other)
            );
        }
    }

    /// A faded chip is graded as it will be *seen*, composited over the canvas
    /// — not as its premultiplied channels read on black.
    ///
    /// The honesty fade is exactly where this matters: a pale line faded to
    /// 45% is still a pale chip, and grading it on black flipped it to the
    /// light ink at under 3:1 while the dark ink it rejected cleared 4.5:1.
    #[test]
    fn a_faded_fill_is_graded_as_it_will_be_seen() {
        let faded = Color32::WHITE.gamma_multiply(0.45);
        assert_eq!(
            ink_on(faded),
            CHIP_INK,
            "a white line faded over a dark canvas is still a light chip"
        );
        assert_eq!(
            ink_on(Color32::WHITE),
            CHIP_INK,
            "and fading it does not change which ink it wants"
        );
    }

    /// Light colours take the dark ink and dark ones the light ink — stated
    /// outright, so a rule broken by accident fails here rather than in a
    /// screenshot nobody looks at twice.
    #[test]
    fn the_ink_follows_how_light_the_fill_looks_not_its_average() {
        assert_eq!(ink_on(Color32::WHITE), CHIP_INK);
        assert_eq!(ink_on(Color32::BLACK), TEXT_PRIMARY);
        // Same channel average, opposite answers: green carries the weight.
        assert_eq!(ink_on(Color32::from_rgb(0xC0, 0xC0, 0x00)), CHIP_INK);
        assert_eq!(ink_on(Color32::from_rgb(0x00, 0x00, 0xC0)), TEXT_PRIMARY);
    }
}
