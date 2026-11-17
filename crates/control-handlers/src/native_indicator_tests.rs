//! `indicator.native.*` through the fake window.
use super::*;
use crate::test_support::FakeWindow;
use serde_json::json;

fn list_target(window: &mut FakeWindow) -> Result<Value, ControlError> {
    let input = json!({ "target": { "tab_id": "1", "pane_id": "2" } });
    list_request(window, &mut (), &FakeWindow::assistant(), &input)
}

#[test]
fn an_unknown_pane_is_refused_in_its_words() {
    let mut window = FakeWindow::new(Vec::new());
    window.indicators.refusal = Some(IndicatorRefusal::UnknownTabOrPane);
    let error = list_target(&mut window).unwrap_err();
    assert_eq!(error.message, "unknown tab or pane");
}

#[test]
fn an_empty_pane_lists_nothing() {
    let mut window = FakeWindow::new(Vec::new());
    let result = list_target(&mut window).unwrap();
    assert_eq!(result["entries"], json!([]));
}

/// A saved volume indicator in slot 3, loaded or still loading.
fn volume(loaded: bool) -> (u64, quantick_stores::indicator_state::SavedIndicator, bool) {
    use quantick_stores::indicator_state::{SavedIndicator, SavedKind};
    let saved = SavedIndicator {
        kind: SavedKind::native("native.volume"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    };
    (3, saved, loaded)
}

fn edit_target(
    window: &mut FakeWindow,
    access: &mut crate::test_support::FakeAccess,
    operation: Value,
) -> Result<Value, ControlError> {
    let input = json!({ "target": { "tab_id": "1", "pane_id": "2" }, "operation": operation });
    edit_request(window, access, &FakeWindow::assistant(), &input)
}

/// Each edit is validated against the slot it names before the owner applies
/// anything; an applied edit answers its slot and is journaled once.
#[test]
fn an_edit_is_validated_before_it_is_applied_and_journaled_once() {
    let mut window = FakeWindow::new(Vec::new());
    window.indicators.saved = vec![volume(true)];
    let mut access = crate::test_support::FakeAccess::default();
    for refused in [
        json!({"action": "add", "native_id": "native.missing"}),
        json!({"action": "set_hidden", "slot_id": "9", "hidden": true}),
        json!({"action": "set_inputs", "slot_id": "3",
            "values": {"length": {"type": "int", "value": 0}}}),
        json!({"action": "set_plot_style", "slot_id": "3", "plot": 99,
            "style": {"color": [1, 2, 3, 4]}}),
        json!({"action": "set_plot_style", "slot_id": "3", "plot": 0,
            "style": {"width": "9"}}),
    ] {
        let error = edit_target(&mut window, &mut access, refused.clone()).unwrap_err();
        assert_eq!(
            error.code.as_str(),
            quantick_control::error::codes::INVALID_REQUEST,
            "{refused}"
        );
    }
    assert!(
        window.indicators.applied.is_empty(),
        "refused before any edit"
    );
    assert!(access.events.is_empty(), "a refusal journals nothing");

    let added = edit_target(
        &mut window,
        &mut access,
        json!({"action": "add", "native_id": "native.volume"}),
    )
    .unwrap();
    assert_eq!(
        (&added["slot_id"], &added["removed"]),
        (&json!("7"), &json!(false))
    );
    let styled = edit_target(
        &mut window,
        &mut access,
        json!({"action": "set_plot_style", "slot_id": "3", "plot": 0,
            "style": {"visible": true, "color": [10, 20, 30, 125], "width": "2.5"}}),
    )
    .unwrap();
    assert_eq!(styled["slot_id"], "3");
    let removed = edit_target(
        &mut window,
        &mut access,
        json!({"action": "remove", "slot_id": "3"}),
    )
    .unwrap();
    assert_eq!(removed["removed"], true);
    let applied: Vec<_> = window
        .indicators
        .applied
        .iter()
        .map(|(slot, edit)| (*slot, matches!(edit, wire::Prepared::Remove)))
        .collect();
    assert_eq!(applied, [(None, false), (Some(3), false), (Some(3), true)]);
    assert_eq!(access.events.len(), 3, "one event per applied edit");
}

/// Style and visibility wait for the slot to load; inputs do not.
#[test]
fn style_and_visibility_wait_for_the_indicator_to_load() {
    let mut window = FakeWindow::new(Vec::new());
    window.indicators.saved = vec![volume(false)];
    let mut access = crate::test_support::FakeAccess::default();
    for waiting in [
        json!({"action": "set_hidden", "slot_id": "3", "hidden": true}),
        json!({"action": "set_plot_style", "slot_id": "3", "plot": 0,
            "style": {"visible": false}}),
    ] {
        let error = edit_target(&mut window, &mut access, waiting.clone()).unwrap_err();
        assert!(
            error.message.contains("load"),
            "{waiting}: {}",
            error.message
        );
    }
    edit_target(
        &mut window,
        &mut access,
        json!({"action": "set_inputs", "slot_id": "3",
            "values": {"length": {"type": "int", "value": 17}}}),
    )
    .unwrap();
    assert_eq!(window.indicators.applied.len(), 1);
}
