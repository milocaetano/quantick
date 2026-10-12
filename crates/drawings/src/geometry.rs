//! Screen-space geometry shared by the tools and the gestures.
//!
//! Every operation copies the `emath` arithmetic it replaces op for op —
//! `length` is `hypot`, not a square root of a sum — so a hit threshold sits
//! on exactly the pixel it sat on when the window did this maths, and a
//! painted shape lands on exactly the same float. [`Px`] is both a position
//! and an offset, the way a renderer's point and vector share one layout.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use crate::Px;

/// A point (or offset) in screen pixels.
#[must_use]
pub const fn pos2(x: f32, y: f32) -> Px {
    Px { x, y }
}

/// An offset in screen pixels; the same type as a point.
#[must_use]
pub const fn vec2(x: f32, y: f32) -> Px {
    Px { x, y }
}

impl Px {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub const fn splat(value: f32) -> Self {
        Self { x: value, y: value }
    }

    /// The straight-line distance to `other`, as `emath::Pos2::distance`.
    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    #[must_use]
    pub fn distance_sq(self, other: Self) -> f32 {
        (self - other).length_sq()
    }

    #[must_use]
    pub fn length(self) -> f32 {
        self.x.hypot(self.y)
    }

    #[must_use]
    pub fn length_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    /// Unit length, or unchanged when it has none.
    #[must_use]
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len <= 0.0 { self } else { self / len }
    }

    /// Rotated a quarter turn: `(y, -x)`.
    #[must_use]
    pub const fn rot90(self) -> Self {
        Self::new(self.y, -self.x)
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    #[must_use]
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(self.x.clamp(min.x, max.x), self.y.clamp(min.y, max.y))
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Px {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Px {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Px {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Px {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Neg for Px {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

impl Mul<f32> for Px {
    type Output = Self;
    fn mul(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor)
    }
}

impl Mul<Px> for f32 {
    type Output = Px;
    fn mul(self, vec: Px) -> Px {
        Px::new(self * vec.x, self * vec.y)
    }
}

impl Div<f32> for Px {
    type Output = Self;
    fn div(self, factor: f32) -> Self {
        Self::new(self.x / factor, self.y / factor)
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
