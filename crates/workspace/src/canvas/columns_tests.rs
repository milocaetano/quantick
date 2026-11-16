//! The column painter's folds, the pane a layout switch reveals and when the
//! column painter draws.

use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);

fn layout(id: &str) -> &'static LayoutPreset {
    preset(id).unwrap()
}

#[test]
fn a_switch_reveals_the_pane_that_appeared() {
    let (flow, time, split) = (layout("flow"), layout("time"), layout("time+flow"));
    assert_eq!(revealed(time, flow), Some(PaneKind::Flow));
    assert_eq!(revealed(flow, time), Some(PaneKind::Time));
    assert_eq!(revealed(flow, split), Some(PaneKind::Time));
    assert_eq!(revealed(time, split), Some(PaneKind::Flow));
    assert_eq!(revealed(split, layout("stack2+flow")), None);
}

#[test]
fn a_preset_counts_its_panes_by_kind() {
    let stacked = layout("stack2+flow");
    assert_eq!(stacked.count(PaneKind::Time), 2);
    assert_eq!(stacked.count(PaneKind::Flow), 1);
    assert_eq!(layout("flow").count(PaneKind::Time), 0);
}

#[test]
fn the_column_painter_draws_only_its_own_preset_untouched() {
    let split = layout("time+flow");
    let mut state = CanvasState::from_preset(split);
    assert!(state.column_painted(split, true));
    assert!(!state.column_painted(split, false));
    assert!(!state.column_painted(layout("time"), true));
    let slot = state.panes()[0].0;
    state.minimize(slot).unwrap();
    assert!(!state.column_painted(split, true));
    let mut floated = CanvasState::from_preset(split);
    floated.float(slot, AREA).unwrap();
    assert!(!floated.column_painted(split, true));
}

