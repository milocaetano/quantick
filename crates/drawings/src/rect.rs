//! An axis-aligned screen rectangle and the anchoring of a box against a
//! point, with `emath`'s arithmetic copied op for op so a tool's geometry
//! lands on the same float the window computed before the tools moved here.

use quantick_chart::geometry::PixelRect;

use crate::Px;

/// A rectangle in screen pixels, `min` the top-left and `max` the
/// bottom-right corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PxRect {
    pub min: Px,
    pub max: Px,
}

/// A closed span on one axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PxRange {
    pub min: f32,
    pub max: f32,
}

impl PxRange {
    #[must_use]
    pub fn contains(self, value: f32) -> bool {
        self.min <= value && value <= self.max
    }
}

impl PxRect {
    pub const NOTHING: Self = Self {
        min: Px::new(f32::INFINITY, f32::INFINITY),
        max: Px::new(-f32::INFINITY, -f32::INFINITY),
    };

    #[must_use]
    pub const fn from_min_max(min: Px, max: Px) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub fn from_min_size(min: Px, size: Px) -> Self {
        Self {
            min,
            max: min + size,
        }
    }

    #[must_use]
    pub fn from_center_size(center: Px, size: Px) -> Self {
        Self {
            min: center - size * 0.5,
            max: center + size * 0.5,
        }
    }

    #[must_use]
    pub fn from_two_pos(a: Px, b: Px) -> Self {
        Self {
            min: Px::new(a.x.min(b.x), a.y.min(b.y)),
            max: Px::new(a.x.max(b.x), a.y.max(b.y)),
        }
    }

    #[must_use]
    pub const fn left(&self) -> f32 {
        self.min.x
    }

    #[must_use]
    pub const fn right(&self) -> f32 {
        self.max.x
    }

    #[must_use]
    pub const fn top(&self) -> f32 {
        self.min.y
    }

    #[must_use]
    pub const fn bottom(&self) -> f32 {
        self.max.y
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    #[must_use]
    pub fn size(&self) -> Px {
        self.max - self.min
    }

    #[must_use]
    pub fn center(&self) -> Px {
        Px {
            x: (self.min.x + self.max.x) / 2.0,
            y: (self.min.y + self.max.y) / 2.0,
        }
    }

    #[must_use]
    pub const fn left_top(&self) -> Px {
        Px::new(self.left(), self.top())
    }

    #[must_use]
    pub const fn right_top(&self) -> Px {
        Px::new(self.right(), self.top())
    }

    #[must_use]
    pub const fn left_bottom(&self) -> Px {
        Px::new(self.left(), self.bottom())
    }

    #[must_use]
    pub const fn right_bottom(&self) -> Px {
        Px::new(self.right(), self.bottom())
    }

    #[must_use]
    pub fn x_range(&self) -> PxRange {
        PxRange {
            min: self.min.x,
            max: self.max.x,
        }
    }

    #[must_use]
    pub fn y_range(&self) -> PxRange {
        PxRange {
            min: self.min.y,
            max: self.max.y,
        }
    }

    #[must_use]
    pub fn contains(&self, p: Px) -> bool {
        self.min.x <= p.x && p.x <= self.max.x && self.min.y <= p.y && p.y <= self.max.y
    }

    #[must_use]
    pub fn contains_rect(&self, other: Self) -> bool {
        self.contains(other.min) && self.contains(other.max)
    }

    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
    }

    #[must_use]
    pub fn expand(self, amnt: f32) -> Self {
        self.expand2(Px::splat(amnt))
    }

    #[must_use]
    pub fn expand2(self, amnt: Px) -> Self {
        Self::from_min_max(self.min - amnt, self.max + amnt)
    }

    #[must_use]
    pub fn shrink(self, amnt: f32) -> Self {
        self.shrink2(Px::splat(amnt))
    }

    #[must_use]
    pub fn shrink2(self, amnt: Px) -> Self {
        Self::from_min_max(self.min + amnt, self.max - amnt)
    }

    #[must_use]
    pub fn translate(self, amnt: Px) -> Self {
        Self::from_min_size(self.min + amnt, self.size())
    }

    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Px::new(self.min.x.min(other.min.x), self.min.y.min(other.min.y)),
            max: Px::new(self.max.x.max(other.max.x), self.max.y.max(other.max.y)),
        }
    }

    /// The smallest rectangle holding every point, as `emath::Rect::from_points`.
    #[must_use]
    pub fn from_points(points: &[Px]) -> Self {
        let mut rect = Self::NOTHING;
        for &p in points {
            rect.extend_with(p);
        }
        rect
    }

    /// Grow to include `p`.
    pub fn extend_with(&mut self, p: Px) {
        self.min = Px::new(self.min.x.min(p.x), self.min.y.min(p.y));
        self.max = Px::new(self.max.x.max(p.x), self.max.y.max(p.y));
    }

    #[must_use]
    pub fn clamp(&self, p: Px) -> Px {
        p.clamp(self.min, self.max)
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.min.is_finite() && self.max.is_finite()
    }
}

impl From<PxRect> for PixelRect {
    fn from(rect: PxRect) -> Self {
        Self {
            left: rect.min.x,
            right: rect.max.x,
            top: rect.min.y,
            bottom: rect.max.y,
        }
    }
}

impl From<PixelRect> for PxRect {
    fn from(rect: PixelRect) -> Self {
        Self::from_min_max(
            Px::new(rect.left, rect.top),
            Px::new(rect.right, rect.bottom),
        )
    }
}

/// Where along one axis a box is anchored to a point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Min,
    Center,
    Max,
}

/// Which point of a box sits on the anchor: `[x, y]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Align2(pub [Align; 2]);

impl Align2 {
    pub const LEFT_TOP: Self = Self([Align::Min, Align::Min]);
    pub const LEFT_CENTER: Self = Self([Align::Min, Align::Center]);
    pub const LEFT_BOTTOM: Self = Self([Align::Min, Align::Max]);
    pub const CENTER_TOP: Self = Self([Align::Center, Align::Min]);
    pub const CENTER_CENTER: Self = Self([Align::Center, Align::Center]);
    pub const CENTER_BOTTOM: Self = Self([Align::Center, Align::Max]);
    pub const RIGHT_TOP: Self = Self([Align::Max, Align::Min]);
    pub const RIGHT_CENTER: Self = Self([Align::Max, Align::Center]);
    pub const RIGHT_BOTTOM: Self = Self([Align::Max, Align::Max]);

    /// The box of `size` placed so its anchored point lands on `pos`.
    #[must_use]
    pub fn anchor_size(self, pos: Px, size: Px) -> PxRect {
        let x = match self.0[0] {
            Align::Min => pos.x,
            Align::Center => pos.x - 0.5 * size.x,
            Align::Max => pos.x - size.x,
        };
        let y = match self.0[1] {
            Align::Min => pos.y,
            Align::Center => pos.y - 0.5 * size.y,
            Align::Max => pos.y - size.y,
        };
        PxRect::from_min_size(Px::new(x, y), size)
    }
}
