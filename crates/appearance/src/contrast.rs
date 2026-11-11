//! Contrast decisions over actual premultiplied renderer bytes.

use crate::{
    PremultipliedRgba,
    color::{RgbaExt, opaque, with_opacity},
};

pub const INK_WHITE_MIX: f32 = 0.45;
pub const ACTIVE_TINT_ALPHA: u8 = 56;
pub const PRESS_TINT_ALPHA: u8 = 84;

pub fn side_ink(base: PremultipliedRgba) -> PremultipliedRgba {
    let lighten = |channel: u8| -> u8 {
        let from = f32::from(channel);
        (from + (255.0 - from) * INK_WHITE_MIX).round() as u8
    };
    PremultipliedRgba::from_rgb(lighten(base.r()), lighten(base.g()), lighten(base.b()))
}

pub fn relative_luminance(color: PremultipliedRgba) -> f32 {
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

pub fn contrast_ratio(a: PremultipliedRgba, b: PremultipliedRgba) -> f32 {
    let (x, y) = (relative_luminance(a), relative_luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Composite a premultiplied fill over its actual background before grading it.
pub fn over(fill: PremultipliedRgba, background: PremultipliedRgba) -> PremultipliedRgba {
    let blend = |channel: u8, base: u8| -> u8 {
        let transparency = f32::from(255 - fill.a()) / 255.0;
        (f32::from(channel) + f32::from(base) * transparency).min(255.0) as u8
    };
    PremultipliedRgba::from_rgb(
        blend(fill.r(), background.r()),
        blend(fill.g(), background.g()),
        blend(fill.b(), background.b()),
    )
}

/// Choose the better of two configured inks, without claiming every pair can
/// meet a fixed contrast ratio on every possible explicit user color.
pub fn ink_on(
    fill: PremultipliedRgba,
    background: PremultipliedRgba,
    first: PremultipliedRgba,
    second: PremultipliedRgba,
) -> PremultipliedRgba {
    let composited = over(fill, background);
    if contrast_ratio(composited, first) >= contrast_ratio(composited, second) {
        first
    } else {
        second
    }
}

/// The least contrast, against what it is painted over, at which a drawing's
/// authored colour is still painted as authored.
///
/// Deliberately far below WCAG's 3:1 for graphics: that is a target for a
/// palette someone designs, and holding a trader's chosen colour to it would
/// overrule a soft amber or cyan he picked on purpose. This catches the colour
/// that has disappeared - near-white on a light canvas (1.07:1), near-black on
/// a dark one - and leaves every bundled default and swatch alone on both.
pub const MIN_DRAWING_CONTRAST: f32 = 1.5;

/// The colour a drawing is painted in over `background`: its authored `color`,
/// or, when that has all but vanished there, the same hue shaded toward black
/// (on a light background) or white (on a dark one) just far enough to reach
/// [`MIN_DRAWING_CONTRAST`]. Yellow on white paints a deeper yellow, never a
/// borrowed grey: the trader picked the hue, the canvas only moves its shade.
///
/// The hue is graded, not the translucent paint: a faint stroke is a choice
/// of faintness, so the shade keeps the authored alpha. Display only - the
/// saved style is never rewritten, so the colour returns as soon as the
/// canvas it was chosen for does.
#[must_use]
pub fn legible(color: PremultipliedRgba, background: PremultipliedRgba) -> PremultipliedRgba {
    let seen = |ink: PremultipliedRgba| contrast_ratio(ink, background);
    let hue = opaque(color);
    if color.a() == 0 || seen(hue) >= MIN_DRAWING_CONTRAST {
        return color;
    }
    let black = PremultipliedRgba::from_rgb(0, 0, 0);
    let white = PremultipliedRgba::from_rgb(255, 255, 255);
    let toward = if seen(black) >= seen(white) {
        black
    } else {
        white
    };
    let shade = shade_until_seen(hue, toward, |ink| seen(ink) >= MIN_DRAWING_CONTRAST);
    if color.a() == u8::MAX {
        shade
    } else {
        with_opacity(shade, color.a())
    }
}

/// Steps the binary search takes along the hue-to-`toward` mix: 2^-12 of the
/// way is finer than one 8-bit channel step.
const SHADE_SEARCH_STEPS: u32 = 12;

/// The least mix of `hue` toward `toward` that `reaches`, found by a fixed
/// binary search so the answer is deterministic; `toward` itself when no mix
/// short of it does.
fn shade_until_seen(
    hue: PremultipliedRgba,
    toward: PremultipliedRgba,
    reaches: impl Fn(PremultipliedRgba) -> bool,
) -> PremultipliedRgba {
    let mix = |amount: f32| {
        let channel = |from: u8, to: u8| -> u8 {
            (f32::from(from) + (f32::from(to) - f32::from(from)) * amount).round() as u8
        };
        PremultipliedRgba::from_rgb(
            channel(hue.r(), toward.r()),
            channel(hue.g(), toward.g()),
            channel(hue.b(), toward.b()),
        )
    };
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..SHADE_SEARCH_STEPS {
        let middle = (low + high) / 2.0;
        if reaches(mix(middle)) {
            high = middle;
        } else {
            low = middle;
        }
    }
    let shade = mix(high);
    if reaches(shade) { shade } else { toward }
}

impl crate::ResolvedTheme {
    /// Theme-owned foreground selection for an authored fill.
    pub fn ink_on(&self, fill: PremultipliedRgba) -> PremultipliedRgba {
        ink_on(
            fill,
            self.color(crate::Token::Canvas),
            self.color(crate::Token::ChipInk),
            self.color(crate::Token::TextPrimary),
        )
    }

    /// The plate and ink of a drawing's price-axis tag in the drawing's own
    /// colour: the colour as a filled plate with computed ink on it, or —
    /// where the theme turns label plates off — no plate and the colour,
    /// made legible on the canvas as the line is, as the ink.
    #[must_use]
    pub fn drawing_tag_paint(
        &self,
        color: PremultipliedRgba,
    ) -> (PremultipliedRgba, PremultipliedRgba) {
        if self.drawing_label_plates {
            (color, self.ink_on(color))
        } else {
            (crate::color::TRANSPARENT, self.legible_drawing(color))
        }
    }

    /// A drawing's authored colour as it is painted on this theme's canvas;
    /// see [`legible`].
    #[must_use]
    pub fn legible_drawing(&self, color: PremultipliedRgba) -> PremultipliedRgba {
        legible(color, self.color(crate::Token::Canvas))
    }

    /// A drawing's colour as it is painted on this surface when its tool's
    /// colour role is `role`. A colour that is still the role's root default
    /// (the colour a new drawing is born in) takes this surface's own colour
    /// for the role, so the flow pane's VWAP reads in the flow pane's VWAP
    /// colour; any other colour is authored and painted as
    /// [`Self::legible_drawing`] paints it.
    #[must_use]
    pub fn legible_drawing_as(
        &self,
        color: PremultipliedRgba,
        role: crate::Token,
    ) -> PremultipliedRgba {
        let born = color == self.root_color(role);
        self.legible_drawing(if born { self.color(role) } else { color })
    }

    /// Readout text as it is painted on the readout plate, which sits over
    /// the canvas — or on the canvas itself where the theme paints no label
    /// plates; see [`legible`].
    #[must_use]
    pub fn legible_readout(&self, color: PremultipliedRgba) -> PremultipliedRgba {
        let canvas = self.color(crate::Token::Canvas);
        if !self.drawing_label_plates {
            return legible(color, canvas);
        }
        legible(
            color,
            over(self.color(crate::Token::DrawingReadoutPlate), canvas),
        )
    }
}
