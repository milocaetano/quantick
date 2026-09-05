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
/// `seam/line` — the venue/prints boundary: white, and mostly transparent.
///
/// The one provenance mark that is *always* on screen, on every time-cutting
/// pane, for the whole session. [`AMBER`] earns its loudness where it says
/// "this is not live" about something the trader might act on; here it was
/// shouting a fact that never changes and never needs acting on, and it sat
/// permanently across a chart read for shape. White at low alpha keeps the
/// boundary findable when looked for and out of the way when not — it borrows
/// no hue that already means something on this chart, which is exactly why a
/// merely *dimmer* amber would not do: dimming a reserved colour keeps the
/// promise it makes while making the promise harder to read.
pub const SEAM_LINE: Color32 = Color32::from_rgba_premultiplied(0x3C, 0x3C, 0x3C, 0x3C);
/// `seam/label` — the "venue" caption beside [`SEAM_LINE`], at the same
/// weight as the line it names so the pair reads as one quiet mark rather
/// than a faint rule under a legible word.
pub const SEAM_LABEL: Color32 = Color32::from_rgba_premultiplied(0x50, 0x50, 0x50, 0x50);
/// `warn` — threshold breaches and errors.
pub const WARN: Color32 = Color32::from_rgb(0xFF, 0x63, 0x47);

/// The recording red: the REC control while the venue's deal counter is
/// being written down. Distinct from [`SELL`] on purpose — a sell candle
/// and a recorder are not the same kind of fact — and from [`WARN`], because
/// recording is the healthy state.
pub const REC: Color32 = Color32::from_rgb(0xFF, 0x4D, 0x4D);

/// `gap/line` — the dashed mark where the tape has a hole a reconnect left.
///
/// Louder than [`SEAM_LINE`] and quieter than [`AMBER`], because it sits
/// between the two things they mark. The venue seam is provenance a trader
/// reads once; a gap is *missing data* under the bars beside it, and the
/// honesty rule says it is labelled at the point of reading. It is still not
/// an alarm: nothing is wrong with the chart, there is simply a stretch of the
/// market it never saw.
pub const GAP_LINE: Color32 = Color32::from_rgba_premultiplied(0x78, 0x5C, 0x0A, 0x96);
/// `gap/label` — the caption beside [`GAP_LINE`], naming how long the silence
/// was. Readable without competing with price.
pub const GAP_LABEL: Color32 = Color32::from_rgb(0x9A, 0x82, 0x3A);
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

/// `casing` — the dark base under ink that has to survive whatever is behind
/// it: the liquidity map, a candle body at any of the four appearance
/// presets, or a canvas the trader switched off.
///
/// Deliberately *darker* than [`CANVAS`], because the heat ramp's floor is
/// black: a casing that matches that floor stops separating exactly where
/// separation is needed. Composed over the map's brightest band it still
/// lands near canvas-dark, which is the whole point — it makes the floor a
/// constant instead of a function of whatever happens to be underneath.
///
/// Born in the fixed-range profile, which needed a readable histogram over
/// the depth map; promoted here the day the footprint's ladder needed the
/// same guarantee. A colour copied by hand goes stale the day the theme
/// moves, with no test to notice.
pub const CASING: Color32 = Color32::from_rgba_premultiplied(5, 7, 12, 235);
/// How far a casing extends past the ink it carries: one pixel a side.
pub const CASING_EXTRA_PX: f32 = 2.0;

/// How far a side's ink is mixed toward white. See [`ink`].
const INK_WHITE_MIX: f32 = 0.45;

/// Readable ink in a side's own hue — [`BUY`] and [`SELL`] lightened toward
/// white until they clear 4.5:1 over [`CASING`] and over a side-tinted cell
/// drawn on top of it.
///
/// The raw side colours are chart *fills*, not text: `SELL` written on a
/// sell-tinted pill is 3.2:1, which is how a footprint ends up with its most
/// important cell less legible than its ordinary ones. Lightening keeps the
/// hue — so a column still scans without being read — and buys the contrast
/// the fill colour never had.
#[must_use]
pub fn ink(side: quantick_engine::Side) -> Color32 {
    let base = side_color(side);
    let lighten = |channel: u8| -> u8 {
        let from = f32::from(channel);
        (from + (255.0 - from) * INK_WHITE_MIX).round() as u8
    };
    Color32::from_rgb(lighten(base.r()), lighten(base.g()), lighten(base.b()))
}

/// The fill colour of an aggressor side. One answer for the whole app: every
/// surface that paints buy against sell reads it here.
#[must_use]
pub const fn side_color(side: quantick_engine::Side) -> Color32 {
    match side {
        quantick_engine::Side::Buy => BUY,
        quantick_engine::Side::Sell => SELL,
    }
}

