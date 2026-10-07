//! The History press and its cancel through a fake window: the refusals that
//! name the field, and the run each answer reports.

use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeTab, FakeWindow};

fn window() -> FakeWindow {
    FakeWindow::new(vec![FakeTab::new(3, "WIN")])
}

fn call(
    handler: fn(
        &mut FakeWindow,
        &mut FakeAccess,
        &ActorContext,
        &Value,
    ) -> Result<Value, ControlError>,
    window: &mut FakeWindow,
    input: Value,
) -> Result<Value, ControlError> {
    handler(
        window,
        &mut FakeAccess::with_budget(0),
        &FakeWindow::assistant(),
        &input,
    )
}

#[test]
fn a_press_starts_the_run_and_moves_the_main_click_default() {
    let mut window = window();
    let result = call(load, &mut window, json!({ "reach": "sessions:2" })).expect("pressed");
    assert_eq!(result["tab_id"], "3");
    assert_eq!(result["reach"], "sessions:2");
    assert_eq!(result["press"], "started");
    assert_eq!(
        result["status"]["state"],
        RunStatus::Queued(HistoryReach::Sessions(2)).token()
    );
    assert_eq!(result["status"]["main_reach"], "sessions:2");
}

#[test]
fn a_press_on_a_running_tab_says_so() {
    let mut window = window();
    window.tabs[0].history = RunStatus::Queued(HistoryReach::Hours(4));
    let result = call(load, &mut window, json!({ "reach": "hours:1" })).expect("pressed");
    assert_eq!(result["press"], "already_running");
}

#[test]
fn an_unparsable_reach_names_the_field_and_what_is_accepted() {
    let mut window = window();
    let error = call(load, &mut window, json!({ "reach": "days:3" })).expect_err("refused");
    let details = error.context.details.expect("the refusal carries details");
    assert_eq!(details["field"], "reach");
    assert_eq!(details["sent"], "days:3");
    assert_eq!(
        details["accepted"],
        json!([
            format!("hours:1..={MAX_REACH_HOURS}"),
            format!("sessions:1..={MAX_REACH_SESSIONS}")
        ])
    );
    assert_eq!(
        error.context.next_steps,
        ["send reach as hours:N or sessions:N"]
    );
}

#[test]
fn a_feed_that_cannot_page_is_refused_before_anything_is_pressed() {
    let mut window = window();
    window.tabs[0].history_paging = false;
    let error = call(load, &mut window, json!({ "reach": "hours:2" })).expect_err("refused");
    assert_eq!(error.message, "this tab's feed cannot page older trades");
    assert!(matches!(window.tabs[0].history, RunStatus::Idle));
}

#[test]
fn a_cancel_reports_what_it_cancelled_and_the_idle_run() {
    let mut window = window();
    window.tabs[0].history = RunStatus::Queued(HistoryReach::Hours(4));
    let result = call(cancel, &mut window, json!({})).expect("cancelled");
    assert_eq!(result["cancelled"], "queued");
    assert_eq!(result["status"]["state"], RunStatus::Idle.token());
    let again = call(cancel, &mut window, json!({})).expect("cancelled");
    assert_eq!(again["cancelled"], "nothing");
}

#[test]
fn an_idle_snapshot_carries_only_the_main_reach() {
    let snapshot = snapshot(RunStatus::Idle, HistoryReach::Hours(1));
    assert_eq!(snapshot.reach, None);
    assert_eq!(snapshot.main_reach, HistoryReach::Hours(1).token());
    assert_eq!(snapshot.back_to_unix_ms, None);
    assert_eq!(snapshot.prints_pulled, None);
}
