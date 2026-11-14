//! A press on a rail that never travels is a click and restores the rail, on
//! the tree and on the column painter alike; a drag from it still follows
//! the pointer.

use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
const DIVIDER: f32 = 4.0;
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
    Hand {
        pressed,
        held: true,
        delta,
        ..Hand::default()
    }
}

/// The frame the hand lets go, of the primary button or another.
fn let_go(primary: bool) -> Hand {
    Hand {
        released: true,
        primary,
        ..Hand::default()
    }
}

fn shares_of(state: &CanvasState) -> Vec<f32> {
    fn walk(node: &PaneTree, out: &mut Vec<f32>) {
        if let PaneTree::Split {
            children, shares, ..
        } = node
        {
            out.extend(shares.iter());
            children.iter().for_each(|child| walk(child, out));
        }
    }
    let mut out = Vec::new();
    walk(state.tree(), &mut out);
    out
}

fn close(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < EPS)
}

fn along(rect: CanvasRect, axis: SplitAxis) -> f32 {
    match axis {
        SplitAxis::Row => rect.w,
        SplitAxis::Column => rect.h,
    }
}

/// Press `seam`, carry it `by` points in sixteen frames, let go.
fn drag(state: &mut CanvasState, seam: &SeamAddress, by: f32) {
    let mut drag = None;
    state.seam_hand(&mut drag, (seam, held(0.0, true)), AREA);
    for _ in 0..16 {
        state.seam_hand(&mut drag, (seam, held(by / 16.0, false)), AREA);
    }
    state.seam_hand(&mut drag, (seam, let_go(true)), AREA);
    assert_eq!(drag, None, "letting go drops the drag");
}

/// Press `seam` and let go where it was pressed; answers whether the
/// release changed anything.
fn click(state: &mut CanvasState, seam: &SeamAddress, primary: bool) -> bool {
    let mut drag = None;
    let pressed = state.seam_hand(&mut drag, (seam, held(0.0, true)), AREA);
    assert!(!pressed, "a press alone moves nothing");
    state.seam_hand(&mut drag, (seam, let_go(primary)), AREA)
}

/// Every seam of every layout, dragged shut either way and then clicked on
/// its rail: the pair goes back to the shares it had before the fold and
/// no other pane moves.
#[test]
fn a_click_on_a_rail_restores_its_pre_fold_shares_on_every_preset() {
    for preset in LAYOUT_PRESETS {
        let laid = CanvasState::from_preset(preset);
        for seam in tree_seams(&laid, AREA, DIVIDER) {
            let whole = along(AREA, seam.axis);
            for by in [whole, -whole] {
                let name = format!("{} {:?} by {by}", preset.id, seam.address);
                let mut live = laid.clone();
                // Off the preset's own shares, so a restore to them would show.
                let _ = live.move_seam(&seam.address, 37.0, AREA);
                let before = shares_of(&live);
                drag(&mut live, &seam.address, by);
                assert!(!live.railed().is_empty(), "{name}: folded");
                let mut beside = tree_seams(&live, AREA, DIVIDER).into_iter();
                let rail = beside
                    .find(|seam| seam.rail.is_some())
                    .unwrap_or_else(|| panic!("{name}: a seam beside the rail"));
                assert!(click(&mut live, &rail.address, true), "{name}: restored");
                assert!(live.railed().is_empty(), "{name}: open");
                let after = shares_of(&live);
                assert!(
                    close(&before, &after),
                    "{name}: the pre-fold shares, {before:?} -> {after:?}"
                );
            }
        }
    }
}

