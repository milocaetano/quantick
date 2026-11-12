//! The one drop rule: edges and panes dock, corners park, anywhere else
//! floats.

use super::*;

const CANVAS: CanvasRect = CanvasRect::new(100.0, 50.0, 1600.0, 900.0);

fn at(x: f32, y: f32) -> DropTarget {
    drop_target(CANVAS, &[], CanvasPoint::new(x, y))
}

#[test]
fn edges_dock_corners_park_elsewhere_floats() {
    let (left, top) = (CANVAS.x, CANVAS.y);
    let (right, bottom) = (CANVAS.right(), CANVAS.bottom());
    let (mid_x, mid_y) = (left + CANVAS.w / 2.0, top + CANVAS.h / 2.0);

    // Inside each edge band, away from the corners.
    assert_eq!(
        at(left + 5.0, mid_y),
        DropTarget::Dock(DockTarget::Canvas(Edge::Left))
    );
    assert_eq!(
        at(right - 5.0, mid_y),
        DropTarget::Dock(DockTarget::Canvas(Edge::Right))
    );
    assert_eq!(
        at(mid_x, top + 5.0),
        DropTarget::Dock(DockTarget::Canvas(Edge::Top))
    );
    assert_eq!(
        at(mid_x, bottom - 5.0),
        DropTarget::Dock(DockTarget::Canvas(Edge::Bottom))
    );
    assert_eq!(
        at(left + DOCK_EDGE_BAND_PX, mid_y),
        DropTarget::Dock(DockTarget::Canvas(Edge::Left)),
        "the band includes its inner edge"
    );

    // A corner wins over the two edges that meet there.
    assert_eq!(at(left + 5.0, top + 5.0), DropTarget::Park(Corner::TopLeft));
    assert_eq!(
        at(right - 5.0, top + 5.0),
        DropTarget::Park(Corner::TopRight)
    );
    assert_eq!(
        at(left + 5.0, bottom - 5.0),
        DropTarget::Park(Corner::BottomLeft)
    );
    assert_eq!(
        at(right - PARK_CORNER_PX, bottom - PARK_CORNER_PX),
        DropTarget::Park(Corner::BottomRight)
    );

    // Past the band, and outside the canvas, a pane floats where it lands.
    assert_eq!(at(mid_x, mid_y), DropTarget::Float);
    assert_eq!(at(left + DOCK_EDGE_BAND_PX + 1.0, mid_y), DropTarget::Float);
    assert_eq!(at(left - 1.0, mid_y), DropTarget::Float);
    assert_eq!(at(mid_x, bottom + 40.0), DropTarget::Float);

    // Parking snaps the pane into its corner with the 12 px margin.
    assert_eq!(PARK_MARGIN_PX, 12.0);
    let size = (400.0, 300.0);
    assert_eq!(
        park_rect(CANVAS, Corner::TopLeft, size),
        CanvasRect::new(left + 12.0, top + 12.0, 400.0, 300.0)
    );
    assert_eq!(
        park_rect(CANVAS, Corner::BottomRight, size),
        CanvasRect::new(right - 12.0 - 400.0, bottom - 12.0 - 300.0, 400.0, 300.0)
    );
    assert_eq!(
        park_rect(CANVAS, Corner::TopRight, size),
        CanvasRect::new(right - 12.0 - 400.0, top + 12.0, 400.0, 300.0)
    );
    assert_eq!(
        park_rect(CANVAS, Corner::BottomLeft, size),
        CanvasRect::new(left + 12.0, bottom - 12.0 - 300.0, 400.0, 300.0)
    );
}

/// A canvas laid out as `id`, with a chart floating over it; the floating
/// chart's slot.
fn floating_over(id: &str) -> (CanvasState, PaneSlot) {
    let mut state = CanvasState::from_preset(preset(id).unwrap());
    let slot = state.add(PaneKind::Time, CANVAS).unwrap();
    (state, slot)
}

fn rect_of(state: &CanvasState, slot: PaneSlot) -> CanvasRect {
    let rects = state.rects(CANVAS);
    rects.into_iter().find(|(held, _)| *held == slot).unwrap().1
}

