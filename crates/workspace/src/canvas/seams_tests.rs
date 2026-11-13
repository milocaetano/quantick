//! The seams between a tree's docked panes and the one resize a drag and a
//! remote caller share.

use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
const DIVIDER: f32 = 4.0;
const EPS: f32 = 1e-3;

fn canvas(id: &str) -> CanvasState {
    CanvasState::from_preset(preset(id).unwrap())
}

fn rect_of(state: &CanvasState, slot: PaneSlot) -> CanvasRect {
    let rects = state.rects(AREA);
    rects.iter().find(|(held, _)| *held == slot).unwrap().1
}

fn root_shares(state: &CanvasState) -> Vec<f32> {
    match state.tree() {
        PaneTree::Split { shares, .. } => shares.to_vec(),
        PaneTree::Pane { .. } => Vec::new(),
    }
}

fn root(index: usize) -> SeamAddress {
    SeamAddress {
        path: Vec::new(),
        index,
    }
}

#[test]
fn two_side_by_side_charts_have_one_vertical_seam_between_their_bands() {
    let state = canvas("charts-2");
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert_eq!(seams.len(), 1);
    let seam = &seams[0];
    assert_eq!(seam.address, root(0));
    assert_eq!(seam.axis, SplitAxis::Row);
    assert!(seam.resizable);
    assert_eq!(seam.rect, CanvasRect::new(798.0, 0.0, 4.0, 900.0));
    // The seam is exactly the gap the bands leave.
    let bands = tree_bands(&state, &mut Seats::default(), AREA, DIVIDER);
    assert_eq!(bands[0].rect.right(), seam.rect.x);
    assert_eq!(bands[1].rect.x, seam.rect.right());
}

#[test]
fn a_grid_lists_its_outer_seam_first_then_each_row_s_inset_by_the_cross_seam() {
    let seams = tree_seams(&canvas("charts-4"), AREA, DIVIDER);
    let read: Vec<_> = seams
        .iter()
        .map(|seam| (seam.address.path.clone(), seam.address.index, seam.axis))
        .collect();
    assert_eq!(
        read,
        vec![
            (vec![], 0, SplitAxis::Column),
            (vec![0], 0, SplitAxis::Row),
            (vec![1], 0, SplitAxis::Row),
        ]
    );
    assert_eq!(seams[0].rect, CanvasRect::new(0.0, 448.0, 1600.0, 4.0));
    assert_eq!(seams[1].rect, CanvasRect::new(798.0, 0.0, 4.0, 448.0));
    assert_eq!(seams[2].rect, CanvasRect::new(798.0, 452.0, 4.0, 448.0));
}

#[test]
fn a_maximized_chart_leaves_no_seam_to_drag() {
    let mut state = canvas("charts-2");
    state.maximize(0).unwrap();
    assert!(tree_seams(&state, AREA, DIVIDER).is_empty());
    assert_eq!(
        state.move_seam(&root(0), 50.0, AREA),
        Err(CanvasRefusal::NoSeam)
    );
}

/// The reopen rule: a seam beside a minimized strip at the split's end never
/// moves (the strip opens from its menu), while a seam beside a rail a drag
/// folded reopens it when dragged back past the threshold, and only a drag
/// does: a nudge between two open charts never reaches a rail.
#[test]
fn a_seam_beside_a_strip_never_moves_but_one_beside_a_rail_reopens_it() {
    let mut state = canvas("charts-3");
    state.minimize(0).unwrap();
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert_eq!(seams.len(), 2);
    assert!(!seams[0].resizable, "beside the folded strip");
    assert!(seams[1].resizable, "between the two open charts");
    assert_eq!(seams[0].rail, None, "a strip is no rail");
    assert_eq!(
        state.move_seam(&root(0), 50.0, AREA),
        Err(CanvasRefusal::NoSeam)
    );
    assert_eq!(state.move_seam(&root(1), 50.0, AREA), Ok(true));

    let mut state = canvas("charts-3");
    drag(&mut state, &root(0), -AREA.w);
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert!(seams[0].resizable, "beside the rail, it reopens it");
    let rail = seams[0].rail.expect("the rail beside the seam");
    assert_eq!((rail.slot, rail.meets_after), (0, true));
    assert_eq!(
        state.move_seam(&root(0), 200.0, AREA),
        Err(CanvasRefusal::NoSeam),
        "a nudge never reopens a rail"
    );
    drag(&mut state, &root(0), COLLAPSE_AT_PX / 2.0);
    assert!(state.railed().contains(&0), "short of the threshold: still a rail");
    drag(&mut state, &root(0), 300.0);
    assert!(state.railed().is_empty() && state.minimized().is_empty());
    let reopened = rect_of(&state, 0).w;
    assert!(
        (reopened - (rail.rect.w + DIVIDER / 2.0 + 300.0)).abs() < 1.0,
        "it opens under the pointer, as the column painter's rail does: {reopened}"
    );
}

