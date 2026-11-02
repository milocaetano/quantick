//! Contrast decisions over actual premultiplied renderer bytes.

use crate::{PremultipliedRgba, color::RgbaExt};

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
