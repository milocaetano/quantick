//! Where a floating settings panel opens beside the object it configures
//! (`docs/ux/drawing-tools-2026-08.md` §D3).
//!
//! Pure screen geometry, so the rule is decided and tested without a UI. The
//! panel opens **beside** the object, so the eye that just clicked it finds
//! its settings next to it, and never on it: the object's box is inflated by
//! the neighbourhood margin first, which keeps the price action right next to
//! the object readable as well.

pub use crate::scripted_hand::Area;

/// The neighbourhood margin the drawing inspector keeps from its object.
pub const NEIGHBOURHOOD_PX: f32 = 32.0;

impl Area {
    /// The rectangle whose top-left is `(left, top)`, of the given size.
    #[must_use]
    pub fn from_min_size(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self::new(left, top, left + width, top + height)
    }

    /// This rectangle grown by `by` on every side.
    #[must_use]
    pub fn expand(self, by: f32) -> Self {
        Self::new(
            self.left - by,
            self.top - by,
            self.right + by,
            self.bottom + by,
        )
    }

    /// Area shared with `other`; zero when they only touch or are apart.
    #[must_use]
    pub fn overlap(self, other: Self) -> f32 {
        let width = self.right.min(other.right) - self.left.max(other.left);
        let height = self.bottom.min(other.bottom) - self.top.max(other.top);
        if width > 0.0 && height > 0.0 {
            width * height
        } else {
            0.0
        }
    }

    fn contains(self, other: Self) -> bool {
        self.left <= other.left
            && self.top <= other.top
            && other.right <= self.right
            && other.bottom <= self.bottom
    }

    fn centre_distance(self, other: Self) -> f32 {
        let dx = (self.left + self.right - other.left - other.right) / 2.0;
        let dy = (self.top + self.bottom - other.top - other.bottom) / 2.0;
        dx.hypot(dy)
    }
}

/// The spacing the caller's chrome uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelSpacing {
    /// Margin kept clear around the object by a placement beside it.
    pub neighbourhood: f32,
    /// Gap from the chart edges, and from the object in the fallbacks.
    pub gap: f32,
    /// The narrowest the panel may be squeezed into a gutter.
    pub min_width: f32,
}

/// Where the panel goes: its top-left corner, and how wide it may be there —
/// `Some` only when it had to be narrowed into a gutter beside an object too
/// big to place around.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPlacement {
    pub left: f32,
    pub top: f32,
    pub max_width: Option<f32>,
}

/// Move a `width` x `height` panel at `(left, top)` the least distance that
/// puts it inside `area` (its top-left edge when it is bigger than `area`).
#[must_use]
pub fn clamp_into(left: f32, top: f32, width: f32, height: f32, area: Area) -> (f32, f32) {
    let max_left = (area.right - width).max(area.left);
    let max_top = (area.bottom - height).max(area.top);
    (
        left.clamp(area.left, max_left),
        top.clamp(area.top, max_top),
    )
}