/// One hand drag of `seam` by `by` points, pressed and released, in sixteen
/// frames: travel adds up from the press, never lost against a floor.
fn drag(state: &mut CanvasState, seam: &SeamAddress, by: f32) {
    let mut held = None;
    for _ in 0..16 {
        state
            .step_seam_drag(&mut held, (seam, by / 16.0), AREA)
            .unwrap();
    }
}

/// Every split's shares, depth first.
fn all_shares(node: &PaneTree, out: &mut Vec<f32>) {
    if let PaneTree::Split {
        children, shares, ..
    } = node
    {
        out.extend(shares.iter());
        children.iter().for_each(|child| all_shares(child, out));
    }
}

fn shares_of(state: &CanvasState) -> Vec<f32> {
    let mut out = Vec::new();
    all_shares(state.tree(), &mut out);
    out
}

fn along(rect: CanvasRect, axis: SplitAxis) -> f32 {
    match axis {
        SplitAxis::Row => rect.w,
        SplitAxis::Column => rect.h,
    }
}

/// Each child's extent along the split at `seam`'s path.
fn extents(state: &CanvasState, seam: &SeamAddress) -> Vec<f32> {
    let Some(PaneTree::Split { axis, children, .. }) = super::seams::node_at(state.tree(), &seam.path)
    else {
        panic!("{seam:?} names a split");
    };
    let rects = state.rects(AREA);
    (children.iter())
        .map(|child| along(super::seams::bounds(child, &rects), *axis))
        .collect()
}

/// The first pane of the child at `index` of `seam`'s split.
fn first_slot(state: &CanvasState, seam: &SeamAddress, index: usize) -> PaneSlot {
    let Some(PaneTree::Split { children, .. }) = super::seams::node_at(state.tree(), &seam.path)
    else {
        panic!("{seam:?} names a split");
    };
    children[index].panes()[0].0
}

/// Drag every seam of `state` the whole canvas each way and back, checking
/// the fold, the fill and the way back; `case` names it in a failure.
fn every_seam_folds_and_comes_back(state: &CanvasState, case: &str) {
    for seam in tree_seams(state, AREA, DIVIDER) {
        let name = format!("{case} {:?}", seam.address);
        let whole = along(AREA, seam.axis);
        let (index, before) = (seam.address.index, extents(state, &seam.address));
        for (by, folded) in [(whole, index + 1), (-whole, index)] {
            let (kept, mut live) = (index + index + 1 - folded, state.clone());
            let mut held = None;
            for _ in 0..16 {
                let step = (&seam.address, by / 16.0);
                live.step_seam_drag(&mut held, step, AREA).unwrap();
            }
            let rails = tree_rails(&live, AREA, DIVIDER);
            assert_eq!(rails.len(), 1, "{name} by {by}: one rail");
            let rail = rails[0];
            assert_eq!(rail.axis, seam.axis, "{name}");
            assert_eq!(rail.slot, first_slot(state, &seam.address, folded), "{name}: the far side");
            assert!(
                (along(rail.rect, seam.axis) - COLLAPSED_PANE_WIDTH_PX).abs() < EPS,
                "{name} by {by}: one rail wide, {:?}",
                rail.rect
            );
            let after = extents(&live, &seam.address);
            let pair = before[index] + before[index + 1];
            assert!(
                (after[kept] + after[folded] - pair).abs() < EPS,
                "{name} by {by}: the other side fills the rest: {before:?} -> {after:?}"
            );
            for other in (0..before.len()).filter(|at| *at != index && *at != index + 1) {
                assert!((after[other] - before[other]).abs() < EPS, "{name}: {other} kept");
            }
            for _ in 0..16 {
                let step = (&seam.address, -by / 16.0);
                live.step_seam_drag(&mut held, step, AREA).unwrap();
            }
            assert!(live.railed().is_empty(), "{name} by {by}: back open");
            let (was, now) = (shares_of(state), shares_of(&live));
            assert!(
                was.iter().zip(&now).all(|(a, b)| (a - b).abs() < 1e-5),
                "{name} by {by}: the pre-drag shares, {was:?} -> {now:?}"
            );
        }
    }
}

