//! `indicator.guide.set` through the fake window.
use super::*;
use crate::indicator_host::IndicatorRefusal;
use crate::test_support::{FakeAccess, FakeWindow};
use serde_json::json;

fn call(window: &mut FakeWindow, access: &mut FakeAccess) -> Result<Value, ControlError> {
    let input = json!({ "tab_id": "1", "pane_id": "2", "slot_id": "3", "enabled": true });
    set(window, access, &FakeWindow::assistant(), &input)
}

#[test]
fn the_guide_reaches_the_owner_and_is_journaled() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let result = call(&mut window, &mut access).unwrap();
    assert_eq!(result["enabled"], true);
    assert_eq!(window.indicators.guides, [(1, 2, 3, true)]);
    assert_eq!(access.events.len(), 1);
}

#[test]
fn a_target_with_no_live_indicator_is_refused_unjournaled() {
    let mut window = FakeWindow::new(Vec::new());
    window.indicators.refusal = Some(IndicatorRefusal::NoLiveIndicator);
    let mut access = FakeAccess::with_budget(0);
    let error = call(&mut window, &mut access).unwrap_err();
    assert_eq!(error.message, IndicatorRefusal::NoLiveIndicator.reason());
    assert!(access.events.is_empty());
}
