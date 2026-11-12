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
        park_rect(CANVAS, Corner::TopLeft, size, &[]),
        CanvasRect::new(left + 12.0, top + 12.0, 400.0, 300.0)
    );
    assert_eq!(
        park_rect(CANVAS, Corner::BottomRight, size, &[]),
        CanvasRect::new(right - 12.0 - 400.0, bottom - 12.0 - 300.0, 400.0, 300.0)
    );
    assert_eq!(
        park_rect(CANVAS, Corner::TopRight, size, &[]),
        CanvasRect::new(right - 12.0 - 400.0, top + 12.0, 400.0, 300.0)
    );
    assert_eq!(
        park_rect(CANVAS, Corner::BottomLeft, size, &[]),
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
    let panes = open_panes(&state, CANVAS, None);
    let zones = drop_zones(CANVAS, &panes);
    assert_eq!(
        zones.len(),
        4 + 4 + 4 * 5,
        "four corners, four edges, four sides and a centre per pane"
    );
    for (target, zone) in &zones {
        assert_eq!(
            drop_target(CANVAS, &panes, centre(*zone)),
            *target,
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
    let panes = open_panes(&state, CANVAS, None);
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
        Ok(false),
        "a repeat changes nothing"
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
    let panes = open_panes(&state, CANVAS, None);
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

/// A docked pane held by its ⋯ stays docked until the drop: on another
/// pane's centre the two trade places, on a side it moves there, its own
/// zones are never offered, and a corner floats it parked there.
#[test]
fn a_held_docked_pane_trades_places_or_moves() {
    let window = CanvasRect::new(0.0, 0.0, CANVAS.right(), CANVAS.bottom());
    let state = CanvasState::from_preset(preset("time+time+flow").unwrap());
    let (zero, flow) = (rect_of(&state, 0), rect_of(&state, 2));
    let start = centre(zero);
    let held = FloatGesture::Move {
        slot: 0,
        from: CanvasRect::new(start.x - 100.0, start.y - 50.0, 200.0, 100.0),
        start,
    };
    assert_eq!(
        held.release(&state, CANVAS, window, centre(zero)),
        DropOutcome::Dock(DockTarget::Swap(0)),
        "let go over its own place, it stays"
    );
    let mut kept = state.clone();
    assert_eq!(kept.dock(0, DockTarget::Swap(0)), Ok(false));
    // Past the top band, in the corner chart 1's side zones leave.
    let near_top = CanvasPoint::new(rect_of(&state, 1).x + 10.0, CANVAS.y + 55.0);
    let DropOutcome::Float(floated) = held.release(&state, CANVAS, window, near_top) else {
        panic!("away from every zone it floats");
    };
    assert!(floated.y >= CANVAS.y, "kept inside the canvas: {floated:?}");
    let swap = held.release(&state, CANVAS, window, centre(flow));
    assert_eq!(swap, DropOutcome::Dock(DockTarget::Swap(2)));
    let lit = held.dock_preview(&state, CANVAS, window, centre(flow));
    assert_eq!(lit, Some(flow), "the preview lights the flow's place");
    let mut traded = state.clone();
    assert_eq!(traded.dock(0, DockTarget::Swap(2)), Ok(true));
    assert_eq!((rect_of(&traded, 0), rect_of(&traded, 2)), (flow, zero));
    assert_eq!(traded.mode(2), Some(PaneMode::Docked), "both stay docked");
    assert_eq!(traded.dock(0, DockTarget::Swap(0)), Ok(false));

    let corner = CanvasPoint::new(CANVAS.right() - 5.0, CANVAS.y + 5.0);
    let DropOutcome::Float(parked) = held.release(&state, CANVAS, window, corner) else {
        panic!("a docked pane floats parked in the corner");
    };
    assert_eq!(
        parked,
        park_rect(CANVAS, Corner::TopRight, (200.0, 100.0), &[])
    );

    let mut moved = state.clone();
    let beside = DockTarget::Beside {
        slot: 2,
        side: Edge::Bottom,
    };
    assert_eq!(moved.dock(0, beside), Ok(true));
    assert_eq!(rect_of(&moved, 0).x, rect_of(&moved, 2).x, "under the flow");
    let mut lone = CanvasState::from_preset(preset("time").unwrap());
    assert_eq!(
        lone.dock(0, DockTarget::Canvas(Edge::Left)),
        Err(CanvasRefusal::LastDockedPane)
    );
}

/// A pane in its own window cannot swap: the docked pane would leave for
/// that window.
#[test]
fn a_pane_in_its_own_window_does_not_swap() {
    let (mut state, slot) = floating_over("charts-2");
    let window = CanvasRect::new(2000.0, 0.0, 800.0, 600.0);
    state.pop_out(slot, "DISPLAY2", window).unwrap();
    let before = state.clone();
    assert_eq!(
        state.dock(slot, DockTarget::Swap(0)),
        Err(CanvasRefusal::NotFloating(slot))
    );
    assert_eq!(state, before);
}

/// C9: a strip folded against the left edge keeps its restore in reach: a
/// float parked top-left stands clear of it, as does one parked against a
/// strip folded along the top.
#[test]
fn a_parked_float_never_covers_a_strip() {
    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    assert_eq!(state.minimize(0), Ok(true));
    let strip = state.rects(CANVAS)[0].1;
    assert_eq!(strip.x, CANVAS.x, "the strip stands on the left edge");
    let slot = state.add(PaneKind::Time, CANVAS).unwrap();
    assert_eq!(state.park(slot, Corner::TopLeft, CANVAS), Ok(true));
    let FloatHost::Canvas { rect } = state.host(slot).unwrap().clone() else {
        panic!("a parked pane floats over the canvas");
    };
    let clear = |a: CanvasRect, b: CanvasRect| {
        a.right() <= b.x || b.right() <= a.x || a.bottom() <= b.y || b.bottom() <= a.y
    };
    assert!(clear(rect, strip), "{rect:?} covers the strip {strip:?}");
    assert_eq!(rect.x, strip.right() + PARK_MARGIN_PX, "beside the strip");
    assert_eq!(rect.y, CANVAS.y + PARK_MARGIN_PX, "still in its corner");

    let size = (400.0, 300.0);
    let top = CanvasRect::new(CANVAS.x, CANVAS.y, CANVAS.w, 24.0);
    let parked = park_rect(CANVAS, Corner::TopRight, size, &[top, strip]);
    assert!(clear(parked, top) && clear(parked, strip));
    assert_eq!(
        parked.y,
        top.bottom() + PARK_MARGIN_PX,
        "below a flat strip"
    );
    let free = park_rect(CANVAS, Corner::BottomRight, size, &[top, strip]);
    assert_eq!(free, park_rect(CANVAS, Corner::BottomRight, size, &[]));

    // A strip mid-canvas never pushes a wide pane off the canvas.
    let middle = CanvasRect::new(CANVAS.x + 800.0, CANVAS.y, 24.0, CANVAS.h);
    let wide = park_rect(CANVAS, Corner::TopLeft, (1000.0, 300.0), &[middle]);
    assert!(wide.x >= CANVAS.x && wide.right() <= CANVAS.right() - PARK_MARGIN_PX);
}
