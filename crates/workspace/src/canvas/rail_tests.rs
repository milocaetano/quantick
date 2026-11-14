//! A press on a rail let go without a move does nothing, on the tree and on
//! the column painter alike; only a drag folds or opens a rail. A column
//! fold keeps the split its press found, so a restore by call opens the
//! side where it stood.

use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
const WIDTH: f32 = AREA.w;
const EPS: f32 = 1e-5;

fn canvas(id: &str) -> CanvasState {
    CanvasState::from_preset(preset(id).unwrap())
}

fn root(index: usize) -> SeamAddress {
    SeamAddress {
        path: Vec::new(),
        index,
    }
}

/// A frame the hand holds on, `delta` points along the split, `pressed`
/// on its first.
fn held(delta: f32, pressed: bool) -> Hand {
    Hand::of([pressed, true, false], delta)
}

/// The frame the hand lets go.
fn let_go() -> Hand {
    Hand::of([false, false, true], 0.0)
}

/// Press `seam`, carry it `by` points in sixteen frames, let go.
fn drag(state: &mut CanvasState, seam: &SeamAddress, by: f32) {
    let mut drag = None;
    state.seam_hand(&mut drag, (seam, held(0.0, true)), AREA);
    for _ in 0..16 {
        state.seam_hand(&mut drag, (seam, held(by / 16.0, false)), AREA);
    }
    state.seam_hand(&mut drag, (seam, let_go()), AREA);
    assert_eq!(drag, None, "letting go drops the drag");
}

/// The column painter's divider pressed `from` points into the canvas and
/// carried to `to` in sixteen frames, let go there.
fn column_drag_to(state: &mut CanvasState, split: &mut f32, (from, to): (f32, f32)) {
    let mut hold = None;
    state.column_hand((&mut hold, split), held(0.0, true), (from, WIDTH));
    for _ in 0..16 {
        let step = held((to - from) / 16.0, false);
        state.column_hand((&mut hold, split), step, (from, WIDTH));
    }
    state.column_hand((&mut hold, split), let_go(), (from, WIDTH));
    assert_eq!(hold, None, "letting go drops the hold");
}

/// A press and a let go on a tree rail, or on the seam beside it, changes
/// nothing: the rails, the shares, and the answer.
#[test]
fn a_click_on_a_tree_rail_changes_nothing() {
    let mut state = canvas("time+time+flow");
    drag(&mut state, &root(1), AREA.w);
    assert!(state.railed().contains(&2), "the flow folded");
    let before = state.clone();
    for seam in tree_seams(&state, AREA, CANVAS_DIVIDER_PX) {
        let mut drag = None;
        let pressed = state.seam_hand(&mut drag, (&seam.address, held(0.0, true)), AREA);
        let released = state.seam_hand(&mut drag, (&seam.address, let_go()), AREA);
        assert!(!pressed && !released, "{:?}: nothing changed", seam.address);
        assert_eq!(drag, None);
        assert_eq!(state, before, "{:?}: the same canvas", seam.address);
    }
}

/// A press and a let go on the column painter's rail changes nothing: the
/// fold, the split, and the answer.
#[test]
fn a_click_on_a_column_rail_changes_nothing() {
    for (id, flow) in [("time+flow", false), ("stack2+flow", true)] {
        let preset = preset(id).unwrap();
        let mut state = CanvasState::from_preset(preset);
        let mut split = 0.3;
        state.mirror_split(preset, &mut split, true);
        let edge = if flow { WIDTH } else { 0.0 };
        column_drag_to(&mut state, &mut split, (0.3 * WIDTH, edge));
        let (before, was) = (state.clone(), split);
        let rail = if flow { WIDTH - 8.0 } else { 4.0 };
        let mut hold = None;
        let pressed = state.column_hand((&mut hold, &mut split), held(0.0, true), (rail, WIDTH));
        let released = state.column_hand((&mut hold, &mut split), let_go(), (rail, WIDTH));
        assert!(!pressed && !released, "{id}: nothing changed");
        assert_eq!((state, split), (before, was), "{id}: the same canvas");
    }
}

/// A drag from a tree rail still opens it under the pointer.
#[test]
fn a_drag_from_a_rail_follows_the_pointer() {
    let mut state = canvas("time+flow");
    drag(&mut state, &root(0), AREA.w);
    assert!(state.railed().contains(&1), "the flow folded");
    let time = |state: &CanvasState| state.rects(AREA)[0].1.w;
    let pressed_at = time(&state);
    drag(&mut state, &root(0), -600.0);
    assert!(
        state.railed().is_empty(),
        "dragged past the threshold, open"
    );
    assert!(
        (time(&state) - (pressed_at - 600.0)).abs() < 1.0,
        "it opened under the pointer: {} from {pressed_at}",
        time(&state)
    );
}

/// On the column painter a drag fold keeps the split the press found, not
/// the floor the drag passed on its way, so `canvas.pane.restore` (the
/// split mirrored into the tree, the rail restored, the tree's split read
/// back, as the call does) opens the side where it stood; and a drag from
/// the rail still follows the pointer. Each side of each column layout.
#[test]
fn a_column_drag_fold_reopens_by_call_at_the_split_before_the_fold() {
    for (id, flow) in [
        ("time+flow", false),
        ("time+flow", true),
        ("stack2+flow", false),
        ("stack2+flow", true),
    ] {
        let name = format!("{id} flow {flow}");
        let preset = preset(id).unwrap();
        let mut state = CanvasState::from_preset(preset);
        let mut split = 0.3;
        state.mirror_split(preset, &mut split, true);
        let edge = if flow { WIDTH } else { 0.0 };
        column_drag_to(&mut state, &mut split, (0.3 * WIDTH, edge));
        let folds = if flow { (false, true) } else { (true, false) };
        assert_eq!(state.flow_side_folds(), folds, "{name}: folded");
        assert!(
            (split - 0.3).abs() < EPS,
            "{name}: the press's split, {split}"
        );
        let slot = *state.railed().iter().next().expect("a rail");
        state.mirror_split(preset, &mut split, true);
        assert_eq!(state.restore(slot), Ok(true), "{name}: the call opens it");
        state.mirror_split(preset, &mut split, false);
        assert_eq!(state.flow_side_folds(), (false, false), "{name}: open");
        assert!((split - 0.3).abs() < EPS, "{name}: at the split, {split}");
        column_drag_to(&mut state, &mut split, (0.3 * WIDTH, edge));
        let rail = if flow { WIDTH - 8.0 } else { 4.0 };
        column_drag_to(&mut state, &mut split, (rail, 0.5 * WIDTH));
        assert_eq!(
            state.flow_side_folds(),
            (false, false),
            "{name}: dragged open"
        );
        assert!(
            (split - 0.5).abs() < 1e-3,
            "{name}: under the pointer, {split}"
        );
    }
}
