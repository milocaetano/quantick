//! The column painter's folds, the pane a layout switch reveals and when the
//! column painter draws.

use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);

fn layout(id: &str) -> &'static LayoutPreset {
    preset(id).unwrap()
}

#[test]
fn folding_one_column_opens_the_other() {
    let (mut context, mut flow) = (false, true);
    assert!(fold_column(&mut context, &mut flow, true));
    assert_eq!((context, flow), (true, false));
    assert!(fold_column(&mut flow, &mut context, true));
    assert_eq!((context, flow), (false, true));
}

#[test]
fn a_fold_already_in_place_changes_nothing() {
    let (mut context, mut flow) = (true, false);
    assert!(!fold_column(&mut context, &mut flow, true));
    let (mut open, mut other) = (false, true);
    assert!(!fold_column(&mut open, &mut other, false));
    assert_eq!((open, other), (false, true));
}

#[test]
fn opening_a_folded_column_leaves_the_other_alone() {
    let (mut context, mut flow) = (true, false);
    assert!(fold_column(&mut context, &mut flow, false));
    assert_eq!((context, flow), (false, false));
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
