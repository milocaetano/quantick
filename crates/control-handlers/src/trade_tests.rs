//! The family's wiring only: each call docks under the trade permission,
//! decodes the owner's input, turns the tab's refusal into the right error,
//! and journals who asked. What a call states and what the desk answers are
//! `quantick_session::paper`'s tests (`paper_wire_tests.rs`, `paper_gate_tests.rs`).

use quantick_control::wire::ActorKind;
use quantick_control_host::actions::ActionRegistry;
use quantick_control_host::authority::TRADE_EFFECT_ID;
use quantick_sources::sync::{Desync, LinkState, SyncLimits, SyncReading};
use serde_json::json;

use super::*;
use crate::test_support::{DeskWindow, FakeAccess, FakeWindow};

/// A desk marked at 100 on a symbol, journaling under this source file,
/// where no directory can be made, so nothing can land on disk.
fn desk_window() -> DeskWindow {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/trade_tests.rs/trades");
    let mut account = PaperAccount::with_trades_dir(dir);
    account.set_symbol("TRADEX");
    account.seed(&quantick_engine::Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: rust_decimal::Decimal::from(100),
        quantity: rust_decimal::Decimal::ONE,
        side: quantick_engine::Side::Buy,
    });
    DeskWindow::new(account)
}

#[test]
fn every_trade_call_docks_decodes_refuses_and_journals_through_one_adapter() {
    let mut registry = ActionRegistry::<DeskWindow, FakeAccess>::new();
    register(&mut registry).unwrap();
    let ids = [
        PLACE_CAPABILITY_ID,
        BRACKET_CAPABILITY_ID,
        CANCEL_CAPABILITY_ID,
        FLATTEN_CAPABILITY_ID,
        REVERSE_CAPABILITY_ID,
        SELECT_STRATEGY_CAPABILITY_ID,
        SET_RULER_CAPABILITY_ID,
        SET_RISK_CAPABILITY_ID,
        SET_INSTRUMENT_MONEY_CAPABILITY_ID,
    ];
    for id in ids {
        let action = registry.lookup(id, CAPABILITY_VERSION).expect(id);
        let descriptor = &action.descriptor;
        assert_eq!(descriptor.effect.as_str(), TRADE_EFFECT_ID, "{id}");
        assert!(!descriptor.read_only && !descriptor.destructive, "{id}");
        let permissions: Vec<_> = descriptor
            .required_permissions
            .iter()
            .map(|permission| permission.as_str())
            .collect();
        assert_eq!(permissions, [TRADE_PERMISSION_ID], "{id}: never annotate's");
    }
    let action = |id| registry.lookup(id, CAPABILITY_VERSION).unwrap();
    assert!(action(CANCEL_CAPABILITY_ID).descriptor.risk_reducing);
    let place = action(PLACE_CAPABILITY_ID);
    let priced = json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": 95.5 });
    assert!(
        place.input.validate(&priced).is_err(),
        "a price is a decimal string"
    );
    let no_quantity = json!({ "side": "buy", "kind": "limit" });
    assert!(
        place.input.validate(&no_quantity).is_err(),
        "quantity is required"
    );

    let trader = FakeWindow::trader();
    let mut access = FakeAccess::with_budget(0);
    let mut window = desk_window();
    let run = |window: &mut DeskWindow, access: &mut FakeAccess, id, input| {
        let action = action(id);
        let answer = (action.handler)(window, access, &trader, &input);
        if let Ok(value) = &answer {
            action
                .output
                .validate(value)
                .expect("the answer is the published result");
        }
        answer
    };

    // Nothing reaches the venue or the journal while the tab refuses.
    window.desk.set_trade_gate(Some(Desync {
        reading: SyncReading::Link,
        link: LinkState::Offline,
        latency_ms: None,
        latency_hop: None,
        latency_at_source: false,
        tape_age_ms: Some(12_000),
        limits: SyncLimits::default(),
        notice: None,
    }));
    let market = json!({ "side": "buy", "kind": "market", "quantity": "1" });
    let paused = run(
        &mut window,
        &mut access,
        PLACE_CAPABILITY_ID,
        market.clone(),
    )
    .unwrap_err();
    assert_eq!(
        paused.code.as_str(),
        quantick_control::error::codes::CAPABILITY_UNAVAILABLE
    );
    assert!(paused.retryable, "nothing was sent, so a retry is safe");
    assert!(paused.message.contains("the feed is offline"), "{paused:?}");
    let details = paused.context.details.expect("the reading is named");
    assert_eq!(details["trade_gate"], "link");
    assert_eq!(details["link"], "offline");
    window.desk.set_trade_gate(None);
    let flat = run(&mut window, &mut access, REVERSE_CAPABILITY_ID, json!({})).unwrap_err();
    assert_eq!(
        flat,
        ControlError::invalid_request(quantick_session::paper::NOTHING_TO_REVERSE.to_owned())
    );
    let unstated = json!({ "side": "buy", "kind": "stop", "quantity": "1" });
    let unstated = run(&mut window, &mut access, PLACE_CAPABILITY_ID, unstated).unwrap_err();
    assert_eq!(
        unstated,
        ControlError::invalid_request("a limit or stop needs the price it rests at".to_owned())
    );
    assert!(access.events.is_empty(), "a refusal journals nothing");

    // An accepted call journals the actor, what was asked and the answer.
    let placed = run(&mut window, &mut access, PLACE_CAPABILITY_ID, market).unwrap();
    assert_eq!(placed["accepted"], true, "{placed}");
    let event = access.events.last().expect("journaled");
    assert_eq!(event.kind.as_str(), PLACE_EVENT_KIND);
    assert_eq!(
        event.actor.as_ref().map(|actor| actor.kind),
        Some(ActorKind::HumanUi)
    );
    assert_eq!(event.payload["asked"]["kind"], "market");
    assert_eq!(event.payload["order_id"], placed["order_id"]);
    assert_eq!(event.payload["simulated"], true);
    // The venue's refusal is an answer, not an error, and journals too.
    let through = json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": "101" });
    let refused = run(&mut window, &mut access, PLACE_CAPABILITY_ID, through).unwrap();
    assert_eq!(refused["accepted"], false, "{refused}");
    let event = access.events.last().expect("journaled");
    assert_eq!(event.kind.as_str(), PLACE_EVENT_KIND);
    assert_eq!(event.payload["accepted"], false);
    assert!(
        event.payload["rejected_because"]
            .as_str()
            .is_some_and(|reason| reason.contains("fill immediately")),
        "{}",
        event.payload
    );
    run(&mut window, &mut access, FLATTEN_CAPABILITY_ID, json!({})).unwrap();
    assert_eq!(
        access.events.last().unwrap().kind.as_str(),
        FLATTEN_EVENT_KIND
    );

    // A ticket edit persists the one shared setting it changed; the ruler
    // and an order persist nothing.
    for (id, input) in [
        (SELECT_STRATEGY_CAPABILITY_ID, json!({})),
        (SET_RISK_CAPABILITY_ID, json!({ "basis": "off" })),
        (SET_RULER_CAPABILITY_ID, json!({ "ticks": 4 })),
    ] {
        run(&mut window, &mut access, id, input).expect(id);
    }
    assert_eq!(
        window.persisted,
        [SharedChange::OrderStrategies, SharedChange::RiskSettings]
    );

    // Without a chart open there is no desk, and nothing is journaled.
    let mut registry = ActionRegistry::<FakeWindow, FakeAccess>::new();
    register(&mut registry).unwrap();
    let cancel = registry
        .lookup(CANCEL_CAPABILITY_ID, CAPABILITY_VERSION)
        .unwrap();
    let mut access = FakeAccess::with_budget(0);
    let refusal = (cancel.handler)(
        &mut FakeWindow::new(Vec::new()),
        &mut access,
        &trader,
        &json!({ "order_id": 7 }),
    );
    assert_eq!(refusal, Err(no_chart_open()));
    assert!(access.events.is_empty());
}
