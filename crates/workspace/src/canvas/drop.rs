//! The one rule that decides what dropping a dragged pane does.
//!
//! An edge docks, a corner parks the pane floating there, anywhere else it
//! floats where it lands. The trader has not settled whether a corner should
//! dock or park; this file is the one place that answer lives, so flipping it
//! is an edit here and nowhere else.

use super::tree::{CanvasPoint, CanvasRect};

/// How far in from a canvas edge a drop still docks at that edge.
pub const DOCK_EDGE_BAND_PX: f32 = 48.0;
/// How far in from both edges of a corner a drop parks in that corner. Larger
/// than the edge band, so a corner wins over the two edges that meet there.
pub const PARK_CORNER_PX: f32 = 64.0;
/// The gap a parked pane keeps from the corner's two edges.
pub const PARK_MARGIN_PX: f32 = 12.0;

const _: () = assert!(
    PARK_CORNER_PX > DOCK_EDGE_BAND_PX,
    "a corner zone inside the edge band could never be reached"
);

/// A canvas edge a pane can dock at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// A canvas corner a floating pane can park in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What a drop at a point does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// Insert the pane into the canvas at this edge.
    Dock(Edge),
    /// Keep it floating, snapped into this corner with [`PARK_MARGIN_PX`].
    Park(Corner),
    /// Keep it floating where it lands.
    Float,
}

/// What dropping a pane with the pointer at `pointer` does on `canvas`.
#[must_use]
pub fn drop_target(canvas: CanvasRect, pointer: CanvasPoint) -> DropTarget {
    if !canvas.contains(pointer) {
        return DropTarget::Float;
    }
    let left = pointer.x - canvas.x;
    let right = canvas.right() - pointer.x;
    let top = pointer.y - canvas.y;
    let bottom = canvas.bottom() - pointer.y;
    let near_left = left <= right;
    let near_top = top <= bottom;
    let across = left.min(right);
    let down = top.min(bottom);
    if across <= PARK_CORNER_PX && down <= PARK_CORNER_PX {
        return DropTarget::Park(match (near_top, near_left) {
            (true, true) => Corner::TopLeft,
            (true, false) => Corner::TopRight,
            (false, true) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        });
    }
    if across.min(down) > DOCK_EDGE_BAND_PX {
        return DropTarget::Float;
    }
    DropTarget::Dock(if across <= down {
        if near_left { Edge::Left } else { Edge::Right }
    } else if near_top {
        Edge::Top
    } else {
        Edge::Bottom
    })
}

/// Where a pane of `size` (width, height) sits parked in `corner`, clear of
/// every folded chart's strip in `strips`: one in the way pushes it off
/// along the strip's short side, keeping [`PARK_MARGIN_PX`] from it, so the
/// strip's restore stays in reach.
#[must_use]
pub fn park_rect(
    canvas: CanvasRect,
    corner: Corner,
    size: (f32, f32),
    strips: &[CanvasRect],
) -> CanvasRect {
    let (w, h) = size;
    let from_left = matches!(corner, Corner::TopLeft | Corner::BottomLeft);
    let from_top = matches!(corner, Corner::TopLeft | Corner::TopRight);
    let left = canvas.x + PARK_MARGIN_PX;
    let right = canvas.right() - PARK_MARGIN_PX - w;
    let top = canvas.y + PARK_MARGIN_PX;
    let bottom = canvas.bottom() - PARK_MARGIN_PX - h;
    let mut at = CanvasRect::new(
        if from_left { left } else { right },
        if from_top { top } else { bottom },
        w,
        h,
    );
    let overlaps = |a: CanvasRect, b: CanvasRect| {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    };
    // The pane only moves away from its corner, so a strip once cleared stays
    // clear: as many passes as strips do.
    for _ in 0..strips.len() {
        let Some(strip) = strips.iter().find(|strip| overlaps(at, **strip)) else {
            break;
        };
        match (strip.h >= strip.w, from_left, from_top) {
            (true, true, _) => at.x = strip.right() + PARK_MARGIN_PX,
            (true, false, _) => at.x = strip.x - PARK_MARGIN_PX - w,
            (false, _, true) => at.y = strip.bottom() + PARK_MARGIN_PX,
            (false, _, false) => at.y = strip.y - PARK_MARGIN_PX - h,
        }
    }
    // Never pushed off the canvas: its grip has to stay in reach.
    at.x = at.x.min(right).max(left);
    at.y = at.y.min(bottom).max(top);
    at
}
