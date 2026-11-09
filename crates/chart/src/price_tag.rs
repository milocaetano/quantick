//! The chips written on the chart's axes, as numbers: where one claims the
//! axis so a gridline label stands aside, and the size a price tag's second
//! line is set at so the chip stays inside the price gutter. The host lays
//! the text out and paints; nothing here measures a glyph.
use crate::constants::{AXIS_LABEL_FONT_PX, AXIS_LABEL_GAP_PX, PRICE_GUTTER_PX};
use crate::style::CandleStyle;
use quantick_appearance::{ResolvedTheme, Token};

/// The last-price chip's colour over a bar rising (`up`) or falling.
///
/// Dark wears the candle's own outline colour, so chip and candle always
/// agree. A light theme draws every contour in one dark ink, which says
/// nothing about direction; its chip wears the theme's own pair instead,
/// still one per direction, as TradingView's does.
#[must_use]
pub fn last_price_rgb(theme: &ResolvedTheme, candles: &CandleStyle, up: bool) -> [u8; 3] {
    if theme.dark {
        return if up {
            candles.bull_outline
        } else {
            candles.bear_outline
        };
    }
    let token = if up {
        Token::LastPriceUp
    } else {
        Token::LastPriceDown
    };
    let [r, g, b, _] = theme.color(token).to_array();
    [r, g, b]
}

/// Padding either side of a tag's text, in pixels.
pub const TAG_PAD_X_PX: f32 = 3.0;

/// How much smaller a tag's second line is than its price, in points.
pub const SECOND_LINE_SMALLER_PT: f32 = 1.0;

/// The size a tag's second line is set at when it fits.
pub const SECOND_LINE_PX: f32 = AXIS_LABEL_FONT_PX - SECOND_LINE_SMALLER_PT;

/// The smallest a second line shrinks to so its chip stays in the gutter: the
/// widest line the progress formatter writes (`999.9K/999.9K`) fits at it.
pub const SECOND_LINE_MIN_PX: f32 = 7.0;

/// The width a tag's text has inside the price gutter.
pub const TAG_TEXT_ROOM_PX: f32 = PRICE_GUTTER_PX - AXIS_LABEL_GAP_PX - TAG_PAD_X_PX;

/// Whether a gridline label centred at `at` would share pixels with a chip
/// already written on this axis, and should therefore stand aside.
///
/// `label` and `chip` are the two extents along the axis — heights on the
/// price axis, widths on the time strip. They are asked for separately
/// because they are not the same: the time strip drops its own labels to
/// `HH:MM` when it runs out of room while the pointer's chip stays `HH:MM:SS`,
/// so a rule that assumed one size left a 30 px label overlapping a 54 px chip
/// by a dozen pixels — the very interleaving this exists to remove.
///
/// Two boxes touch once their centres are within half of each extent, plus
/// the padding a chip wears over its text.
///
/// The claims arrive as an iterator so a caller can chain the axis's own chips
/// onto the levels it has already gathered, instead of building a third list
/// every frame. Each is the coordinate the chip will *actually* be drawn at —
/// not the pointer's, which the time strip clamps away from near its edges.
#[must_use]
pub fn claimed(at: f32, label: f32, chip: f32, claims: impl IntoIterator<Item = f32>) -> bool {
    let clearance = (label + chip) / 2.0 + TAG_PAD_X_PX;
    claims
        .into_iter()
        .any(|claim| (claim - at).abs() < clearance)
}

/// The rows a price tag at `y` takes on the axis, as label-sized claims: its
/// price, and `second_line` below it the line it carries, if any — which
/// covers the chip's grown bottom, since a label stands aside within a label.
pub fn price_tag_claims(y: f32, second_line: Option<f32>) -> impl Iterator<Item = f32> {
    std::iter::once(y).chain(second_line.map(|height| y + height))
}

/// The size a tag's second line is set at: [`SECOND_LINE_PX`], smaller in
/// proportion when `width_at_full`, its text measured at that size, would run
/// past [`TAG_TEXT_ROOM_PX`] — never below [`SECOND_LINE_MIN_PX`].
#[must_use]
pub fn second_line_font_px(width_at_full: f32) -> f32 {
    if width_at_full <= TAG_TEXT_ROOM_PX {
        return SECOND_LINE_PX;
    }
    // Half points, cut down: a monospace line scales with its size, and the
    // glyphs' pixel rounding must not push it back over.
    let fitted = (SECOND_LINE_PX * TAG_TEXT_ROOM_PX / width_at_full * 2.0).floor() / 2.0;
    fitted.max(SECOND_LINE_MIN_PX)
}

#[cfg(test)]
#[path = "price_tag_tests.rs"]
mod price_tag_tests;

#[cfg(test)]
mod last_price_tests {
    use super::*;
    use crate::style::CandlePreset;

    /// Light's contours are all black; its chip keeps the green and black it
    /// wore before. Dark's chip stays the candle's outline, custom or not.
    #[test]
    fn the_last_price_chip_keeps_a_colour_per_direction() {
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        let candles = CandlePreset::OrderFlow.style();
        assert_eq!(last_price_rgb(&light, &candles, true), [0x3D, 0x85, 0x40]);
        assert_eq!(last_price_rgb(&light, &candles, false), [0, 0, 0]);
        // The slate plot's chip keeps its outline colours too.
        assert_eq!(
            last_price_rgb(light.bookmap(), &candles, true),
            [0x68, 0xE0, 0xC9]
        );
        assert_eq!(
            last_price_rgb(light.bookmap(), &candles, false),
            [0xFF, 0x89, 0x8B]
        );
        let dark = ResolvedTheme::default();
        let mut custom = candles;
        custom.bull_outline = [1, 2, 3];
        assert_eq!(last_price_rgb(&dark, &custom, true), [1, 2, 3]);
        assert_eq!(last_price_rgb(&dark, &custom, false), candles.bear_outline);
    }
}
