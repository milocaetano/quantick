//! Screen-space geometry shared by the tools and the gestures.
//!
//! Every operation copies the `emath` arithmetic it replaces op for op —
//! `length` is `hypot`, not a square root of a sum — so a hit threshold sits
//! on exactly the pixel it sat on when the window did this maths.

use crate::Px;

impl Px {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The straight-line distance to `other`, as `emath::Pos2::distance`.
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

pub fn distance_to_segment(position: Px, start: Px, end: Px) -> f32 {
    let segment = (end.x - start.x, end.y - start.y);
    let length_sq = segment.0 * segment.0 + segment.1 * segment.1;
    if length_sq <= f32::EPSILON {
        return position.distance(start);
    }
    let along = (position.x - start.x) * segment.0 + (position.y - start.y) * segment.1;
    let projection = (along / length_sq).clamp(0.0, 1.0);
    position.distance(Px::new(
        start.x + segment.0 * projection,
        start.y + segment.1 * projection,
    ))
}

/// The unit normal of `direction` — the axis a shape's thickness is measured
/// along. A direction of no length has no normal of its own, so it is given
/// the vertical, which is the axis a chart measures in.
#[must_use]
pub fn unit_normal(direction: Px) -> Px {
    let length = direction.x.hypot(direction.y);
    if length <= f32::EPSILON {
        return Px::new(0.0, 1.0);
    }
    Px::new(-direction.y / length, direction.x / length)
}

/// Push `cursor` off the line through `start`–`end` until it stands at least
/// `floor_px` away from it, keeping exactly where it sits *along* that line.
///
/// The shared half of a tool's pending anchor: a tool whose third anchor
/// gives a shape its thickness is degenerate when that anchor lands on the
/// line the first two drew — a channel of no width, a triangle of no area —
/// and that is precisely where the pointer is standing the instant a drag
/// lets go. Sliding *along* the line is left alone, so the gesture still
/// means what it always meant; only the collapsed case is refused.
///
/// A cursor exactly on the line opens the shape on the normal's own side, so
/// the same gesture always produces the same object.
#[must_use]
pub fn off_line_by(start: Px, end: Px, cursor: Px, floor_px: f32) -> Px {
    let normal = unit_normal(Px::new(end.x - start.x, end.y - start.y));
    let offset = (cursor.x - start.x) * normal.x + (cursor.y - start.y) * normal.y;
    if offset.abs() >= floor_px {
        return cursor;
    }
    let side = if offset < 0.0 { -1.0 } else { 1.0 };
    let push = side * floor_px - offset;
    Px::new(cursor.x + normal.x * push, cursor.y + normal.y * push)
}

/// Hold `cursor` level with `anchor` — the same height, free to slide along
/// the tape.
///
/// Level, and not "the nearest of 0°/45°/90°", because a chart's two axes are
/// not the same kind of thing: one is a price and the other is time, their
/// ratio changes with every zoom, and a 45° line drawn today is a different
/// line after one scroll. Horizontal is the only angle that survives a zoom,
/// and it is the one that means something — a level *is* a price a trader is
/// holding constant. Vertical is an instant, which is what the vertical-line
/// tool is for.
#[must_use]
pub fn level_with(anchor: Px, cursor: Px) -> Px {
    Px::new(cursor.x, anchor.y)
}
