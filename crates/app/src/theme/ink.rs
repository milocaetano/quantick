//! Colours derived from other colours: the ink a fill can carry, the tint
//! a control wears while it is held, and the contrast arithmetic behind
//! both.

use eframe::egui::Color32;

use super::{BUY, CANVAS, CHIP_INK, SELL, TEXT_PRIMARY};

/// How far a side's ink is mixed toward white. See [`ink`].
pub const INK_WHITE_MIX: f32 = 0.45;

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

/// Alpha of an "active" tint: a layer accent at 22% over the chrome.
pub const ACTIVE_TINT_ALPHA: u8 = 56; // ≈ 22% of 255

/// Alpha of a "pressed" tint: a layer accent at 33% over the chrome. One
/// step deeper than [`ACTIVE_TINT_ALPHA`], so a press on an already-active
/// button is still visible.
pub const PRESS_TINT_ALPHA: u8 = 84;

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
pub fn over_canvas(fill: Color32) -> Color32 {
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
