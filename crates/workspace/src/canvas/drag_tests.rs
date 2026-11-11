//! A held floating pane: moving, resizing, the dock preview and the drop.

use super::*;

const CANVAS: CanvasRect = CanvasRect::new(0.0, 40.0, 1600.0, 900.0);
const WINDOW: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 960.0);

fn floating() -> (CanvasState, PaneSlot, CanvasRect) {
    let mut state = CanvasState::from_preset(preset("time+flow").unwrap());
    let slot = state.add(PaneKind::Time, CANVAS).unwrap();
    let Some(FloatHost::Canvas { rect }) = state.host(slot).cloned() else {
        panic!("a new pane floats over the canvas");
    };
    (state, slot, rect)
}

fn held(slot: PaneSlot, rect: CanvasRect) -> FloatGesture {
    FloatGesture::Move {
        slot,
        from: rect,
        start: CanvasPoint::new(rect.x + 20.0, rect.y + 10.0),
    }
}

#[test]
fn a_held_pane_follows_the_pointer_and_lands_where_the_drop_rule_says() {
    let (state, slot, rect) = floating();
    let grip = held(slot, rect);
    let moved = grip.rect(CanvasPoint::new(rect.x + 120.0, rect.y + 60.0));
    assert_eq!((moved.x, moved.y), (rect.x + 100.0, rect.y + 50.0));
    assert_eq!(
        (moved.w, moved.h),
        (rect.w, rect.h),
        "a move keeps the size"
    );

    let middle = CanvasPoint::new(800.0, 500.0);
    assert_eq!(
        grip.release(CANVAS, WINDOW, middle),
        DropOutcome::Float(grip.rect(middle))
    );
    let left = CanvasPoint::new(10.0, 500.0);
    assert_eq!(
        grip.release(CANVAS, WINDOW, left),
        DropOutcome::Dock(Edge::Left)
    );
    let corner = CanvasPoint::new(1590.0, 935.0);
    assert_eq!(
        grip.release(CANVAS, WINDOW, corner),
        DropOutcome::Park(Corner::BottomRight)
    );
    let past = CanvasPoint::new(1700.0, 500.0);
    assert_eq!(
        grip.release(CANVAS, WINDOW, past),
        DropOutcome::PopOut(grip.rect(past)),
        "past the window's edge the pane leaves for its own window"
    );

    // The preview lights the slot the dock would make, and only for a dock.
    let lit = grip.dock_preview(&state, CANVAS, WINDOW, left).unwrap();
    assert_eq!((lit.x, lit.y, lit.h), (CANVAS.x, CANVAS.y, CANVAS.h));
    assert!(lit.w > 0.0 && lit.w < CANVAS.w);
    assert_eq!(grip.dock_preview(&state, CANVAS, WINDOW, middle), None);
    assert_eq!(grip.dock_preview(&state, CANVAS, WINDOW, corner), None);
}

#[test]
fn an_edge_resizes_and_never_below_the_floor() {
    let rect = CanvasRect::new(100.0, 100.0, 400.0, 300.0);
    assert_eq!(ResizeEdges::at(rect, CanvasPoint::new(300.0, 250.0)), None);
    assert_eq!(ResizeEdges::at(rect, CanvasPoint::new(0.0, 0.0)), None);
    let corner = ResizeEdges::at(rect, CanvasPoint::new(501.0, 399.0)).unwrap();
    assert!(corner.right && corner.bottom && !corner.left && !corner.top);
    assert_eq!(
        corner.resized(rect, (50.0, 20.0)),
        CanvasRect::new(100.0, 100.0, 450.0, 320.0)
    );
    let left = ResizeEdges::at(rect, CanvasPoint::new(98.0, 250.0)).unwrap();
    let narrowed = left.resized(rect, (1000.0, 0.0));
    assert_eq!(narrowed.w, MIN_FLOAT_SIZE.0, "the floor stops the edge");
    assert_eq!(narrowed.right(), rect.right(), "the other edge stays put");

    let gesture = FloatGesture::Resize {
        slot: 3,
        edges: corner,
        from: rect,
        start: CanvasPoint::new(500.0, 400.0),
    };
    let pointer = CanvasPoint::new(2000.0, 400.0);
    assert_eq!(
        gesture.release(CANVAS, WINDOW, pointer),
        DropOutcome::Float(gesture.rect(pointer)),
        "a resize never docks or pops out"
    );
    assert_eq!(gesture.slot(), 3);
}
