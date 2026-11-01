use quantick_control_host::actions::ActionRegistry;
use quantick_control_host::authority::TRADE_EFFECT_ID;
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

fn docked() -> ActionRegistry<FakeWindow, FakeAccess> {
    let mut registry = ActionRegistry::new();
    register(&mut registry).unwrap();
    registry
}

#[test]
fn the_trade_actions_register_and_validate_their_schemas() {
    let registry = docked();
    for id in [
        PLACE_CAPABILITY_ID,
        BRACKET_CAPABILITY_ID,
        CANCEL_CAPABILITY_ID,
    ] {
        let action = registry
            .lookup(id, CAPABILITY_VERSION)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(action.descriptor.effect.as_str(), TRADE_EFFECT_ID);
        assert!(!action.descriptor.read_only, "{id} changes the account");
        assert!(
            action
                .descriptor
                .required_permissions
                .iter()
                .any(|permission| permission.as_str() == TRADE_PERMISSION_ID),
            "{id} sits behind the trade permission and nothing softer"
        );
    }

    let place = registry.lookup(PLACE_CAPABILITY_ID, 1).unwrap();
    place
        .input
        .validate(&json!({
            "side": "buy",
            "kind": "limit",
            "quantity": "2",
            "price": "95.5",
            "stop_loss": "90"
        }))
        .unwrap();
    assert!(
        place
            .input
            .validate(&json!({ "side": "buy", "kind": "limit" }))
            .is_err(),
        "quantity is not optional"
    );
    assert!(
        place
            .input
            .validate(&json!({
                "side": "buy",
                "kind": "limit",
                "quantity": "1",
                "price": 95.5
            }))
            .is_err(),
        "a price is a decimal string, never a JSON double"
    );
    assert!(
        place
            .input
            .validate(&json!({ "side": "sideways", "kind": "limit", "quantity": "1" }))
            .is_err()
    );
}

/// The one thing that must stay true while no profile may trade: the
/// permission is real, and it is not the annotate tier's.
#[test]
fn trading_never_borrows_the_annotate_permission() {
    let registry = docked();
    let cancel = registry.lookup(CANCEL_CAPABILITY_ID, 1).unwrap();
    assert!(
        !cancel.descriptor.destructive,
        "a working order is an instruction, not the trader's work"
    );
    assert!(
        cancel.descriptor.risk_reducing,
        "and taking one off the book can only ever reduce exposure"
    );
    assert!(
        !cancel
            .descriptor
            .required_permissions
            .iter()
            .any(|permission| permission.as_str().starts_with("annotate")),
        "annotate promises it never affects a position; a trade cannot borrow it"
    );
}

/// Without a chart open there is no desk, and every trade action says so
/// in the words the ticket uses.
#[test]
fn a_trade_without_a_chart_open_is_refused_before_it_journals() {
    let mut window = FakeWindow::new(Vec::new());
    let mut access = FakeAccess::with_budget(0);
    let refusal = cancel_order(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &json!({ "order_id": 7 }),
    )
    .expect_err("no desk");
    assert_eq!(refusal, no_chart_open());
    assert!(access.events.is_empty(), "a refused call before the desk records nothing");
}
