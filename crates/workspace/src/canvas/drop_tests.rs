//! The one drop rule: edges dock, corners park, anywhere else floats.

use super::*;

const CANVAS: CanvasRect = CanvasRect::new(100.0, 50.0, 1600.0, 900.0);

fn at(x: f32, y: f32) -> DropTarget {
    drop_target(CANVAS, CanvasPoint::new(x, y))
}

#[test]
fn edges_dock_corners_park_elsewhere_floats() {
    let (left, top) = (CANVAS.x, CANVAS.y);
    let (right, bottom) = (CANVAS.right(), CANVAS.bottom());
    let (mid_x, mid_y) = (left + CANVAS.w / 2.0, top + CANVAS.h / 2.0);

    // Inside each edge band, away from the corners.
    assert_eq!(at(left + 5.0, mid_y), DropTarget::Dock(Edge::Left));
    assert_eq!(at(right - 5.0, mid_y), DropTarget::Dock(Edge::Right));
    assert_eq!(at(mid_x, top + 5.0), DropTarget::Dock(Edge::Top));
    assert_eq!(at(mid_x, bottom - 5.0), DropTarget::Dock(Edge::Bottom));
    assert_eq!(
        at(left + DOCK_EDGE_BAND_PX, mid_y),
        DropTarget::Dock(Edge::Left),
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
