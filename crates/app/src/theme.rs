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

use eframe::egui;
use eframe::egui::Color32;

/// `bg/canvas` — default chart canvas (user-configurable in the appearance
/// dialog; [`crate::style::CanvasStyle`] holds the editable copy, and the
/// parity test below keeps the two in agreement — the reason this token
/// exists without a chrome consumer).
#[allow(dead_code)]
pub const CANVAS: Color32 = Color32::from_rgb(0x13, 0x17, 0x22);
/// `bg/chrome` — menu bar, toolbar, tool rail, dock and status bar.
pub const CHROME: Color32 = Color32::from_rgb(0x17, 0x1B, 0x26);
/// `bg/inset` — sub-panes and wells sunk into the chrome.
pub const INSET: Color32 = Color32::from_rgb(0x10, 0x14, 0x1D);
/// `bg/control` — buttons, combos and inputs (also the default grid colour).
pub const CONTROL: Color32 = Color32::from_rgb(0x23, 0x29, 0x36);
/// `border` — panel and control borders.
pub const BORDER: Color32 = Color32::from_rgb(0x2E, 0x36, 0x48);
/// `text/primary` — labels and values.
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xD2, 0xDA, 0xE2);
/// `text/muted` — secondary labels.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x96, 0xA0, 0xAF);
/// `text/faint` — hints, disabled text and the crosshair.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6E, 0x78, 0x87);
/// `buy` — bull candles, buy flow and the healthy/live status dot.
pub const BUY: Color32 = Color32::from_rgb(0x26, 0xA6, 0x9A);
/// `sell` — bear candles, sell flow, and the chrome's SELL affordances
/// (paper trading). Candles read their editable copy from [`crate::style`];
/// the parity test below pins the two together.
pub const SELL: Color32 = Color32::from_rgb(0xEF, 0x53, 0x50);
/// `poc` — the footprint's point-of-control line. Yellow, the user's call
/// after the reference charts (exocharts red would collide with the sell
/// side here). Close in hue to [`AMBER`] but a different surface: AMBER is
/// provenance *text* in the chrome, this is a line inside a candle — the
/// two never sit side by side.
pub const POC: Color32 = Color32::from_rgb(0xFF, 0xD5, 0x4F);
/// `accent/overlay` — selection accent and the future first indicator plot.
pub const ACCENT: Color32 = Color32::from_rgb(0x8A, 0xB4, 0xF8);
/// `honest/amber` — not-live provenance only: replay, backfill, inferred data.
pub const AMBER: Color32 = Color32::from_rgb(0xF0, 0xB9, 0x0B);
/// `warn` — threshold breaches and errors.
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0x63, 0x47);
/// `tag/bg` — tooltips and the axis price tag.
pub const TAG_BG: Color32 = Color32::from_rgb(0x37, 0x3F, 0x50);
/// `text/support` — small explanatory lines that carry real information
/// (the inspector's locked/hidden notes). [`TEXT_FAINT`] stays reserved for
/// decoration and disabled states, where 4.5:1 contrast is not required.
pub const TEXT_SUPPORT: Color32 = Color32::from_rgb(0x86, 0x92, 0xA4);
/// `chip/ink` — dark text inside a solid semantic-color chip: the last-price
/// chip's ink, shared by every surface that speaks the chip language (trade
/// buttons, the position HUD's side tag, the jump-to-live chip).
pub const CHIP_INK: Color32 = Color32::from_rgb(0x0E, 0x12, 0x1A);

/// Alpha of an "active" tint: a layer accent at 22% over the chrome.
const ACTIVE_TINT_ALPHA: u8 = 56; // ≈ 22% of 255
/// Alpha of a "pressed" tint: a layer accent at 33% over the chrome. One
/// step deeper than [`ACTIVE_TINT_ALPHA`], so a press on an already-active
/// button is still visible.
const PRESS_TINT_ALPHA: u8 = 84;

/// `accent` reduced to the 22% tint used behind an active icon.
#[must_use]
pub fn active_tint(accent: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), ACTIVE_TINT_ALPHA)
}

/// `accent` at the 33% tint painted while an icon button is held down.
#[must_use]
pub fn press_tint(accent: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), PRESS_TINT_ALPHA)
}

/// Point egui's own widgets at the tokens, so combos, sliders and windows
/// drawn anywhere in the app match the chrome without per-call overrides.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    // Tooltips wait a beat (350 ms) so a sweep across the tool rail does not
    // flash a trail of labels.
    style.interaction.tooltip_delay = 0.35;
    let visuals = &mut style.visuals;
    *visuals = egui::Visuals::dark();
    visuals.panel_fill = CHROME;
    visuals.window_fill = CHROME;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.extreme_bg_color = INSET;
    visuals.faint_bg_color = INSET;
    visuals.widgets.noninteractive.bg_fill = CHROME;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke.color = TEXT_MUTED;
    visuals.widgets.inactive.bg_fill = CONTROL;
    visuals.widgets.inactive.weak_bg_fill = CONTROL;
    visuals.widgets.inactive.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.hovered.bg_fill = BORDER;
    visuals.widgets.hovered.weak_bg_fill = BORDER;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    visuals.widgets.hovered.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.active.bg_fill = active_tint(ACCENT);
    visuals.widgets.active.weak_bg_fill = active_tint(ACCENT);
    visuals.widgets.active.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.open.bg_fill = CONTROL;
    visuals.widgets.open.weak_bg_fill = CONTROL;
    visuals.widgets.open.fg_stroke.color = TEXT_PRIMARY;
    visuals.selection.bg_fill = active_tint(ACCENT);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    ctx.set_style(style);
}

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
}
