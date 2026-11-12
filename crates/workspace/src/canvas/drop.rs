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
    /// Beside the docked pane `slot`, on its `side`: into that pane's split
    /// when it lays panes that way, for a fair share of it, else that pane
    /// splits in half.
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
/// docked panes stand at `panes`: the first of [`drop_zones`] holding it, so
/// a canvas corner wins over its edges and a canvas edge over the panes along
/// it; anywhere else, and outside the canvas, the pane floats.
#[must_use]
pub fn drop_target(
    canvas: CanvasRect,
    panes: &[(PaneSlot, CanvasRect)],
    pointer: CanvasPoint,
) -> DropTarget {
    let zones = drop_zones(canvas, panes).into_iter();
    let mut hit = zones.filter(|(_, zone)| zone.contains(pointer));
    hit.next().map_or(DropTarget::Float, |(target, _)| target)
}

/// Every zone a drop docks or parks in on `canvas`, in the order the drop
/// rule tries them: the four corners, the four edge bands, then each docked
/// pane's four sides and centre, in pane order. What the scene publishes and
/// what [`drop_target`] tests a pointer against, so the two never disagree.
#[must_use]
pub fn drop_zones(
    canvas: CanvasRect,
    panes: &[(PaneSlot, CanvasRect)],
) -> Vec<(DropTarget, CanvasRect)> {
    let (band, corner) = (DOCK_EDGE_BAND_PX, PARK_CORNER_PX);
    let (far_x, far_y) = (canvas.right() - corner, canvas.bottom() - corner);
    let r = CanvasRect::new;
    let corners = [
        (Corner::TopLeft, r(canvas.x, canvas.y, corner, corner)),
        (Corner::TopRight, r(far_x, canvas.y, corner, corner)),
        (Corner::BottomLeft, r(canvas.x, far_y, corner, corner)),
        (Corner::BottomRight, r(far_x, far_y, corner, corner)),
    ];
    let edges = [
        (Edge::Left, r(canvas.x, canvas.y, band, canvas.h)),
        (
            Edge::Right,
            r(canvas.right() - band, canvas.y, band, canvas.h),
        ),
        (Edge::Top, r(canvas.x, canvas.y, canvas.w, band)),
        (
            Edge::Bottom,
            r(canvas.x, canvas.bottom() - band, canvas.w, band),
        ),
    ];
    let corners = corners.map(|(at, zone)| (DropTarget::Park(at), zone));
    let edges = edges.map(|(at, zone)| (DropTarget::Dock(DockTarget::Canvas(at)), zone));
    let panes = (panes.iter())
        .flat_map(|&(slot, rect)| pane_zones(canvas, slot, rect))
        .map(|(target, zone)| (DropTarget::Dock(target), zone));
    corners.into_iter().chain(edges).chain(panes).collect()
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
