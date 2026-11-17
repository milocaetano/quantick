//! `indicator.script_editor.set` through the fake window.
use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use serde_json::json;

fn call(window: &mut FakeWindow, input: &Value) -> Result<Value, ControlError> {
    let mut access = FakeAccess::with_budget(0);
    edit(window, &mut access, &FakeWindow::assistant(), input)
}

#[test]
fn a_page_out_of_range_never_reaches_the_owner() {
    let mut window = FakeWindow::new(Vec::new());
    let input = json!({ "operation": { "action": "add_to_chart", "id": "a.pine" }, "limit": 0 });
    let error = call(&mut window, &input).unwrap_err();
    assert_eq!(error.message, "limit must be in 1..=64");
    assert!(window.indicators.added.is_empty());
}

#[test]
fn add_to_chart_reaches_the_library_owner() {
    let mut window = FakeWindow::new(Vec::new());
    let input = json!({ "operation": { "action": "add_to_chart", "id": "a.pine" } });
    call(&mut window, &input).unwrap();
    assert_eq!(window.indicators.added, ["a.pine"]);
}

#[test]
fn the_owners_refusal_is_typed() {
    let mut window = FakeWindow::new(Vec::new());
    window.indicators.refusal = Some(IndicatorRefusal::ScriptNotOpen);
    let input = json!({ "operation": { "action": "save" } });
    let error = call(&mut window, &input).unwrap_err();
    assert_eq!(error.message, IndicatorRefusal::ScriptNotOpen.reason());
}
