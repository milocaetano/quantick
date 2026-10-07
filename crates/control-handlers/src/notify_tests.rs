//! The notify handlers driven through a fake window's attention lanes and a
//! fake access, with no application behind them.

use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

#[test]
fn a_toast_lands_on_the_fake_windows_lane_with_its_author() {
    let mut window = FakeWindow::new(Vec::new());
    let result = raise_toast(
        &mut window,
        &mut FakeAccess::with_budget(1),
        &FakeWindow::assistant(),
        &json!({ "message": "hello" }),
    )
    .expect("a toast within budget is raised");
    assert_eq!(result["raised"], true);
    assert_eq!(window.toasts, ["hello — fake assistant (agent)"]);
}

#[test]
fn every_refused_sound_is_reported_as_not_raised() {
    let mut window = FakeWindow::new(Vec::new());
    window.speaker_refusal = Some("no audio output device".to_owned());
    let mut access = FakeAccess::with_budget(2);
    for call in 0..2 {
        let result = sound_alert(
            &mut window,
            &mut access,
            &FakeWindow::assistant(),
            &json!({ "message": "listen" }),
        )
        .expect("a refused sound is an answer, not an error");
        assert_eq!(result["raised"], false, "call {call}");
        assert_eq!(result["unavailable_reason"], "no audio output device");
    }
    let delivered: Vec<_> = access
        .events
        .iter()
        .map(|event| event.payload["delivered"].clone())
        .collect();
    assert_eq!(delivered, [json!(false), json!(false)]);
}

#[test]
fn a_popup_carries_the_default_title_and_the_interfaces_attribution() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(1);
    raise_popup(
        &mut window,
        &mut access,
        &FakeWindow::assistant(),
        &json!({ "message": "look at the bid" }),
    )
    .expect("a popup within budget is raised");
    assert_eq!(
        window.popups,
        [AgentPopup {
            title: "Message from an assistant".to_owned(),
            message: "look at the bid".to_owned(),
            author: "fake assistant (agent)".to_owned(),
        }]
    );
    let event = &access.events[0];
    assert_eq!(event.kind.as_str(), NOTIFICATION_EVENT_KIND);
    assert_eq!(
        event.payload,
        json!({ "channel": "popup", "message": "look at the bid", "delivered": true })
    );
}

#[test]
fn a_spent_budget_is_refused_before_anything_is_shown_or_journaled() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let error = raise_toast(
        &mut window,
        &mut access,
        &FakeWindow::assistant(),
        &json!({ "message": "again" }),
    )
    .expect_err("a spent budget refuses");
    assert_eq!(error.code.as_str(), codes::BACKPRESSURE);
    assert!(error.retryable);
    assert!(
        error.context.next_steps[0].contains("retry in about 1 second(s)"),
        "{:?}",
        error.context.next_steps
    );
    assert!(window.toasts.is_empty());
    assert!(access.events.is_empty());
}

#[test]
fn every_channel_registers_into_a_generic_registry() {
    let mut registry = ActionRegistry::<FakeWindow, FakeAccess>::new();
    register(&mut registry).expect("the notify descriptors are valid");
    let ids: Vec<_> = registry
        .descriptors()
        .map(|descriptor| descriptor.id.as_str().to_owned())
        .collect();
    assert_eq!(ids, ["notify.popup", "notify.sound", "notify.toast"]);
}