/// A fold is one rail in the canvas model whichever painter draws it: folded
/// on a column layout, it stays the same rail when a floating chart drops
/// the tab onto the tree and when the chart leaves again, reads back the
/// same both ways, and a drag from it reopens it.
#[test]
fn a_column_fold_stays_one_rail_across_the_painter_switch() {
    for (id, flow) in [("time+flow", true), ("stack2+flow", false)] {
        let preset = layout(id);
        let mut state = CanvasState::from_preset(preset);
        assert!(state.fold_flow_side(flow, true), "{id}");
        let folds = state.flow_side_folds();
        assert!(
            state.column_painted(preset, true),
            "{id}: the column painter draws its fold"
        );
        let one_rail = |state: &CanvasState| {
            let rails = tree_rails(state, AREA, CANVAS_DIVIDER_PX);
            rails.len() == 1 && (rails[0].rect.w - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3
        };
        assert!(one_rail(&state), "{id}");
        let float = state.add(PaneKind::Time, AREA).unwrap();
        assert!(
            !state.column_painted(preset, true),
            "{id}: the tree paints now"
        );
        assert!(one_rail(&state), "{id}: still one rail on the tree");
        assert_eq!(state.flow_side_folds(), folds, "{id}: the same readback");
        state.remove(float).unwrap();
        assert!(
            state.column_painted(preset, true),
            "{id}: back on the columns"
        );
        assert!(one_rail(&state), "{id}: still one rail");
        assert_eq!(state.flow_side_folds(), folds, "{id}");
        let seam = SeamAddress {
            path: Vec::new(),
            index: 0,
        };
        let mut held = None;
        let by = if flow { -800.0 } else { 800.0 };
        state
            .step_seam_drag(&mut held, (&seam, by, true), AREA)
            .unwrap();
        assert_eq!(
            state.flow_side_folds(),
            (false, false),
            "{id}: a drag reopens it"
        );
        assert!(state.column_painted(preset, true), "{id}");
    }
}

/// A strip from the pane menu is no column fold: the tree paints it.
#[test]
fn a_menu_strip_keeps_the_tree_painting() {
    let preset = layout("time+flow");
    let mut state = CanvasState::from_preset(preset);
    state.minimize(0).unwrap();
    assert!(!state.column_painted(preset, true));
}

/// A chart docked beside the flow pane while the charts there are folded:
/// the readback still says the context is folded while any rail stands.
#[test]
fn the_context_readback_agrees_with_a_rail_after_a_dock() {
    let mut state = CanvasState::from_preset(layout("time+flow"));
    assert!(state.fold_flow_side(false, true));
    let float = state.add(PaneKind::Time, AREA).unwrap();
    state.dock(float, DockTarget::Canvas(Edge::Right)).unwrap();
    assert_eq!(tree_rails(&state, AREA, CANVAS_DIVIDER_PX).len(), 1);
    assert_eq!(state.flow_side_folds(), (true, false));
}

/// The stack's charts, top to bottom, of a canvas laid out as `id`.
fn stacked(id: &str) -> (CanvasState, Vec<PaneSlot>) {
    let state = CanvasState::from_preset(layout(id));
    let panes = state.tree().panes().into_iter();
    let slots = panes.filter(|(_, kind)| *kind == PaneKind::Time);
    let slots = slots.map(|(slot, _)| slot).collect();
    (state, slots)
}

/// A chart folded inside the stack is one rail of the canvas model, and the
/// column painter still draws the canvas: its fold is no column fold, the
/// stack folded whole is the column's own and stays so, and a save that
/// keeps only the preset, the split and the stack's flags rebuilds it.
#[test]
fn a_stacked_chart_folds_in_the_model_and_the_columns_still_draw() {
    let preset = layout("stack2+flow");
    let (mut state, slots) = stacked("stack2+flow");
    assert!(state.rail_stack(&slots, &[true, false]));
    assert_eq!(
        state.railed().iter().copied().collect::<Vec<_>>(),
        [slots[0]]
    );
    assert!(state.column_painted(preset, true), "the columns draw it");
    assert_eq!(state.flow_side_folds(), (false, false), "no column fold");
    assert_eq!(
        state.saved_form(preset, 0.4, true),
        None,
        "rebuilt from flags"
    );
    assert!(!state.rail_stack(&slots, &[true, false]), "again: nothing");
    assert!(
        !state.rail_stack(&slots, &[true, true]),
        "never the stack whole"
    );
    assert!(state.rail_stack(&slots, &[false, true]));
    assert_eq!(
        state.railed().iter().copied().collect::<Vec<_>>(),
        [slots[1]]
    );

    // The column folded whole is the column's fold, which the stack leaves.
    assert!(state.fold_flow_side(false, true));
    assert!(
        !state.rail_stack(&slots, &[false, false]),
        "the column's own"
    );
    assert_eq!(state.flow_side_folds(), (true, false));
}

/// The flow pane folds and opens beside a folded stacked chart without
/// opening it, by the call and by the column painter's divider; restoring
/// the chart by name and the context's expand open it.
#[test]
fn a_stacked_fold_outlives_the_flow_fold_and_opens_by_name() {
    let preset = layout("stack2+flow");
    let (mut state, slots) = stacked("stack2+flow");
    state.rail_stack(&slots, &[false, true]);
    assert!(state.fold_flow_side(true, true));
    assert!(state.railed().contains(&slots[1]), "the flow fold keeps it");
    assert!(state.column_painted(preset, true));
    let (mut hold, mut split) = (None, 0.4);
    let press = Hand::of([true, true, false], 0.0);
    state.column_hand((&mut hold, &mut split), press, (1592.0, 1600.0));
    let drag = Hand::of([false, true, true], -900.0);
    assert!(state.column_hand((&mut hold, &mut split), drag, (1592.0, 1600.0)));
    assert_eq!(state.flow_side_folds(), (false, false), "the flow opened");
    assert!(
        state.railed().contains(&slots[1]),
        "the stacked chart did not"
    );
    assert!(state.restore(slots[1]).unwrap());
    assert!(state.railed().is_empty());
    state.rail_stack(&slots, &[true, false]);
    assert!(state.fold_context(false), "the context's expand opens it");
    assert!(state.railed().is_empty());
}

/// "3 stacked + Flow" is drawn from the tree: a stacked chart folds there
/// by its seam, and is no column fold either.
#[test]
fn a_chart_of_three_stacked_folds_on_the_tree() {
    let (mut state, slots) = stacked("stack3+flow");
    let seam = SeamAddress {
        path: vec![0],
        index: 0,
    };
    let mut held = None;
    state
        .step_seam_drag(&mut held, (&seam, -900.0, true), AREA)
        .unwrap();
    assert_eq!(
        state.railed().iter().copied().collect::<Vec<_>>(),
        [slots[0]]
    );
    assert_eq!(state.flow_side_folds(), (false, false));
}