/// Every seam of every layout folds the side it is dragged towards to one
/// rail, as "2 stacked + Flow" does, and gives it back on the way back: as
/// laid out, with a floating chart added, and after a minimize and restore.
#[test]
fn every_seam_of_every_layout_folds_to_one_rail_and_reopens() {
    for preset in LAYOUT_PRESETS {
        let state = CanvasState::from_preset(preset);
        every_seam_folds_and_comes_back(&state, preset.id);
        let mut floated = state.clone();
        if floated.add(PaneKind::Time, AREA).is_ok() {
            every_seam_folds_and_comes_back(&floated, &format!("{} + float", preset.id));
        }
        let mut restored = state.clone();
        let slot = restored.tree().panes()[0].0;
        if restored.minimize(slot).is_ok() {
            restored.restore(slot).unwrap();
            every_seam_folds_and_comes_back(&restored, &format!("{} restored", preset.id));
        }
    }
}

/// The 2x2 grid of "4 TF + Flow", dragged shut, is one rail, not four strips
/// end to end; no seam inside it is listed, and the seam beside it carries it.
#[test]
fn a_folded_grid_is_one_rail_wide() {
    let mut state = canvas("grid4+flow");
    drag(&mut state, &root(0), -AREA.w);
    assert_eq!(state.railed().len(), 4, "every chart of the grid folded");
    let rails = tree_rails(&state, AREA, DIVIDER);
    assert_eq!(rails.len(), 1);
    assert!((rails[0].rect.w - COLLAPSED_PANE_WIDTH_PX).abs() < EPS, "{:?}", rails[0]);
    assert_eq!(rails[0].rect.h, AREA.h);
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert_eq!(seams.len(), 1, "no seam inside the rail");
    assert_eq!(seams[0].rail, Some(rails[0]));
    let flow = rect_of(&state, 4);
    assert!((flow.x + flow.w - AREA.w).abs() < EPS && flow.w > AREA.w - 16.0);
    let bands = tree_bands(&state, &mut Seats::default(), AREA, DIVIDER);
    assert!(bands.iter().filter(|band| band.railed).count() == 4);
}

/// Beside a rail the seam takes the column painter's rail target: the
/// rail's hit width from its outer edge, through the seam into the chart.
#[test]
fn a_seam_beside_a_rail_is_grabbed_over_the_rail_hit_width() {
    let mut state = canvas("charts-2");
    let open = tree_seams(&state, AREA, DIVIDER)[0].clone();
    assert_eq!(open.grab(6.0), CanvasRect::new(792.0, 0.0, 16.0, 900.0));
    drag(&mut state, &root(0), AREA.w);
    let seam = tree_seams(&state, AREA, DIVIDER)[0].clone();
    let rail = seam.rail.expect("the right chart folded");
    assert!(!rail.meets_after);
    let grab = seam.grab(6.0);
    assert_eq!(grab.right(), AREA.w);
    assert!((grab.w - COLLAPSED_HIT_PX).abs() < EPS, "{grab:?}");
}

/// A restore by call opens a rail at the shares the drag found it at, and a
/// maximize opens it too.
#[test]
fn a_restore_opens_a_rail_at_its_pre_drag_shares() {
    let mut state = canvas("time+flow");
    state.move_seam(&root(0), 200.0, AREA).unwrap();
    let before = root_shares(&state);
    drag(&mut state, &root(0), AREA.w);
    assert_eq!(state.mode(1), Some(PaneMode::Minimized), "folded, as a reader sees it");
    assert_eq!(state.restore(1), Ok(true));
    let after = root_shares(&state);
    assert!((before[0] - after[0]).abs() < 1e-5 && (before[1] - after[1]).abs() < 1e-5);
    drag(&mut state, &root(0), -AREA.w);
    assert_eq!(state.maximize(0), Ok(true));
    assert!(state.railed().is_empty() && state.minimized().is_empty());
}

