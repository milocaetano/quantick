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
