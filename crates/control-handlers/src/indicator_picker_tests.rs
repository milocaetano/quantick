//! `indicator.picker.set` through the fake window.
use super::*;
use crate::indicator_host::IndicatorRefusal;
use crate::test_support::{FakeAccess, FakeWindow};
use serde_json::json;

#[test]
fn the_picker_answers_and_is_journaled() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let input = json!({ "operation": { "action": "open" }, "offset": 4 });
    let result = set(&mut window, &mut access, &FakeWindow::assistant(), &input).unwrap();
    assert_eq!(result["next_offset"], 4);
    assert_eq!(access.events.len(), 1);
}

#[test]
fn the_pickers_refusal_keeps_its_words() {
    let mut window = FakeWindow::new(Vec::new());
    window.indicators.refusal = Some(IndicatorRefusal::Rejected("query exceeds 256 bytes".into()));
    let mut access = FakeAccess::with_budget(0);
    let input = json!({ "operation": { "action": "inspect" } });
    let error = set(&mut window, &mut access, &FakeWindow::assistant(), &input).unwrap_err();
    assert_eq!(error.message, "query exceeds 256 bytes");
    assert!(access.events.is_empty());
}
