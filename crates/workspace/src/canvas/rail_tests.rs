//! The rail's hand rules. A press on a rail let go without a move does
//! nothing, on the tree and on the column painter alike; only a drag folds
//! or opens a rail. A column fold keeps the split its press found, so a
//! restore by call opens the side where it stood. Two drag-folds in a row:
//! the second leaves the first rail folded, save where it drags the seam
//! beside that rail, which reopens it by the rule
//! `a_seam_beside_a_strip_never_moves_but_one_beside_a_rail_reopens_it`
//! states.

use super::seams::{node_at, partners, railed};
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

/// One expand case: its name, whether the flow side folds, and the seam
/// drags (seam index, points) that fold it.
type Case = (&'static str, bool, Vec<(usize, f32)>);

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
        let mut cases: Vec<Case> = vec![
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

/// What dragging `seam` the whole canvas towards the end of its split
/// (`forward`) or its start folds: the slots of that partner, and whether
/// the pair holds a rail already, which the drag then reopens or refolds.
fn fold_side(state: &CanvasState, seam: &SeamAddress, forward: bool) -> (Vec<PaneSlot>, bool) {
    let Some(PaneTree::Split { children, .. }) = node_at(state.tree(), &seam.path) else {
        panic!("{seam:?} names a split");
    };
    let (first, second) = partners(state, children, seam.index).expect("a seam that moves");
    let folded = &children[if forward { second } else { first }];
    let beside = railed(state, &children[first]) || railed(state, &children[second]);
    let slots = folded.panes().into_iter().map(|(slot, _)| slot).collect();
    (slots, beside)
}

/// One hand drag of `seam` the whole canvas, `forward` or back, pressed
/// and released in sixteen frames.
fn drag_shut(state: &mut CanvasState, seam: &SeamAddress, forward: bool) {
    let axis = tree_seams(state, AREA, CANVAS_DIVIDER_PX)
        .into_iter()
        .find(|listed| listed.address == *seam)
        .unwrap_or_else(|| panic!("{seam:?} is listed"))
        .axis;
    let whole = match axis {
        SplitAxis::Row => AREA.w,
        SplitAxis::Column => AREA.h,
    };
    let by = if forward { whole } else { -whole };
    let mut held = None;
    for _ in 0..16 {
        state
            .step_seam_drag(&mut held, (seam, by / 16.0, false), AREA)
            .unwrap();
    }
}

fn all_railed(state: &CanvasState, slots: &[PaneSlot]) -> bool {
    slots.iter().all(|slot| state.railed().contains(slot))
}

/// Fold `first` then `second` on `id`, each dragged `forward` or back, and
/// check both stay folded.
fn second_fold_keeps_the_first(
    id: &str,
    (first, first_forward): (SeamAddress, bool),
    (second, second_forward): (SeamAddress, bool),
) {
    let mut state = canvas(id);
    let (one, _) = fold_side(&state, &first, first_forward);
    drag_shut(&mut state, &first, first_forward);
    assert!(all_railed(&state, &one), "{id}: {first:?} folds {one:?}");
    let (two, beside) = fold_side(&state, &second, second_forward);
    assert!(!beside, "{id}: {second:?} is no seam beside the first rail");
    drag_shut(&mut state, &second, second_forward);
    assert!(all_railed(&state, &two), "{id}: {second:?} folds {two:?}");
    assert!(
        all_railed(&state, &one),
        "{id}: {first:?} stays folded after {second:?}: {:?}",
        state.railed()
    );
}

#[test]
fn stack2_flow_folding_the_flow_keeps_the_stacked_chart_folded() {
    second_fold_keeps_the_first(
        "stack2+flow",
        (root_at(&[0], 0), true),
        (root_at(&[], 0), true),
    );
}

#[test]
fn grid4_flow_folding_the_flow_keeps_the_grid_chart_folded() {
    second_fold_keeps_the_first(
        "grid4+flow",
        (root_at(&[0, 0], 0), true),
        (root_at(&[], 0), true),
    );
}

#[test]
fn grid4_flow_folding_the_bottom_row_keeps_the_top_row_chart_folded() {
    second_fold_keeps_the_first(
        "grid4+flow",
        (root_at(&[0, 0], 0), true),
        (root_at(&[0], 0), true),
    );
}

#[test]
fn flow_over_2time_folding_the_flow_keeps_the_chart_folded() {
    second_fold_keeps_the_first(
        "flow-over-2time",
        (root_at(&[1], 0), true),
        (root_at(&[], 0), false),
    );
}

/// "6 charts", `[0]/0` then `[0]/1`: the second seam borders the first
/// rail, so its drag reopens that rail and folds the chart past it, as a
/// seam beside a rail always does.
#[test]
fn charts6_a_seam_beside_the_first_rail_reopens_it_by_the_rail_rule() {
    let mut state = canvas("charts-6");
    drag_shut(&mut state, &root_at(&[0], 0), true);
    assert_eq!(state.railed().iter().copied().collect::<Vec<_>>(), [1]);
    let (_, beside) = fold_side(&state, &root_at(&[0], 1), true);
    assert!(beside, "seam [0]/1 borders the rail");
    drag_shut(&mut state, &root_at(&[0], 1), true);
    assert_eq!(
        state.railed().iter().copied().collect::<Vec<_>>(),
        [2],
        "the rail beside the seam reopened and the far chart folded"
    );
}

/// Every ordered pair of distinct seams of every layout, each dragged shut
/// either way, one after the other: both stay folded. A second seam the
/// first fold put inside its rail is not listed and is skipped; one beside
/// the first rail reopens it unless it folds that same rail again.
#[test]
fn every_second_fold_of_every_layout_keeps_the_first() {
    let mut beside_pairs = 0;
    for preset in LAYOUT_PRESETS {
        let fresh = CanvasState::from_preset(preset);
        let seams = tree_seams(&fresh, AREA, CANVAS_DIVIDER_PX);
        for first in &seams {
            for second in seams.iter().filter(|seam| seam.address != first.address) {
                for (first_forward, second_forward) in
                    [(true, true), (true, false), (false, true), (false, false)]
                {
                    let (a, b) = (&first.address, &second.address);
                    let name = format!(
                        "{} {a:?} {first_forward} then {b:?} {second_forward}",
                        preset.id
                    );
                    let mut state = fresh.clone();
                    let (one, _) = fold_side(&state, a, first_forward);
                    drag_shut(&mut state, a, first_forward);
                    assert!(all_railed(&state, &one), "{name}: the first folds");
                    let listed = tree_seams(&state, AREA, CANVAS_DIVIDER_PX)
                        .into_iter()
                        .any(|seam| seam.address == *b && seam.resizable);
                    if !listed {
                        continue;
                    }
                    let (two, beside) = fold_side(&state, b, second_forward);
                    drag_shut(&mut state, b, second_forward);
                    assert!(all_railed(&state, &two), "{name}: the second folds");
                    let refolded = one.iter().all(|slot| two.contains(slot));
                    if beside && !refolded {
                        beside_pairs += 1;
                        assert!(
                            one.iter().all(|slot| !state.railed().contains(slot)),
                            "{name}: the seam beside the first rail reopens it"
                        );
                    } else {
                        assert!(
                            all_railed(&state, &one),
                            "{name}: the first stays folded: {:?}",
                            state.railed()
                        );
                    }
                }
            }
        }
    }
    assert!(beside_pairs > 0, "the rail rule is exercised");
}

/// The outer side folded over a rail and dragged back open in the same
/// gesture: the rail inside it stays folded, and its restore still opens it
/// at the shares it had.
#[test]
fn a_rail_inside_a_side_folded_and_reopened_in_one_drag_stays_folded() {
    let mut state = canvas("grid4+flow");
    let before = state.clone();
    drag_shut(&mut state, &root_at(&[0, 0], 0), true);
    let mut held = None;
    for by in [-AREA.w, AREA.w] {
        for _ in 0..16 {
            let step = (&root_at(&[], 0), by / 16.0, false);
            state.step_seam_drag(&mut held, step, AREA).unwrap();
        }
        if by < 0.0 {
            assert_eq!(state.railed().len(), 4, "the grid folded whole");
        }
    }
    assert_eq!(
        state.railed().iter().copied().collect::<Vec<_>>(),
        [1],
        "the grid opened, its folded chart did not"
    );
    assert_eq!(state.restore(1), Ok(true));
    let Some(PaneTree::Split { shares, .. }) = node_at(state.tree(), &[0, 0]) else {
        panic!("the grid's top row");
    };
    let Some(PaneTree::Split { shares: was, .. }) = node_at(before.tree(), &[0, 0]) else {
        panic!("the grid's top row");
    };
    assert!(
        (shares.iter().zip(was.iter())).all(|(a, b)| (a - b).abs() < 1e-5),
        "{shares:?} vs {was:?}"
    );
}

/// The context column's expand on a layout with no flow pane: every chart
/// is context, so it opens every rail at its pre-fold shares, as a restore
/// does; a second finds nothing, and a collapse has no column to fold.
#[test]
fn a_context_expand_without_a_flow_pane_opens_every_rail() {
    for id in [
        "charts-2", "charts-3", "charts-4", "charts-5", "charts-6", "charts-7", "charts-8",
    ] {
        let fresh = canvas(id);
        let mut state = fresh.clone();
        assert!(!state.fold_context(true), "{id}: no column to collapse");
        drag_shut(&mut state, &root_at(&[], 0), false);
        assert!(!state.railed().is_empty(), "{id}");
        assert!(state.fold_context(false), "{id}: the rail opens");
        assert!(state.railed().is_empty(), "{id}");
        assert_eq!(state, fresh, "{id}: at the shares it had");
        assert!(!state.fold_context(false), "{id}: nothing left folded");
    }
    let mut flow = canvas("time+time+flow");
    assert!(
        flow.fold_context(true),
        "beside a flow pane it folds the charts"
    );
    assert_eq!(flow.flow_side_folds(), (true, false));
    assert!(flow.fold_context(false));
    assert_eq!(flow, canvas("time+time+flow"));
}
