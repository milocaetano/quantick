//! `indicator.script.attach` and `.detach` through the fake window: the
//! compile gate, the operator's authorship and the trader's own slot.
use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_control::error::codes;
use serde_json::json;

const SOURCE: &str = "//@version=5\nindicator(\"fake\")\nplot(close)\n";

fn attach_as(window: &mut FakeWindow, access: &mut FakeAccess) -> Result<Value, ControlError> {
    let input = json!({ "name": "fake", "source": SOURCE });
    attach(window, access, &FakeWindow::assistant(), &input)
}

fn detach_slot(window: &mut FakeWindow, access: &mut FakeAccess) -> Result<Value, ControlError> {
    let input = json!({ "slot_id": "0" });
    detach(window, access, &FakeWindow::assistant(), &input)
}

#[test]
fn an_agents_script_is_an_operators_and_detaches() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let attached = attach_as(&mut window, &mut access).expect("a script that compiles attaches");
    assert_eq!(attached["slot_id"], "0");
    assert!(
        window.indicators.attached[0].2,
        "an agent's script is an operator's"
    );

    let detached = detach_slot(&mut window, &mut access).expect("the operator's own slot");
    assert_eq!(detached["detached"], true);
    assert_eq!(access.events.len(), 2, "attach and detach are journaled");
}

#[test]
fn a_replayed_hand_attaches_as_the_recorded_actor() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    access.replayed = Some((ActorKind::HumanUi, "trader".to_owned()));
    attach_as(&mut window, &mut access).unwrap();
    assert!(
        !window.indicators.attached[0].2,
        "the trader's own hand, replayed"
    );

    let error = detach_slot(&mut window, &mut access).expect_err("the trader's own slot");
    assert_eq!(error.code.as_str(), codes::PERMISSION_DENIED);
}

#[test]
fn a_script_that_does_not_compile_never_reaches_the_owner() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let input = json!({ "name": "broken", "source": "plot(" });
    let error = attach(&mut window, &mut access, &FakeWindow::assistant(), &input)
        .expect_err("a syntax error is refused");
    assert!(error.context.details.is_some(), "the diagnostics are data");
    assert!(window.indicators.attached.is_empty());
    assert!(access.events.is_empty());
}

#[test]
fn nothing_to_detach_journals_nothing() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let detached = detach_slot(&mut window, &mut access).unwrap();
    assert_eq!(detached["detached"], false);
    assert!(access.events.is_empty());
}