/// `draw/violet` and `draw/cyan` — the two drawing colours no chart element
/// already owns. They exist because the drawing palette needs hues that do
/// not collide with meaning: green and red are the candles, blue is
/// [`ACCENT`], yellow is [`POC`]. Without these two, every drawing colour
/// borrows something that already says something else.
pub const DRAW_VIOLET: Color32 = Color32::from_rgb(0xC5, 0x8A, 0xF9);
/// See [`DRAW_VIOLET`].
pub const DRAW_CYAN: Color32 = Color32::from_rgb(0x4D, 0xD0, 0xE1);

/// The drawing palette offered as one-click swatches on the context bar.
///
/// Eight, because that is what fits one row without the row becoming a grid
/// the trader has to read. Six are tokens that already mean something on
/// this chart, so a colour choice speaks the language the rest of the UI
/// speaks; two are the hues nothing else owns.
///
/// [`AMBER`] is deliberately absent and must stay absent: it is reserved for
/// provenance honesty, and a trader painting a line amber by taste would be
/// borrowing the one colour that promises "this data is not live".
/// Grep-guarded in the context bar, the way the rail guards itself.
pub const DRAWING_SWATCHES: [Color32; 8] = [
    ACCENT,
    TEXT_PRIMARY,
    TEXT_MUTED,
    BUY,
    SELL,
    POC,
    DRAW_VIOLET,
    DRAW_CYAN,
];

/// `shadow/float` — the drop shadow of a surface floating free over the
/// canvas. Docked chrome never uses it: a panel glued to an edge separates
/// itself with its hairline border. A floating strip has no anchored side,
/// and [`CHROME`] over [`CANVAS`] is too close in value to read alone.
pub const FLOAT_SHADOW: Color32 = Color32::from_black_alpha(96);

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

/// WCAG relative luminance of an opaque colour — how *light* it looks, which
/// is not how light its channels average to.
///
/// A saturated blue and a saturated yellow can share a channel average and sit
/// twenty L\* apart; only one of them can carry dark ink. Green weighs seven
/// times what blue does here, and that is the whole reason to spell the
/// formula out rather than take a mean.
#[must_use]
pub fn relative_luminance(color: Color32) -> f32 {
    let linear = |channel: u8| -> f32 {
        let value = f32::from(channel) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
}

/// WCAG contrast ratio between two opaque colours.
#[must_use]
pub fn contrast_ratio(a: Color32, b: Color32) -> f32 {
    let (x, y) = (relative_luminance(a), relative_luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `fill` as it will actually look: composited over the canvas it is drawn on.
///
/// A chip carrying a *faded* colour — the honesty fade a mark wears when this
/// chart's data does not back it — is translucent, and egui's premultiplied
/// `gamma_multiply` scales the channels with the alpha. Grading those channels
/// as if they were opaque asks how the colour reads on **black**, not how it
/// reads on this chart, and a white line faded to 45% then measured that way
/// looks dark enough to want light ink when the composited chip is pale.
#[must_use]
fn over_canvas(fill: Color32) -> Color32 {
    let blend = |channel: u8, base: u8| -> u8 {
        let transparency = f32::from(255 - fill.a()) / 255.0;
        (f32::from(channel) + f32::from(base) * transparency).min(255.0) as u8
    };
    Color32::from_rgb(
        blend(fill.r(), CANVAS.r()),
        blend(fill.g(), CANVAS.g()),
        blend(fill.b(), CANVAS.b()),
    )
}

/// The ink to write on a chip filled with `fill`: whichever of the two reads
/// better on it.
///
/// Where a chip's colour is a constant the ink can be one too — the last-price
/// chip is only ever one of two saturated greens or reds, so it simply wears
/// [`CHIP_INK`]. A chip carrying a *drawing's* colour cannot: the trader picks
/// that colour, and dark navy and pale yellow are both legal.
///
/// Chosen by measuring, not by a lightness threshold. A threshold is only
/// equivalent to measuring when the two inks are black and white; these two
/// are `#0E121A` and `#D2DAE2`, and against that pair the crossover sits where
/// *neither* ink clears 4.5:1 — so a threshold placed at the sRGB midpoint
/// hands some fills the worse of the two options while looking principled.
/// Picking the better one is the most this pair can promise, and
/// `the_ink_is_always_the_better_of_the_two` is what states that honestly.
#[must_use]
pub fn ink_on(fill: Color32) -> Color32 {
    let composited = over_canvas(fill);
    if contrast_ratio(composited, CHIP_INK) >= contrast_ratio(composited, TEXT_PRIMARY) {
        CHIP_INK
    } else {
        TEXT_PRIMARY
    }
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
