//! Exact gamma-aware color arithmetic shared by themes and drawing scenes.

/// An 8-bit RGBA colour with its colour channels premultiplied by alpha — the
/// byte layout a GPU painter stores, kept here without the painter's type.
///
/// Opaque colours read as plain RGB. A translucent one is premultiplied: the
/// conversion from straight alpha is the renderer's (it is gamma-aware), so
/// this type never performs it and only carries the bytes it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PremultipliedRgba([u8; 4]);

impl PremultipliedRgba {
    /// An opaque colour.
    #[must_use]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }

    /// Already-premultiplied channels, in `[r, g, b, a]` order.
    #[must_use]
    pub const fn from_premultiplied(rgba: [u8; 4]) -> Self {
        Self(rgba)
    }

    /// The premultiplied channels, in `[r, g, b, a]` order.
    #[must_use]
    pub const fn to_array(self) -> [u8; 4] {
        self.0
    }
}

pub const TRANSPARENT: PremultipliedRgba = PremultipliedRgba::from_premultiplied([0, 0, 0, 0]);
pub const WHITE: PremultipliedRgba = PremultipliedRgba::from_rgb(255, 255, 255);

/// Already-premultiplied channels.
#[must_use]
pub const fn rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> PremultipliedRgba {
    PremultipliedRgba::from_premultiplied([r, g, b, a])
}

/// Straight-alpha channels, premultiplied in gamma-aware linear space.
#[must_use]
pub fn rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> PremultipliedRgba {
    match a {
        0 => TRANSPARENT,
        255 => PremultipliedRgba::from_rgb(r, g, b),
        a => {
            // The renderer's own table: every (channel, alpha) pair answered
            // once, so a fill on the frame path costs a lookup, not six powf.
            static LOOKUP: std::sync::OnceLock<Box<[u8; 256 * 256]>> = std::sync::OnceLock::new();
            let lut = LOOKUP.get_or_init(|| {
                let mut table = Box::new([0_u8; 256 * 256]);
                for value in 0..=255_u8 {
                    for alpha in 0..=255_u8 {
                        table[usize::from(value) * 256 + usize::from(alpha)] =
                            gamma_u8_from_linear_f32(
                                linear_f32_from_gamma_u8(value) * (f32::from(alpha) / 255.0),
                            );
                    }
                }
                table
            });
            let [r, g, b] = [r, g, b].map(|value| lut[usize::from(value) * 256 + usize::from(a)]);
            rgba_premultiplied(r, g, b, a)
        }
    }
}

fn linear_f32_from_gamma_u8(s: u8) -> f32 {
    if s <= 10 {
        f32::from(s) / 3294.6
    } else {
        ((f32::from(s) + 14.025) / 269.025).powf(2.4)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=255 before the cast, as ecolor does"
)]
fn fast_round(r: f32) -> u8 {
    (r + 0.5) as u8
}

fn gamma_u8_from_linear_f32(l: f32) -> u8 {
    if l <= 0.0 {
        0
    } else if l <= 0.003_130_8 {
        fast_round(3294.6 * l)
    } else if l <= 1.0 {
        fast_round(269.025 * l.powf(1.0 / 2.4) - 14.025)
    } else {
        255
    }
}

/// The channel reads and the fade a renderer's colour offers.
pub trait RgbaExt: Sized {
    fn r(self) -> u8;
    fn g(self) -> u8;
    fn b(self) -> u8;
    fn a(self) -> u8;
    /// Every channel, alpha included, scaled by `factor` in gamma space.
    #[must_use]
    fn gamma_multiply(self, factor: f32) -> Self;
}

impl RgbaExt for PremultipliedRgba {
    fn r(self) -> u8 {
        self.to_array()[0]
    }
    fn g(self) -> u8 {
        self.to_array()[1]
    }
    fn b(self) -> u8 {
        self.to_array()[2]
    }
    fn a(self) -> u8 {
        self.to_array()[3]
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a factor in 0..=1 keeps every channel in u8 range, as ecolor assumes"
    )]
    fn gamma_multiply(self, factor: f32) -> Self {
        let [r, g, b, a] = self.to_array();
        let scale = |channel: u8| (f32::from(channel) * factor + 0.5) as u8;
        rgba_premultiplied(scale(r), scale(g), scale(b), scale(a))
    }
}

/// Compose a presentation opacity with configured alpha, in straight color space.
pub fn with_opacity(color: PremultipliedRgba, opacity: u8) -> PremultipliedRgba {
    let a = color.a();
    if a == 0 {
        return TRANSPARENT;
    }
    let [r, g, b, _] = opaque(color).to_array();
    let alpha = (f32::from(a) * f32::from(opacity) / 255.0).round() as u8;
    rgba_unmultiplied(r, g, b, alpha)
}

/// The colour's own hue at full opacity: what it is, apart from how faintly
/// it is laid down. A transparent colour has no hue and answers transparent.
pub fn opaque(color: PremultipliedRgba) -> PremultipliedRgba {
    let [r, g, b, a] = color.to_array();
    if a == 0 {
        return TRANSPARENT;
    }
    let straight = |channel| {
        gamma_u8_from_linear_f32(linear_f32_from_gamma_u8(channel) / (f32::from(a) / 255.0))
    };
    PremultipliedRgba::from_rgb(straight(r), straight(g), straight(b))
}
