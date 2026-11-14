//! Two drag-folds in a row: the second leaves the first rail folded, save
//! where it drags the seam beside that rail, which reopens it by the rule
//! `a_seam_beside_a_strip_never_moves_but_one_beside_a_rail_reopens_it`
//! states.

use super::seams::{node_at, partners, railed};
use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
const DIVIDER: f32 = 4.0;

fn canvas(id: &str) -> CanvasState {
    CanvasState::from_preset(preset(id).unwrap())
}

fn seam(path: &[usize], index: usize) -> SeamAddress {
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
    let axis = tree_seams(state, AREA, DIVIDER)
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
    second_fold_keeps_the_first("stack2+flow", (seam(&[0], 0), true), (seam(&[], 0), true));
}

#[test]
fn grid4_flow_folding_the_flow_keeps_the_grid_chart_folded() {
    second_fold_keeps_the_first("grid4+flow", (seam(&[0, 0], 0), true), (seam(&[], 0), true));
}

#[test]
fn grid4_flow_folding_the_bottom_row_keeps_the_top_row_chart_folded() {
    second_fold_keeps_the_first(
        "grid4+flow",
        (seam(&[0, 0], 0), true),
        (seam(&[0], 0), true),
    );
}

#[test]
fn flow_over_2time_folding_the_flow_keeps_the_chart_folded() {
    second_fold_keeps_the_first(
        "flow-over-2time",
        (seam(&[1], 0), true),
        (seam(&[], 0), false),
    );
}

/// "6 charts", `[0]/0` then `[0]/1`: the second seam borders the first
/// rail, so its drag reopens that rail and folds the chart past it, as a
/// seam beside a rail always does.
#[test]
fn charts6_a_seam_beside_the_first_rail_reopens_it_by_the_rail_rule() {
    let mut state = canvas("charts-6");
    drag_shut(&mut state, &seam(&[0], 0), true);
    assert_eq!(state.railed().iter().copied().collect::<Vec<_>>(), [1]);
    let (_, beside) = fold_side(&state, &seam(&[0], 1), true);
    assert!(beside, "seam [0]/1 borders the rail");
    drag_shut(&mut state, &seam(&[0], 1), true);
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
        let seams = tree_seams(&fresh, AREA, DIVIDER);
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
                    let listed = tree_seams(&state, AREA, DIVIDER)
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
