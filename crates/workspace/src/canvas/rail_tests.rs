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

/// The split holding the flow pane: its path, the flow pane's index there,
/// the split's axis and its child count.
fn flow_split(state: &CanvasState) -> Option<(Vec<usize>, usize, SplitAxis, usize)> {
    fn walk(
        node: &PaneTree,
        path: &mut Vec<usize>,
    ) -> Option<(Vec<usize>, usize, SplitAxis, usize)> {
        let PaneTree::Split { axis, children, .. } = node else {
            return None;
        };
        let flow = |child: &PaneTree| matches!(child, PaneTree::Pane { kind, .. } if *kind == PaneKind::Flow);
        if let Some(index) = children.iter().position(flow) {
            return Some((path.clone(), index, *axis, children.len()));
        }
        children.iter().enumerate().find_map(|(at, child)| {
            path.push(at);
            let found = walk(child, path);
            path.pop();
            found
        })
    }
    walk(state.tree(), &mut Vec::new())
}

/// Every split's shares, depth first.
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

/// One `canvas.seam.drag` of the seam after child `index` of the split at
/// `path`, `by` points in one press.
fn seam_drag(state: &mut CanvasState, path: &[usize], index: usize, by: f32) {
    let seam = SeamAddress {
        path: path.to_vec(),
        index,
    };
    state
        .step_seam_drag(&mut None, (&seam, by, true), AREA)
        .unwrap_or_else(|refused| panic!("{seam:?}: {refused}"));
}

/// Every layout with a flow side, its shares first moved off the preset's:
/// the flow pane dragged shut, the chart beside it dragged shut, and every
/// chart beside it dragged shut one rail at a time, each reopened by the
/// expand call of its side (`layout.flow.expand`, `layout.pane.expand`),
/// comes back at the shares it had before the drag, no chart under the
/// least a pane keeps.
#[test]
fn an_expand_after_a_drag_fold_reopens_at_the_pre_fold_shares() {
    for preset in LAYOUT_PRESETS {
        let mut laid = CanvasState::from_preset(preset);
        let Some((path, flow, axis, len)) = flow_split(&laid) else {
            continue;
        };
        let whole = match axis {
            SplitAxis::Row => AREA.w,
            SplitAxis::Column => AREA.h,
        };
        let neighbour = if flow > 0 { flow - 1 } else { flow + 1 };
        let _ = laid.move_seam(&root_at(&path, flow.min(neighbour)), 37.0, AREA);
        let before = shares_of(&laid);
        // A chart folds when its seam with the next child toward the flow
        // pane is dragged away from the flow; the flow pane, the other way.
        let shut = |at: usize| if at < flow { -whole } else { whole };
        let mut cases: Vec<(&str, bool, Vec<(usize, f32)>)> = vec![
            ("flow", true, vec![(flow.min(neighbour), -shut(neighbour))]),
            (
                "one chart",
                false,
                vec![(flow.min(neighbour), shut(neighbour))],
            ),
        ];
        let mut charts: Vec<usize> = (0..len).filter(|at| *at != flow).collect();
        charts.sort_by_key(|at| std::cmp::Reverse(at.abs_diff(flow)));
        let every = charts.iter().map(|at| {
            let seam = if *at < flow { *at } else { at - 1 };
            (seam, shut(*at))
        });
        cases.push(("every chart", false, every.collect()));
        for (case, flow_side, drags) in cases {
            let name = format!("{} {case}", preset.id);
            let mut state = laid.clone();
            for (seam, by) in drags {
                seam_drag(&mut state, &path, seam, by);
            }
            assert!(!state.railed().is_empty(), "{name}: folded");
            assert!(state.fold_flow_side(flow_side, false), "{name}: expanded");
            assert!(state.railed().is_empty(), "{name}: open");
            let after = shares_of(&state);
            let same = before.iter().zip(&after).all(|(a, b)| (a - b).abs() < EPS);
            assert!(same, "{name}: the pre-fold shares, {before:?} -> {after:?}");
            for (slot, rect) in state.rects(AREA) {
                let least = MIN_TREE_PANE_PX.min(AREA.h / 2.0) - 1.0;
                assert!(
                    rect.w >= least && rect.h >= least,
                    "{name}: {slot} {rect:?}"
                );
            }
        }
    }
}

fn root_at(path: &[usize], index: usize) -> SeamAddress {
    SeamAddress {
        path: path.to_vec(),
        index,
    }
}
