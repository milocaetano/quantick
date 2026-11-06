//! The chips written on the chart's axes, as numbers: where one claims the
//! axis so a gridline label stands aside, and the size a price tag's second
//! line is set at so the chip stays inside the price gutter. The host lays
//! the text out and paints; nothing here measures a glyph.
use crate::constants::{AXIS_LABEL_FONT_PX, AXIS_LABEL_GAP_PX, PRICE_GUTTER_PX};

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