/// The collapse calls fold the flow pane or the charts beside it on a tree
/// the column painter does not draw; the two never fold together.
#[test]
fn the_collapse_calls_fold_either_side_of_the_flow_pane() {
    let mut state = canvas("time+time+flow");
    assert_eq!(state.flow_side_folds(), (false, false));
    assert!(state.fold_flow_side(true, true), "the flow folds");
    assert_eq!(state.flow_side_folds(), (false, true));
    assert!(!state.fold_flow_side(true, true), "a repeat changes nothing");
    assert!(state.fold_flow_side(false, true), "the charts fold, the flow opens");
    assert_eq!(state.flow_side_folds(), (true, false));
    assert_eq!(state.railed().len(), 2);
    let flow = rect_of(&state, 2);
    assert!(flow.w > AREA.w - 2.0 * (COLLAPSED_PANE_WIDTH_PX + DIVIDER) - EPS);
    assert!(state.fold_flow_side(false, false));
    assert_eq!(state.flow_side_folds(), (false, false));
    assert_eq!(state, canvas("time+time+flow"), "back as laid out");
    let mut alone = canvas("charts-2");
    assert!(!alone.fold_flow_side(true, true), "no flow pane, nothing to fold");
    let mut strip = canvas("time+flow");
    strip.minimize(0).unwrap();
    assert!(!strip.fold_flow_side(true, true), "the last open chart stays open");
}

/// A rail is saved with the canvas and read back as itself.
#[test]
fn a_rail_survives_a_save() {
    let mut state = canvas("stack3+flow");
    drag(&mut state, &root(0), -AREA.w);
    assert_eq!(state.railed().len(), 3);
    let back: CanvasState = toml::from_str(&toml::to_string(&state).unwrap()).unwrap();
    assert_eq!(back, state);
    assert!(!back.numbered_as(preset("stack3+flow").unwrap()));
}

#[test]
fn dragging_a_seam_moves_the_boundary_and_keeps_the_shares_sum() {
    let mut state = canvas("charts-2");
    let before: f32 = root_shares(&state).iter().sum();
    assert_eq!(state.move_seam(&root(0), 100.0, AREA), Ok(true));
    assert!((rect_of(&state, 0).w - 900.0).abs() < EPS);
    assert!((rect_of(&state, 1).w - 700.0).abs() < EPS);
    let after: f32 = root_shares(&state).iter().sum();
    assert!((before - after).abs() < 1e-5, "{before} became {after}");
    assert_eq!(state.preset().unwrap().id, "charts-2", "still the layout");
    assert_eq!(state.move_seam(&root(0), 0.0, AREA), Ok(false), "no move");
}

#[test]
fn a_seam_stops_where_a_neighbour_keeps_its_minimum_extent() {
    let mut state = canvas("charts-2");
    assert_eq!(state.move_seam(&root(0), 10_000.0, AREA), Ok(true));
    assert!((rect_of(&state, 1).w - MIN_TREE_PANE_PX).abs() < EPS);
    assert_eq!(state.move_seam(&root(0), 10_000.0, AREA), Ok(false));
    assert_eq!(state.move_seam(&root(0), -10_000.0, AREA), Ok(true));
    assert!((rect_of(&state, 0).w - MIN_TREE_PANE_PX).abs() < EPS);
}

#[test]
fn a_nested_split_keeps_the_minimum_for_each_pane_along_the_axis() {
    let saved = r#"
next_slot = 3
[tree]
node = "split"
axis = "row"
shares = [1.0, 1.0]
[[tree.children]]
node = "pane"
slot = 0
kind = "time"
[[tree.children]]
node = "split"
axis = "row"
shares = [1.0, 1.0]
[[tree.children.children]]
node = "pane"
slot = 1
kind = "time"
[[tree.children.children]]
node = "pane"
slot = 2
kind = "time"
"#;
    let mut state: CanvasState = toml::from_str(saved).unwrap();
    assert_eq!(state.move_seam(&root(0), 10_000.0, AREA), Ok(true));
    let right = rect_of(&state, 1).w + rect_of(&state, 2).w;
    assert!((right - 2.0 * MIN_TREE_PANE_PX).abs() < EPS, "{right}");
}

