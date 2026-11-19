//! The one rule that decides what dropping a dragged pane does.
//!
//! Canvas corners park, edge bands dock, and a compass over the hovered pane
//! offers splits and Swap. Painting, scene discovery and release share these zones.

pub use super::tree::Edge;
use super::tree::{CanvasPoint, CanvasRect, PaneSlot};

/// How far in from a canvas edge a drop still docks at that edge.
pub const DOCK_EDGE_BAND_PX: f32 = 48.0;
/// How far in from both edges of a corner a drop parks in that corner. Larger
/// than the edge band, so a corner wins over the two edges that meet there.
pub const PARK_CORNER_PX: f32 = 64.0;
/// The gap a parked pane keeps from the corner's two edges.
pub const PARK_MARGIN_PX: f32 = 12.0;
/// Size of a docking guide and the space between guides in its compass.
pub const DOCK_GUIDE_PX: f32 = 44.0;
pub const DOCK_GUIDE_GAP_PX: f32 = 4.0;
/// Below this size the pane offers only its center, keeping Swap readable.
pub const MIN_DOCK_GUIDE_PX: f32 = 32.0;

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
    drop_guides(canvas, panes, Some(pointer))
        .selected
        .unwrap_or(DropTarget::Float)
}

/// Every zone a drop docks or parks in on `canvas`, in the order the drop
/// rule tries them: four corners, four edge bands, then each docked pane's
/// compass, in pane order. What the scene publishes and
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
        (
            Edge::Left,
            r(canvas.x, canvas.y + corner, band, canvas.h - 2.0 * corner),
        ),
        (
            Edge::Right,
            r(
                canvas.right() - band,
                canvas.y + corner,
                band,
                canvas.h - 2.0 * corner,
            ),
        ),
        (
            Edge::Top,
            r(canvas.x + corner, canvas.y, canvas.w - 2.0 * corner, band),
        ),
        (
            Edge::Bottom,
            r(
                canvas.x + corner,
                canvas.bottom() - band,
                canvas.w - 2.0 * corner,
                band,
            ),
        ),
    ];
    let corners = corners.map(|(at, zone)| (DropTarget::Park(at), zone));
    let edges = edges.map(|(at, zone)| (DropTarget::Dock(DockTarget::Canvas(at)), zone));
    let panes = (panes.iter())
        .flat_map(|&(slot, rect)| pane_zones(canvas, slot, rect))
        .map(|(target, zone)| (DropTarget::Dock(target), zone));
    corners.into_iter().chain(edges).chain(panes).collect()
}

pub struct DropGuides {
    pub zones: Vec<(DropTarget, CanvasRect)>,
    pub selected: Option<DropTarget>,
}

/// The visible guides: canvas destinations and the pane under the pointer.
/// Without a pointer, publishes every destination for discovery.
#[must_use]
pub fn drop_guides(
    canvas: CanvasRect,
    panes: &[(PaneSlot, CanvasRect)],
    pointer: Option<CanvasPoint>,
) -> DropGuides {
    let panes: Vec<_> = panes
        .iter()
        .copied()
        .filter(|(_, rect)| pointer.is_none_or(|point| rect.contains(point)))
        .collect();
    let zones = drop_zones(canvas, &panes);
    let selected = pointer.and_then(|at| {
        zones
            .iter()
            .find(|(_, rect)| rect.contains(at))
            .map(|(target, _)| *target)
    });
    DropGuides { zones, selected }
}

/// A compass inside the canvas bands; narrow panes keep a readable Swap target.
fn pane_zones(
    canvas: CanvasRect,
    slot: PaneSlot,
    rect: CanvasRect,
) -> impl Iterator<Item = (DockTarget, CanvasRect)> {
    let band = PARK_CORNER_PX + DOCK_GUIDE_GAP_PX;
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
    let gap = DOCK_GUIDE_GAP_PX;
    let size = DOCK_GUIDE_PX.min((w.min(h) - 2.0 * gap) / 3.0);
    let centre_size = size.max(MIN_DOCK_GUIDE_PX);
    let r = CanvasRect::new;
    let mut centre = r(0.0, 0.0, centre_size, centre_size).centred_in(rect);
    let inset = if size >= MIN_DOCK_GUIDE_PX {
        size + gap
    } else {
        0.0
    };
    centre.x = centre.x.min(right - centre_size - inset).max(left + inset);
    centre.y = centre.y.min(bottom - centre_size - inset).max(top + inset);
    let (x, y, step) = (centre.x, centre.y, size + gap);
    let zones = [
        (Edge::Left, r(x - step, y, size, size)),
        (Edge::Right, r(x + step, y, size, size)),
        (Edge::Top, r(x, y - step, size, size)),
        (Edge::Bottom, r(x, y + step, size, size)),
    ];
    let sides = zones
        .into_iter()
        .filter(move |_| size >= MIN_DOCK_GUIDE_PX)
        .map(move |(side, zone)| (DockTarget::Beside { slot, side }, zone));
    let all = sides.into_iter().chain([(DockTarget::Swap(slot), centre)]);
    all.filter(move |_| w >= MIN_DOCK_GUIDE_PX && h >= MIN_DOCK_GUIDE_PX)
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

/// `rect` moved, keeping its size, so it stands inside `canvas` as far as
/// its size allows: where a pane dropped outside the canvas floats, never
/// over the toolbar or the window's edge.
#[must_use]
pub fn kept_inside(rect: CanvasRect, canvas: CanvasRect) -> CanvasRect {
    let x = rect.x.min(canvas.right() - rect.w).max(canvas.x);
    let y = rect.y.min(canvas.bottom() - rect.h).max(canvas.y);
    CanvasRect::new(x, y, rect.w, rect.h)
}