/// The placement rule:
///
/// 1. the two **horizontal** sides first — right, then left — top-aligned with
///    the object; of those that hold the whole panel inside the chart and
///    clear of the neighbourhood, the one with more room wins, a tie taking
///    the right, so the panel appears in the same place every time;
/// 2. then below and above, centred on the object, on the same rule, a tie
///    taking below.
///
/// Whenever any chart corner is clear of the neighbourhood, one of those four
/// sides is too, so the fallbacks only run when the neighbourhood cannot be
/// kept. They keep the standard `gap` from the bare object instead:
///
/// 3. the two top chart corners, then the two bottom ones — the clear one
///    farthest from the object;
/// 4. the wider gutter beside an object too big for any corner, the panel
///    narrowed to fit;
/// 5. and as a last resort the candidate that covers the object least.
///
/// A caller whose panel turns out a different size than `width` x `height`
/// asks again with the drawn size.
#[must_use]
pub fn place_beside(
    chart: Area,
    object: Area,
    width: f32,
    height: f32,
    spacing: PanelSpacing,
) -> PanelPlacement {
    let whole = |(left, top): (f32, f32)| PanelPlacement {
        left,
        top,
        max_width: None,
    };
    let fit = |left: f32, top: f32| {
        let (left, top) = clamp_into(left, top, width, height, chart);
        (left, top, Area::from_min_size(left, top, width, height))
    };
    // Of two unclamped (score, left, top) candidates, the clear one with the
    // higher score; the first on a tie.
    let best_clear = |candidates: [(f32, f32, f32); 2], clear_of: Area| {
        let mut best: Option<((f32, f32), f32)> = None;
        for (score, left, top) in candidates {
            let (left, top, rect) = fit(left, top);
            if !chart.contains(rect) || rect.overlap(clear_of) > 0.0 {
                continue;
            }
            if best.is_none_or(|(_, best)| score > best) {
                best = Some(((left, top), score));
            }
        }
        best.map(|(at, _)| at)
    };
    let keep_out = object.expand(spacing.neighbourhood);
    let centred_left = (object.left + object.right - width) / 2.0;
    let horizontal = [
        (chart.right - keep_out.right, keep_out.right, object.top),
        (
            keep_out.left - chart.left,
            keep_out.left - width,
            object.top,
        ),
    ];
    let vertical = [
        (
            chart.bottom - keep_out.bottom,
            centred_left,
            keep_out.bottom,
        ),
        (
            keep_out.top - chart.top,
            centred_left,
            keep_out.top - height,
        ),
    ];
    if let Some(at) = best_clear(horizontal, keep_out).or_else(|| best_clear(vertical, keep_out)) {
        return whole(at);
    }

    let gap = spacing.gap;
    // Scored by distance from the object: the farthest clear corner wins.
    let corner = |left: f32, top: f32| {
        let (_, _, rect) = fit(left, top);
        (rect.centre_distance(object), left, top)
    };
    let top_corners = [
        corner(chart.left + gap, chart.top + gap),
        corner(chart.right - gap - width, chart.top + gap),
    ];
    let bottom_corners = [
        corner(chart.left + gap, chart.bottom - gap - height),
        corner(chart.right - gap - width, chart.bottom - gap - height),
    ];
    if let Some(at) = best_clear(top_corners, object).or_else(|| best_clear(bottom_corners, object))
    {
        return whole(at);
    }

    // No corner is clear, which is what a big object — a volume profile, a
    // range across the whole chart — does to all four. The panel goes into
    // the wider *gutter* the object leaves beside it, narrowed to fit:
    // horizontal gutters only, because narrowing reflows a panel and its
    // scroll area keeps every row reachable, while shortening hides rows.
    let left_gutter = object.left - gap - chart.left;
    let right_gutter = chart.right - object.right - gap;
    let (gutter, gutter_left) = if right_gutter >= left_gutter {
        (right_gutter, object.right + gap)
    } else {
        (left_gutter, chart.left + gap)
    };
    if gutter >= spacing.min_width {
        let narrowed = gutter.min(width);
        let (left, top) = clamp_into(gutter_left, chart.top + gap, narrowed, height, chart);
        return PanelPlacement {
            left,
            top,
            max_width: Some(narrowed),
        };
    }

    // Nothing can be placed clear of the object; crowd it least.
    let candidates = [
        (object.right + gap, object.top),
        (object.left - gap - width, object.top),
        (object.left, object.bottom + gap),
        (object.left, object.top - gap - height),
        (chart.left + gap, chart.top + gap),
        (chart.right - gap - width, chart.top + gap),
        (chart.left + gap, chart.bottom - gap - height),
        (chart.right - gap - width, chart.bottom - gap - height),
    ];
    let mut best: Option<((f32, f32), f32, f32)> = None;
    for (left, top) in candidates {
        let (left, top, rect) = fit(left, top);
        let overlap = rect.overlap(object);
        let distance = rect.centre_distance(object);
        let wins = best.is_none_or(|(_, best_overlap, best_distance)| {
            overlap < best_overlap || (overlap == best_overlap && distance > best_distance)
        });
        if wins {
            best = Some(((left, top), overlap, distance));
        }
    }
    whole(best.map_or((chart.left, chart.top), |(at, _, _)| at))
}

#[cfg(test)]
#[path = "panel_placement_tests.rs"]
mod panel_placement_tests;
