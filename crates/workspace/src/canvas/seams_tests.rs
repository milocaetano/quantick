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
    assert!((state.pane_share(0, SplitAxis::Row).unwrap() - 0.7).abs() < 1e-5);
    assert!((rect_of(&state, 0).w - 1120.0).abs() < EPS);
    // The last child moves the seam before it.
    assert_eq!(state.resize_pane(1, SplitAxis::Row, 0.25, AREA), Ok(true));
    assert!((state.pane_share(1, SplitAxis::Row).unwrap() - 0.25).abs() < 1e-5);
    assert_eq!(
        state.resize_pane(1, SplitAxis::Row, 0.25, AREA),
        Ok(false),
        "a repeat changes nothing"
    );
    // Held to the minimum extent, like a drag.
    assert_eq!(state.resize_pane(0, SplitAxis::Row, 0.01, AREA), Ok(true));
    assert!((rect_of(&state, 0).w - MIN_TREE_PANE_PX).abs() < EPS);
    assert_eq!(state.pane_share(0, SplitAxis::Column), None);
}

#[test]
fn a_pane_share_names_the_nearest_split_along_its_axis() {
    let mut state = canvas("charts-4");
    assert_eq!(state.resize_pane(3, SplitAxis::Column, 0.6, AREA), Ok(true));
    // Slot 3's row took 60 % of the height, slot 2 with it.
    assert!((state.pane_share(2, SplitAxis::Column).unwrap() - 0.6).abs() < 1e-5);
    assert!((state.pane_share(0, SplitAxis::Column).unwrap() - 0.4).abs() < 1e-5);
    assert!((state.pane_share(3, SplitAxis::Row).unwrap() - 0.5).abs() < 1e-5);
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
