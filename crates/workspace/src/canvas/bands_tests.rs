//! The bands the window paints a canvas from, and the readings it compares
//! canvases by.

use super::*;
use crate::workspace_recipe::DeclaredLayout;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1200.0, 800.0);

/// Removing a chart never moves the charts kept onto another pane: each time
/// slot keeps the pane it was seated on.
#[test]
fn removing_a_chart_keeps_the_others_on_their_panes() {
    let preset = preset("stack2+flow").unwrap();
    let mut state = CanvasState::from_preset(preset);
    let mut seats = Vec::new();
    let before = tree_bands(&state, &mut seats, AREA, 4.0);
    assert_eq!(before.iter().filter(|band| band.seat == Some(1)).count(), 1);
    state.remove(0).unwrap();
    let after = tree_bands(&state, &mut seats, AREA, 4.0);
    let seats: Vec<_> = after.iter().map(|band| band.seat).collect();
    assert_eq!(seats, vec![Some(1), None], "the kept chart stays on pane 1");
}

/// A seam inside the canvas gives half a divider to each side; the canvas's
/// own edges give nothing.
#[test]
fn seams_inset_inner_edges_only() {
    let left = CanvasRect::new(0.0, 0.0, 600.0, 800.0);
    assert_eq!(
        inset_seams(left, AREA, 4.0),
        CanvasRect::new(0.0, 0.0, 598.0, 800.0)
    );
    let middle = CanvasRect::new(300.0, 200.0, 600.0, 400.0);
    assert_eq!(
        inset_seams(middle, AREA, 4.0),
        CanvasRect::new(302.0, 202.0, 596.0, 396.0)
    );
}

/// Two canvases with the same arrangement under different slot numbers show
/// the same layout; a fold makes them differ.
#[test]
fn same_layout_ignores_slot_numbers() {
    let preset = preset("stack2+flow").unwrap();
    let saved = CanvasState::from_preset(preset);
    let mut live = CanvasState::from_preset(super::preset("time+flow").unwrap());
    live.apply_preset(preset);
    assert_ne!(live.tree(), saved.tree(), "the flow pane kept slot 1");
    assert!(same_layout(&saved, &live));
    live.minimize(1).unwrap();
    assert!(!same_layout(&saved, &live));
}

/// The marks follow the save: a fold the save lacks dots the layout, a
/// workspace that agrees shows nothing, and each column lists its presets.
#[test]
fn layout_marks_compare_against_the_save() {
    let saved = CanvasState::from_preset(preset("stack2+flow").unwrap());
    let mut live = saved.clone();
    let clean = LayoutMarks::of(&live, Some(&saved), true);
    assert_eq!(clean.current, clean.saved);
    assert!(!clean.layout_unsaved, "the same layout");
    live.minimize(0).unwrap();
    assert!(LayoutMarks::of(&live, Some(&saved), true).layout_unsaved);
    assert!(!LayoutMarks::of(&live, Some(&saved), false).layout_unsaved);
    let columns = PresetGroup::COLUMNS.map(|(group, _)| group.presets().len());
    assert_eq!(columns.iter().sum::<usize>(), LAYOUT_PRESETS.len());
}

/// The split a column painter takes from a preset is where its tree puts the
/// flow pane; a catalogue cell wraps a long name at " + ".
#[test]
fn presets_answer_their_split_label_and_word() {
    let flow = preset("flow").unwrap();
    assert_eq!(flow.flow_left(), Some(0.0));
    assert_eq!(preset("time").unwrap().flow_left(), None);
    let stacked = preset("stack2+flow").unwrap();
    assert!(stacked.flow_left().is_some_and(|x| x > 0.0 && x < 1.0));
    assert_eq!(stacked.cell_label(), "2 stacked\n+ Flow");
    assert_eq!(flow.cell_label(), "Flow");
    assert_eq!(
        DeclaredLayout::nearest(stacked),
        DeclaredLayout::TimeTimeAndFlow
    );
    assert_eq!(DeclaredLayout::nearest(flow), DeclaredLayout::Flow);
    let charts = preset("charts-4").unwrap();
    assert_eq!(DeclaredLayout::nearest(charts), DeclaredLayout::Time);
    assert_eq!(charts.time_panes(), 4);
}
