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