#[test]
fn an_address_the_tree_does_not_hold_is_refused() {
    let mut state = canvas("charts-2");
    assert_eq!(
        state.move_seam(&root(1), 10.0, AREA),
        Err(CanvasRefusal::NoSeam)
    );
    let deep = SeamAddress {
        path: vec![0],
        index: 0,
    };
    assert_eq!(
        state.move_seam(&deep, 10.0, AREA),
        Err(CanvasRefusal::NoSeam)
    );
    assert_eq!(CanvasRefusal::NoSeam.code(), "canvas.no_seam");
}

#[test]
fn the_same_drags_give_the_same_tree_bit_for_bit() {
    let drag = |state: &mut CanvasState| {
        for delta in [37.5, -12.25, 400.0, -3.0] {
            state.move_seam(&root(0), delta, AREA).unwrap();
        }
    };
    let (mut one, mut two) = (canvas("charts-3"), canvas("charts-3"));
    drag(&mut one);
    drag(&mut two);
    let bits = |state: &CanvasState| -> Vec<u32> {
        root_shares(state).iter().map(|s| s.to_bits()).collect()
    };
    assert_eq!(bits(&one), bits(&two));
}

#[test]
fn a_pane_share_is_set_along_the_named_axis_and_read_back() {
    let mut state = canvas("charts-2");
    assert_eq!(state.resize_pane(0, SplitAxis::Row, 0.7, AREA), Ok(true));
    assert!((state.pane_share(0, SplitAxis::Row, AREA).unwrap() - 0.7).abs() < 1e-5);
    assert!((rect_of(&state, 0).w - 1120.0).abs() < EPS);
    // The last child moves the seam before it.
    assert_eq!(state.resize_pane(1, SplitAxis::Row, 0.25, AREA), Ok(true));
    assert!((state.pane_share(1, SplitAxis::Row, AREA).unwrap() - 0.25).abs() < 1e-5);
    assert_eq!(
        state.resize_pane(1, SplitAxis::Row, 0.25, AREA),
        Ok(false),
        "a repeat changes nothing"
    );
    // Held to the minimum extent, like a drag.
    assert_eq!(state.resize_pane(0, SplitAxis::Row, 0.01, AREA), Ok(true));
    assert!((rect_of(&state, 0).w - MIN_TREE_PANE_PX).abs() < EPS);
    assert_eq!(state.pane_share(0, SplitAxis::Column, AREA), None);
}

#[test]
fn a_pane_share_names_the_nearest_split_along_its_axis() {
    let mut state = canvas("charts-4");
    assert_eq!(state.resize_pane(3, SplitAxis::Column, 0.6, AREA), Ok(true));
    // Slot 3's row took 60 % of the height, slot 2 with it.
    assert!((state.pane_share(2, SplitAxis::Column, AREA).unwrap() - 0.6).abs() < 1e-5);
    assert!((state.pane_share(0, SplitAxis::Column, AREA).unwrap() - 0.4).abs() < 1e-5);
    assert!((state.pane_share(3, SplitAxis::Row, AREA).unwrap() - 0.5).abs() < 1e-5);
}

#[test]
fn a_pane_resize_is_refused_with_a_stable_reason() {
    let mut state = canvas("charts-2");
    assert_eq!(
        state.resize_pane(0, SplitAxis::Column, 0.5, AREA),
        Err(CanvasRefusal::NoSeam),
        "no split lays the panes top to bottom"
    );
    assert_eq!(
        state.resize_pane(99, SplitAxis::Row, 0.5, AREA),
        Err(CanvasRefusal::UnknownPane(99))
    );
    let slot = state.add(PaneKind::Time, AREA).unwrap();
    assert_eq!(
        state.resize_pane(slot, SplitAxis::Row, 0.5, AREA),
        Err(CanvasRefusal::NotDocked(slot))
    );
    let mut lone = canvas("time");
    assert_eq!(
        lone.resize_pane(0, SplitAxis::Row, 0.5, AREA),
        Err(CanvasRefusal::NoSeam)
    );
}

#[test]
fn resized_shares_survive_a_save_and_read_as_a_change() {
    let saved = canvas("charts-2");
    let mut live = saved.clone();
    live.move_seam(&root(0), 120.0, AREA).unwrap();
    assert!(
        !live.numbered_as(preset("charts-2").unwrap()),
        "saved whole"
    );
    assert_ne!(signal(&saved), signal(&live), "the recipe signal moves");
    let saved = toml::to_string(&live).unwrap();
    let back: CanvasState = toml::from_str(&saved).unwrap();
    assert_eq!(back, live);
    assert!((rect_of(&back, 0).w - 920.0).abs() < EPS);
}

