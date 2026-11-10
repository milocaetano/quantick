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
