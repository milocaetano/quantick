//! The one rule that decides what dropping a dragged pane does.
//!
//! A canvas corner parks the pane floating there, a canvas edge docks it at
//! that edge, a docked pane's side splits that pane and its centre swaps the
//! two; anywhere else it floats where it lands. The trader has not settled
//! whether a corner should dock or park; this file is the one place that
//! answer lives, so flipping it is an edit here and nowhere else.

pub use super::tree::Edge;
use super::tree::{CanvasPoint, CanvasRect, PaneSlot};

/// How far in from a canvas edge a drop still docks at that edge.
pub const DOCK_EDGE_BAND_PX: f32 = 48.0;
/// How far in from both edges of a corner a drop parks in that corner. Larger
/// than the edge band, so a corner wins over the two edges that meet there.
pub const PARK_CORNER_PX: f32 = 64.0;
/// The gap a parked pane keeps from the corner's two edges.
pub const PARK_MARGIN_PX: f32 = 12.0;
/// How deep a docked pane's side zone reaches in from its edge, at most; a
/// small pane gives a quarter of its extent.
pub const PANE_SIDE_PX: f32 = 96.0;
/// How wide and tall a docked pane's centre zone is, at most; a small pane
/// gives a third of its extent.
pub const PANE_CENTRE_PX: f32 = 120.0;

const _: () = assert!(
    PARK_CORNER_PX > DOCK_EDGE_BAND_PX,
    "a corner zone inside the edge band could never be reached"
);

/// Where a docking pane goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockTarget {
    /// Into the canvas at this edge, taking a fair share of it.
    Canvas(Edge),
    /// Beside the docked pane `slot`, on its `side`: that pane splits.
    Beside { slot: PaneSlot, side: Edge },
    /// Into the docked pane `slot`'s place; that pane floats where the
    /// docking one floated.
    Swap(PaneSlot),
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
    /// Insert the pane into the docked tree there.
    Dock(DockTarget),
    /// Keep it floating, snapped into this corner with [`PARK_MARGIN_PX`].
    Park(Corner),
    /// Keep it floating where it lands.
    Float,
}

/// What dropping a pane with the pointer at `pointer` does on `canvas`, whose
/// docked panes stand at `panes`. A canvas corner wins over its edges, a
/// canvas edge over the panes along it.
#[must_use]
pub fn drop_target(
    canvas: CanvasRect,
    panes: &[(PaneSlot, CanvasRect)],
    pointer: CanvasPoint,
) -> DropTarget {
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
    if across.min(down) <= DOCK_EDGE_BAND_PX {
        return DropTarget::Dock(DockTarget::Canvas(if across <= down {
            if near_left { Edge::Left } else { Edge::Right }
        } else if near_top {
            Edge::Top
        } else {
            Edge::Bottom
        }));
    }
    let zones = panes
        .iter()
        .flat_map(|&(slot, rect)| pane_zones(canvas, slot, rect));
    let mut hit = zones.filter(|(_, zone)| zone.contains(pointer));
    hit.next()
        .map_or(DropTarget::Float, |(target, _)| DropTarget::Dock(target))
}

/// Every docked pane's dock zones on `canvas`: its four sides, then its
/// centre, each where a drop there reaches it, in pane order. What the scene
/// publishes and what [`drop_target`] tests a pointer against.
#[must_use]
pub fn dock_zones(
    canvas: CanvasRect,
    panes: &[(PaneSlot, CanvasRect)],
) -> Vec<(DockTarget, CanvasRect)> {
    (panes.iter())
        .flat_map(|&(slot, rect)| pane_zones(canvas, slot, rect))
        .collect()
}

/// `slot`'s zones: the part of its rect the canvas bands leave, a side strip
/// along each edge less the corners the two strips share, and a box at its
/// centre. A pane too small to give a zone gives none.
fn pane_zones(
    canvas: CanvasRect,
    slot: PaneSlot,
    rect: CanvasRect,
) -> impl Iterator<Item = (DockTarget, CanvasRect)> {
    let band = DOCK_EDGE_BAND_PX;
    let inner = CanvasRect::new(
        canvas.x + band,
        canvas.y + band,
        canvas.w - 2.0 * band,
        canvas.h - 2.0 * band,
    );
    let left = rect.x.max(inner.x);
    let top = rect.y.max(inner.y);
    let right = rect.right().min(inner.right());
    let bottom = rect.bottom().min(inner.bottom());
    let (w, h) = (right - left, bottom - top);
    let (dx, dy) = (PANE_SIDE_PX.min(w / 4.0), PANE_SIDE_PX.min(h / 4.0));
    let (cw, ch) = (PANE_CENTRE_PX.min(w / 3.0), PANE_CENTRE_PX.min(h / 3.0));
    let r = CanvasRect::new;
    let zones = [
        (Edge::Left, r(left, top + dy, dx, h - 2.0 * dy)),
        (Edge::Right, r(right - dx, top + dy, dx, h - 2.0 * dy)),
        (Edge::Top, r(left + dx, top, w - 2.0 * dx, dy)),
        (Edge::Bottom, r(left + dx, bottom - dy, w - 2.0 * dx, dy)),
    ];
    let sides = zones.map(|(side, zone)| (DockTarget::Beside { slot, side }, zone));
    let centre = r(left + (w - cw) / 2.0, top + (h - ch) / 2.0, cw, ch);
    let all = sides.into_iter().chain([(DockTarget::Swap(slot), centre)]);
    all.filter(move |_| w > 0.0 && h > 0.0)
}

/// Where a pane of `size` (width, height) sits parked in `corner`.
#[must_use]
pub fn park_rect(canvas: CanvasRect, corner: Corner, size: (f32, f32)) -> CanvasRect {
    let (w, h) = size;
    let left = canvas.x + PARK_MARGIN_PX;
    let right = canvas.right() - PARK_MARGIN_PX - w;
    let top = canvas.y + PARK_MARGIN_PX;
    let bottom = canvas.bottom() - PARK_MARGIN_PX - h;
    let (x, y) = match corner {
        Corner::TopLeft => (left, top),
        Corner::TopRight => (right, top),
        Corner::BottomLeft => (left, bottom),
        Corner::BottomRight => (right, bottom),
    };
    CanvasRect::new(x, y, w, h)
}

/// `rect` moved, keeping its size, so it stands inside `canvas` as far as
/// its size allows: where a pane dropped outside the canvas floats, never
/// over the toolbar or the window's edge.
#[must_use]
pub fn kept_inside(rect: CanvasRect, canvas: CanvasRect) -> CanvasRect {
    let x = rect.x.min(canvas.right() - rect.w).max(canvas.x);
    let y = rect.y.min(canvas.bottom() - rect.h).max(canvas.y);
    CanvasRect::new(x, y, rect.w, rect.h)
}