#[test]
fn a_split_with_no_children_has_no_seam_and_does_not_panic() {
    let saved = r#"
next_slot = 0
[tree]
node = "split"
axis = "row"
shares = []
children = []
"#;
    let state: CanvasState = toml::from_str(saved).unwrap();
    assert!(tree_seams(&state, AREA, DIVIDER).is_empty());
}

#[test]
fn a_pair_narrower_than_both_minimums_is_refused_with_a_stable_reason() {
    let narrow = CanvasRect::new(0.0, 0.0, 2.0 * MIN_TREE_PANE_PX - 1.0, 900.0);
    let mut state = canvas("charts-2");
    let before = root_shares(&state);
    assert_eq!(
        state.move_seam(&root(0), 10.0, narrow),
        Err(CanvasRefusal::NoRoom)
    );
    assert_eq!(
        state.resize_pane(0, SplitAxis::Row, 0.6, narrow),
        Err(CanvasRefusal::NoRoom)
    );
    assert_eq!(root_shares(&state), before, "nothing moved");
    assert_eq!(CanvasRefusal::NoRoom.code(), "canvas.no_room");
}

#[test]
fn a_pair_exactly_as_wide_as_both_minimums_never_panics_on_rounding() {
    // At 240 px some shares round the low bound past the high one.
    let snug = CanvasRect::new(0.0, 0.0, 2.0 * MIN_TREE_PANE_PX, 900.0);
    for step in 1..4000_u16 {
        let half = f32::from(step) * 0.000_25;
        let mut state = canvas("charts-2");
        if let PaneTree::Split { shares, .. } = state.tree_mut() {
            shares.to_mut().copy_from_slice(&[half, half]);
        }
        let moved = state.move_seam(&root(0), 7.0, snug);
        assert!(moved.is_ok(), "{half}: {moved:?}");
        let shares = root_shares(&state);
        assert!((shares[0] + shares[1] - 2.0 * half).abs() < EPS, "{half}");
    }
}

#[test]
fn a_resize_trades_with_the_neighbour_whose_seam_moves_as_a_drag_does() {
    let mut state = canvas("charts-3");
    state.minimize(2).unwrap();
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert!(seams[0].resizable && !seams[1].resizable, "only 0|1 drags");
    assert_eq!(state.resize_pane(1, SplitAxis::Row, 0.5, AREA), Ok(true));
    assert!((rect_of(&state, 1).w - 800.0).abs() < EPS, "half the row");
    assert_eq!(rect_of(&state, 2).w, MINIMIZED_STRIP_PX, "the strip held");
    assert!((state.pane_share(1, SplitAxis::Row, AREA).unwrap() - 0.5).abs() < 1e-5);
    assert_eq!(
        state.resize_pane(2, SplitAxis::Row, 0.5, AREA),
        Err(CanvasRefusal::NoSeam),
        "a folded chart is pinned"
    );
    let mut first = canvas("charts-3");
    first.minimize(1).unwrap();
    assert_eq!(
        first.resize_pane(0, SplitAxis::Row, 0.5, AREA),
        Ok(true),
        "it trades across the folded strip with the next open chart"
    );
    assert!((rect_of(&first, 0).w - 800.0).abs() < EPS, "half the row");
    assert_eq!(rect_of(&first, 1).w, MINIMIZED_STRIP_PX, "the strip held");
    assert_eq!(
        first.resize_pane(1, SplitAxis::Row, 0.5, AREA),
        Err(CanvasRefusal::NoSeam),
        "a folded chart is still pinned"
    );
}