fn centre(rect: CanvasRect) -> CanvasPoint {
    CanvasPoint::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

/// Every zone a docked pane offers is reached by a drop at its centre, the
/// canvas edges keep winning along the panes beside them, and between the
/// zones a pane still floats.
#[test]
fn every_pane_zone_is_reached_where_it_is_published() {
    let (state, _) = floating_over("charts-4");
    let panes = open_panes(&state, CANVAS);
    let zones = dock_zones(CANVAS, &panes);
    assert_eq!(zones.len(), 4 * 5, "four sides and a centre per pane");
    for (target, zone) in &zones {
        assert_eq!(
            drop_target(CANVAS, &panes, centre(*zone)),
            DropTarget::Dock(*target),
            "{target:?} at {zone:?}"
        );
    }
    let first = rect_of(&state, 0);
    let along_left = CanvasPoint::new(CANVAS.x + 5.0, first.y + first.h / 2.0);
    assert_eq!(
        drop_target(CANVAS, &panes, along_left),
        DropTarget::Dock(DockTarget::Canvas(Edge::Left)),
        "the canvas edge wins over the pane beside it"
    );
    let between = CanvasPoint::new(first.x + first.w / 4.0, first.y + first.h / 2.0 + 30.0);
    assert_eq!(drop_target(CANVAS, &panes, between), DropTarget::Float);
}

/// C10: two charts side by side and a float; a drop on chart 1's bottom zone
/// splits chart 1 top over bottom and leaves chart 0 where it stood.
#[test]
fn a_drop_on_a_pane_side_splits_that_pane() {
    let (mut state, slot) = floating_over("charts-2");
    let (zero, one) = (rect_of(&state, 0), rect_of(&state, 1));
    let panes = open_panes(&state, CANVAS);
    let bottom = CanvasPoint::new(one.x + one.w / 2.0, one.bottom() - DOCK_EDGE_BAND_PX - 10.0);
    let target = DockTarget::Beside {
        slot: 1,
        side: Edge::Bottom,
    };
    assert_eq!(
        drop_target(CANVAS, &panes, bottom),
        DropTarget::Dock(target)
    );
    assert_eq!(state.dock(slot, target), Ok(true));
    assert_eq!(state.mode(slot), Some(PaneMode::Docked));
    assert_eq!(rect_of(&state, 0), zero, "chart 0 is unchanged");
    let (top, below) = (rect_of(&state, 1), rect_of(&state, slot));
    assert_eq!((top.x, top.y, top.w), (one.x, one.y, one.w));
    assert_eq!((below.x, below.w), (one.x, one.w), "under chart 1");
    assert_eq!(top.bottom(), below.y, "chart 1 split top over bottom");
    assert_eq!(top.h + below.h, one.h);
    assert_eq!(
        state.dock(slot, target),
        Err(CanvasRefusal::NotFloating(slot)),
        "a repeat is refused: it no longer floats"
    );
    let (mut other, floated) = floating_over("charts-2");
    let beside_gone = DockTarget::Beside {
        slot: 9,
        side: Edge::Left,
    };
    assert_eq!(
        other.dock(floated, beside_gone),
        Err(CanvasRefusal::UnknownPane(9))
    );
}

/// C11: a float dropped on a docked pane's centre takes that pane's place,
/// and the docked one floats where the float stood; slots and kinds stay
/// with their panes.
#[test]
fn a_centre_drop_swaps_the_two_panes() {
    let (mut state, slot) = floating_over("time+flow");
    let Some(FloatHost::Canvas { rect: floated }) = state.host(slot).cloned() else {
        panic!("a new pane floats over the canvas");
    };
    let flow = rect_of(&state, 1);
    let panes = open_panes(&state, CANVAS);
    assert_eq!(
        drop_target(CANVAS, &panes, centre(flow)),
        DropTarget::Dock(DockTarget::Swap(1))
    );
    assert_eq!(state.dock(slot, DockTarget::Swap(1)), Ok(true));
    assert_eq!(
        rect_of(&state, slot),
        flow,
        "the float took the flow's place"
    );
    assert_eq!(state.mode(1), Some(PaneMode::Floating));
    assert_eq!(
        state.host(1),
        Some(&FloatHost::Canvas { rect: floated }),
        "the flow floats where the float stood"
    );
    let kinds = state.panes();
    let kind = |held| kinds.iter().find(|(slot, ..)| *slot == held).unwrap().1;
    assert_eq!((kind(slot), kind(1)), (PaneKind::Time, PaneKind::Flow));

    // The tree swaps two docked panes the same way: places stay, panes move.
    let tree = state.tree().swap(0, slot);
    let rects = tree.rects(CANVAS);
    let at = |held| rects.iter().find(|(slot, _)| *slot == held).unwrap().1;
    assert_eq!(at(0), flow);
    assert_eq!(
        tree.swap(0, 42),
        tree,
        "a pane the tree lacks swaps nothing"
    );
}
