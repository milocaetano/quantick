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

#[test]
fn a_seam_beside_a_folded_chart_is_painted_but_never_moves() {
    let mut state = canvas("charts-3");
    state.minimize(0).unwrap();
    let seams = tree_seams(&state, AREA, DIVIDER);
    assert_eq!(seams.len(), 2);
    assert!(!seams[0].resizable, "beside the folded strip");
    assert!(seams[1].resizable, "between the two open charts");
    assert_eq!(
        state.move_seam(&root(0), 50.0, AREA),
        Err(CanvasRefusal::NoSeam)
    );
    assert_eq!(state.move_seam(&root(1), 50.0, AREA), Ok(true));
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
