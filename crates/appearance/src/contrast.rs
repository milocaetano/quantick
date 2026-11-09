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
/// unless its hue is all but invisible there and `fallback` reads better.
///
/// The hue is graded, not the translucent paint: a faint stroke is a choice
/// of faintness, so the fallback keeps the authored alpha. Display only - the
/// saved style is never rewritten, so the colour returns as soon as the
/// canvas it was chosen for does.
#[must_use]
pub fn legible(
    color: PremultipliedRgba,
    background: PremultipliedRgba,
    fallback: PremultipliedRgba,
) -> PremultipliedRgba {
    let seen = |ink: PremultipliedRgba| contrast_ratio(opaque(ink), background);
    let authored = seen(color);
    if color.a() == 0 || authored >= MIN_DRAWING_CONTRAST || seen(fallback) <= authored {
        color
    } else if color.a() == u8::MAX {
        fallback
    } else {
        with_opacity(fallback, color.a())
    }
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

    /// A drawing's authored colour as it is painted on this theme's canvas;
    /// see [`legible`].
    #[must_use]
    pub fn legible_drawing(&self, color: PremultipliedRgba) -> PremultipliedRgba {
        legible(
            color,
            self.color(crate::Token::Canvas),
            self.color(crate::Token::DrawingDefault),
        )
    }

    /// Readout text as it is painted on the readout plate, which sits over
    /// the canvas; see [`legible`].
    #[must_use]
    pub fn legible_readout(&self, color: PremultipliedRgba) -> PremultipliedRgba {
        let plate = over(
            self.color(crate::Token::DrawingReadoutPlate),
            self.color(crate::Token::Canvas),
        );
        legible(color, plate, self.color(crate::Token::DrawingDefault))
    }
}
