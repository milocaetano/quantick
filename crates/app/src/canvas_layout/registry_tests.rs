use super::*;

/// The registry is the vocabulary. Every id is a name a config file, the
/// `QUANTICK_LAYOUT` hook and a saved workspace may all use, so two
/// presets answering to one name would make a saved layout ambiguous.
#[test]
fn every_preset_id_is_unique() {
    for (index, preset) in LAYOUT_PRESETS.iter().enumerate() {
        for other in LAYOUT_PRESETS.iter().skip(index + 1) {
            assert_ne!(preset.id, other.id, "two presets answer to {}", preset.id);
        }
    }
}

#[test]
fn every_preset_is_reachable_by_its_id() {
    for entry in LAYOUT_PRESETS {
        assert_eq!(preset(entry.id), Some(entry));
    }
    assert_eq!(preset("no such layout"), None);
    assert_eq!(preset("  flow  "), preset("flow"), "ids are read trimmed");
}

/// The rule that keeps the heatmap the protagonist, held as data rather
/// than as a check inside the layout engine: a preset that draws the flow
/// pane draws it **last**, so context charts default to its left.
#[test]
fn the_flow_pane_is_always_the_rightmost_pane_a_preset_holds() {
    for entry in LAYOUT_PRESETS {
        let flow_at = entry.kinds.iter().position(|kind| *kind == PaneKind::Flow);
        if let Some(index) = flow_at {
            assert_eq!(
                index,
                entry.kinds.len() - 1,
                "preset {} puts a context pane right of the flow pane",
                entry.id
            );
        }
        assert_eq!(
            entry.kinds.iter().filter(|k| **k == PaneKind::Flow).count(),
            flow_at.map_or(0, |_| 1),
            "preset {} holds more than one flow pane",
            entry.id
        );
    }
}

#[test]
fn no_preset_asks_for_more_panes_than_the_canvas_allows() {
    for entry in LAYOUT_PRESETS {
        assert!(!entry.kinds.is_empty(), "preset {} draws nothing", entry.id);
        assert!(
            entry.kinds.len() <= MAX_CANVAS_PANES,
            "preset {} wants {} panes",
            entry.id,
            entry.kinds.len()
        );
    }
}

#[test]
fn every_preset_lays_out_and_spends_the_canvas() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1600.0, 900.0));
    for entry in LAYOUT_PRESETS {
        let widths = vec![PaneWidth::Auto; entry.kinds.len()];
        let areas = split_row(area, &widths);
        assert_eq!(areas.panes.len(), entry.kinds.len(), "preset {}", entry.id);
        assert_eq!(areas.dividers.len(), entry.kinds.len() - 1);
    }
}

/// Width is decided by `PaneWidth` alone — a pane's kind never reaches
/// the arithmetic. Asserted here as a property of the signature rather
/// than by comparing two identical calls: the row is laid out from widths
/// only, so there is no kind to pass in, and a rail is a rail whatever the
/// pane it hides holds.
#[test]
fn the_rail_is_the_same_width_at_every_position_in_the_row() {
    let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
    let collapsed = PaneWidth::Collapsed { restore: 0.3 };

    // Leading, interior and trailing: each loses a different number of
    // divider halves, and each must still come out a full rail wide.
    for widths in [
        vec![collapsed, PaneWidth::Auto, PaneWidth::Auto],
        vec![PaneWidth::Auto, collapsed, PaneWidth::Auto],
        vec![PaneWidth::Auto, PaneWidth::Auto, collapsed],
    ] {
        let index = widths
            .iter()
            .position(|width| width.is_collapsed())
            .expect("the row under test holds a collapsed pane");
        let areas = split_row(area, &widths);
        assert!(
            (areas.panes[index].width() - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3,
            "a rail at position {index} came out {} px",
            areas.panes[index].width()
        );
    }
}
