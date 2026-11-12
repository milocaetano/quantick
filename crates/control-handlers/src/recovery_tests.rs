//! Recovery through a fake window: which tab is named, which act runs, and
//! what the answer says when nothing could be respawned.

use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeTab, FakeWindow};
use quantick_control_host::{actions::ActionRegistry, journal::EventActor};

fn window() -> FakeWindow {
    let mut window = FakeWindow::new(vec![FakeTab::new(7, "BTCUSDT"), FakeTab::new(9, "WIN")]);
    window.active = 1;
    window
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
fn reconnect_without_a_tab_recovers_the_one_on_screen_and_keeps_its_timeline() {
    let mut window = window();
    let result = call(reconnect, &mut window, json!({})).expect("reconnect answers");
    assert_eq!(
        result,
        json!({ "tab_id": "9", "symbol": "WIN", "respawned": true, "timeline_kept": true })
    );
    assert_eq!(window.tabs[1].recoveries, [true]);
    assert!(window.tabs[0].recoveries.is_empty());
}

#[test]
fn a_reload_that_respawned_reports_the_timeline_thrown_away() {
    let mut window = window();
    let result = call(reload, &mut window, json!({ "tab_id": "7" })).expect("reload answers");
    assert_eq!(result["timeline_kept"], false);
    assert_eq!(window.tabs[0].recoveries, [false]);
}

#[test]
fn a_reload_with_nothing_to_spawn_says_the_timeline_was_kept() {
    let mut window = window();
    window.tabs[0].respawns = false;
    let result = call(reload, &mut window, json!({ "tab_id": "7" })).expect("reload answers");
    assert_eq!(result["respawned"], false);
    assert_eq!(result["timeline_kept"], true);
}

#[test]
fn an_unknown_tab_is_refused_by_its_id() {
    let mut window = window();
    let error = call(reconnect, &mut window, json!({ "tab_id": "42" })).expect_err("refused");
    assert_eq!(error.message, "no open tab has id 42");
    assert!(window.tabs.iter().all(|tab| tab.recoveries.is_empty()));
}

#[test]
fn each_recovery_names_the_capability_registered_for_it() {
    assert_eq!(capability_id(Recovery::Reconnect), RECONNECT_CAPABILITY_ID);
    assert_eq!(capability_id(Recovery::Reload), RELOAD_CAPABILITY_ID);
}

#[test]
fn both_capabilities_register_into_a_generic_registry() {
    let mut registry = ActionRegistry::<FakeWindow, FakeAccess>::new();
    register(&mut registry).expect("the recovery descriptors are valid");
    let ids: Vec<_> = registry
        .descriptors()
        .map(|descriptor| descriptor.id.as_str().to_owned())
        .collect();
    assert_eq!(ids, [RECONNECT_CAPABILITY_ID, RELOAD_CAPABILITY_ID]);
}

/// A rebuilt timeline names who rebuilt it, whoever it was. A reload with
/// nothing to respawn rebuilt nothing and records nothing.
#[test]
fn a_reload_journals_who_threw_the_timeline_away() {
    for actor in [FakeWindow::trader(), FakeWindow::assistant()] {
        let mut window = window();
        let mut access = FakeAccess::with_budget(0);
        let input = json!({ "tab_id": "7" });
        reload(&mut window, &mut access, &actor, &input).expect("reload answers");
        window.tabs[0].respawns = false;
        reload(&mut window, &mut access, &actor, &input).expect("reload answers");
        assert_eq!(access.events.len(), 1, "one rebuild, one record");
        let event = &access.events[0];
        assert_eq!(event.module_id.as_str(), "feed");
        assert_eq!(event.kind.as_str(), "feed.timeline.rebuilt");
        assert_eq!(event.actor, Some(EventActor::from(&actor)));
        assert_eq!(
            event.payload,
            json!({ "feed_reload": {
                "tab_id": "7",
                "symbol": "BTCUSDT",
                "respawned": true,
                "timeline_kept": false,
            }})
        );
    }
}