#[test]
fn a_seam_beside_a_middle_strip_still_resizes_its_open_neighbours() {
    let mut state = canvas("charts-3");
    state.minimize(1).unwrap();
    let (first, strip, last) = (rect_of(&state, 0), rect_of(&state, 1), rect_of(&state, 2));
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert!(
        seams.iter().all(|seam| seam.resizable),
        "each seam pairs across the strip"
    );
    assert_eq!(state.move_seam(&root(0), 50.0, AREA), Ok(true));
    assert!((rect_of(&state, 0).w - (first.w + 50.0)).abs() < EPS);
    assert!((rect_of(&state, 2).w - (last.w - 50.0)).abs() < EPS);
    assert_eq!(rect_of(&state, 1).w, MINIMIZED_STRIP_PX, "the strip held");
    assert!(
        (rect_of(&state, 1).x - (strip.x + 50.0)).abs() < EPS,
        "and slid along"
    );
    assert_eq!(state.move_seam(&root(1), -50.0, AREA), Ok(true));
    assert!((rect_of(&state, 0).w - first.w).abs() < EPS, "back again");
}

#[test]
fn a_pane_share_reads_what_the_screen_shows() {
    let mut state = canvas("charts-2");
    state.minimize(1).unwrap();
    let strip = MINIMIZED_STRIP_PX / AREA.w;
    assert!((state.pane_share(1, SplitAxis::Row, AREA).unwrap() - strip).abs() < 1e-6);
    assert!((state.pane_share(0, SplitAxis::Row, AREA).unwrap() - (1.0 - strip)).abs() < 1e-6);
    let mut state = canvas("charts-2");
    state.maximize(0).unwrap();
    assert_eq!(
        state.pane_share(0, SplitAxis::Row, AREA),
        None,
        "a maximized chart covers the canvas, in no split"
    );
    assert_eq!(state.pane_share(1, SplitAxis::Row, AREA), None, "hidden");
}

#[test]
fn a_drag_and_a_call_to_the_same_boundary_store_the_same_shares() {
    let (mut dragged, mut called) = (canvas("charts-2"), canvas("charts-2"));
    dragged.move_seam(&root(0), 160.0, AREA).unwrap();
    called.resize_pane(0, SplitAxis::Row, 0.6, AREA).unwrap();
    let (one, two) = (root_shares(&dragged), root_shares(&called));
    assert!((one[0] - two[0]).abs() < 1e-6 && (one[1] - two[1]).abs() < 1e-6);
}

/// The column painter's split and the tree's row share are one number: the
/// painter drawing writes it, the other reads it, a fold or a float keeps it,
/// and a tree no longer laid out as the layout is left alone.
#[test]
fn the_column_split_mirrors_the_tree_share() {
    let time_flow = preset("time+flow").unwrap();
    let mut state = CanvasState::from_preset(time_flow);
    let mut split = 0.4;
    assert!(
        !state.mirror_split(time_flow, &mut split, true),
        "already one"
    );
    split = 0.25;
    assert!(state.mirror_split(time_flow, &mut split, true));
    assert_eq!(state.preset(), Some(time_flow), "still the layout");
    assert!((state.pane_share(0, SplitAxis::Row, AREA).unwrap() - 0.25).abs() < 1e-4);
    state.minimize(0).unwrap();
    let mut read = 0.9;
    assert!(state.mirror_split(time_flow, &mut read, false));
    assert!(
        (read - 0.25).abs() < 1e-6,
        "a fold keeps the stored share: {read}"
    );
    state.restore(0).unwrap();
    assert_eq!(
        CanvasState::from_preset_split(time_flow, Some(0.25)),
        state,
        "a save keeping the name and the split rebuilds the tree"
    );
    assert_eq!(
        CanvasState::from_preset_split(time_flow, None),
        CanvasState::from_preset(time_flow)
    );
    let fresh = CanvasState::from_preset(time_flow);
    assert_eq!(
        fresh.saved_form(time_flow, 0.35, true),
        None,
        "rebuilt from name and split"
    );
    assert_eq!(state.saved_form(time_flow, 0.25, true), None);
    let mut folded = state.clone();
    folded.minimize(0).unwrap();
    assert_eq!(
        folded.saved_form(time_flow, 0.25, false),
        Some(folded.clone())
    );

    let stacked = preset("stack2+flow").unwrap();
    let mut state = CanvasState::from_preset(stacked);
    let mut split = 0.3;
    assert!(state.mirror_split(stacked, &mut split, true));
    assert!((state.pane_share(1, SplitAxis::Row, AREA).unwrap() - 0.3).abs() < 1e-4);
    let mut other = 0.5;
    assert!(
        !state.mirror_split(time_flow, &mut other, false),
        "another layout's tree is not this split"
    );
    assert_eq!(other, 0.5);
}