/// A drag from the rail still opens it under the pointer; a press that
/// wanders past the click's reach and back, or a let go of another button,
/// is no click and leaves the rail standing.
#[test]
fn a_drag_from_a_rail_follows_the_pointer_and_a_wander_is_no_click() {
    let mut state = canvas("time+flow");
    drag(&mut state, &root(0), AREA.w);
    assert!(state.railed().contains(&1), "the flow folded");
    let mut held_drag = None;
    let seam = &root(0);
    state.seam_hand(&mut held_drag, (seam, held(0.0, true)), AREA);
    state.seam_hand(
        &mut held_drag,
        (seam, held(-2.0 * RAIL_CLICK_PX, false)),
        AREA,
    );
    state.seam_hand(
        &mut held_drag,
        (seam, held(2.0 * RAIL_CLICK_PX, false)),
        AREA,
    );
    assert!(!state.seam_hand(&mut held_drag, (seam, let_go(true)), AREA));
    assert!(state.railed().contains(&1), "a wander is no click");
    assert!(!click(&mut state, seam, false), "another button");
    assert!(state.railed().contains(&1), "still folded");
    let time = |state: &CanvasState| state.rects(AREA)[0].1.w;
    let pressed_at = time(&state);
    drag(&mut state, seam, -600.0);
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

/// A rail a save kept without the shares before its fold opens on a click
/// at the preset's own share of the pair, the pair's sum kept so the rest of
/// the split stays where it is.
#[test]
fn a_click_on_a_rail_with_no_remembered_shares_opens_it_at_the_preset_share() {
    let mut state = canvas("time+time+flow");
    state.move_seam(&root(1), -200.0, AREA).unwrap();
    drag(&mut state, &root(1), AREA.w);
    assert!(state.railed().contains(&2), "the flow folded");
    state.reopen.clear();
    let folded = shares_of(&state);
    assert!(click(&mut state, &root(1), true), "the click opens it");
    assert!(state.railed().is_empty());
    let (laid, now) = (shares_of(&canvas("time+time+flow")), shares_of(&state));
    assert!(
        (now[0] - folded[0]).abs() < EPS,
        "the first chart kept its share"
    );
    assert!(((now[1] + now[2]) - (folded[1] + folded[2])).abs() < EPS);
    let ratio = |shares: &[f32]| shares[2] / (shares[1] + shares[2]);
    assert!(
        (ratio(&now) - ratio(&laid)).abs() < EPS,
        "the preset's share: {now:?} against {laid:?}"
    );
}

const WIDTH: f32 = AREA.w;

/// The column painter's divider pressed `from` points into the canvas and
/// carried to `to` in sixteen frames, let go there.
fn column_drag_to(
    state: &mut CanvasState,
    split: &mut f32,
    (from, to): (f32, f32),
) -> Option<ColumnHold> {
    let mut hold = None;
    state.column_hand((&mut hold, split), held(0.0, true), (from, WIDTH));
    for _ in 0..16 {
        let step = held((to - from) / 16.0, false);
        state.column_hand((&mut hold, split), step, (from, WIDTH));
    }
    state.column_hand((&mut hold, split), let_go(true), (from, WIDTH));
    hold
}

/// The column painter's rail at `at` points pressed and let go there.
fn column_click(state: &mut CanvasState, split: &mut f32, at: f32, primary: bool) -> bool {
    let mut hold = None;
    state.column_hand((&mut hold, split), held(0.0, true), (at, WIDTH));
    let changed = state.column_hand((&mut hold, split), let_go(primary), (at, WIDTH));
    assert_eq!(hold, None, "letting go drops the hold");
    changed
}

/// On the column painter a fold keeps the split the press found, a click on
/// its rail opens it there, and a drag from the rail still follows the
/// pointer: each side of each column layout.
#[test]
fn a_click_on_a_column_rail_opens_it_at_the_split_before_the_fold() {
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
        assert_eq!(
            column_drag_to(&mut state, &mut split, (0.3 * WIDTH, edge)),
            None
        );
        let folds = if flow { (false, true) } else { (true, false) };
        assert_eq!(state.flow_side_folds(), folds, "{name}: folded");
        assert!(
            (split - 0.3).abs() < EPS,
            "{name}: the press's split, {split}"
        );
        let rail = if flow { WIDTH - 8.0 } else { 4.0 };
        assert!(!column_click(&mut state, &mut split, rail, false), "{name}");
        assert_eq!(state.flow_side_folds(), folds, "{name}: another button");
        assert!(column_click(&mut state, &mut split, rail, true), "{name}");
        assert_eq!(state.flow_side_folds(), (false, false), "{name}: open");
        assert!((split - 0.3).abs() < EPS, "{name}: at the split, {split}");
        let mut read = 0.0;
        state.mirror_split(preset, &mut read, false);
        assert!((read - 0.3).abs() < EPS, "{name}: the tree agrees, {read}");
        column_drag_to(&mut state, &mut split, (0.3 * WIDTH, edge));
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
