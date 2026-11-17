//! What the broker venue refuses, and what it never does: send twice, touch
//! another program's exposure, trade a hedging account, close on a reset,
//! or pretend a lost link is a live one.

mod support;

use quantick_broker_mt5::LinkState;
use quantick_broker_mt5::machine::{
    FOREIGN_EXPOSURE, LINK_DOWN, NO_ACCOUNT, NO_CANCEL_AT, NO_FLAT_ONLY, NO_LADDER, NOT_NETTING,
};
use quantick_engine::Side;
use quantick_trading::fake::print_at;
use quantick_trading::{
    Bracket, CloseAmount, ExitPart, OrderIntent, RejectReason, TradingVenue, VenueEvent,
    VenueNotice,
};
use support::{
    LOGIN, MAGIC, Snapshot, account, connected, dec, ids, notices, only, order, position, reply,
};

fn blocked(sentence: &'static str) -> Vec<VenueEvent> {
    vec![VenueEvent::Rejected(RejectReason::Blocked(sentence))]
}

#[test]
fn a_request_resent_after_a_dropped_link_carries_the_same_id_and_is_applied_once() {
    let (mut venue, bridge) = connected();
    let id = match venue
        .submit(OrderIntent::market(Side::Buy, dec(1)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let first = only(&bridge.take_sent(), "submit");

    bridge.disconnect();
    venue.poll();
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(1))),
        blocked(LINK_DOWN),
        "nothing new while the link is down"
    );

    // The same bridge process dials back: it remembers its replies, so the
    // unanswered request is asked about again under its own id, never a new
    // one, and only for the answer: a bridge that never received it must not
    // execute it now.
    bridge.connect("bridge-1");
    venue.poll();
    let resent = bridge.take_sent();
    assert_eq!(only(&resent, "submit")["id"], first["id"]);
    let mut asked = only(&resent, "submit");
    assert_eq!(
        asked["replay_only"], true,
        "an answer, never a new execution"
    );
    asked.as_object_mut().unwrap().remove("replay_only");
    assert_eq!(asked, first, "otherwise the very same line");

    // The bridge answers from memory — and a duplicate answer changes nothing.
    let answer = reply(
        first["id"].as_str().unwrap(),
        10009,
        "Request executed",
        501,
    );
    bridge.push(&answer);
    bridge.push(&answer);
    let events = venue.poll();
    assert_eq!(
        notices(&events),
        vec![VenueNotice::Acknowledged { id, venue_ref: 501 }],
        "acknowledged once"
    );
}

#[test]
fn a_request_the_bridge_never_received_is_reported_unknown_not_executed() {
    let (mut venue, bridge) = connected();
    let id = match venue
        .submit(OrderIntent::market(Side::Buy, dec(1)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    // Lost on the way: the link dropped before the bridge read it.
    bridge.take_sent();
    bridge.disconnect();
    venue.poll();
    bridge.connect("bridge-1");
    venue.poll();
    let asked = only(&bridge.take_sent(), "submit");
    assert_eq!(asked["replay_only"], true);

    // The bridge has no answer for it, so it says so and executes nothing.
    bridge.push(&serde_json::json!({
        "type": "reply", "id": asked["id"], "retcode": null, "unknown": true,
        "error": "never received: not executed", "cached": false,
    }));
    let events = venue.poll();
    assert_eq!(
        notices(&events),
        vec![VenueNotice::OutcomeUnknown { id: Some(id) }]
    );
    assert_eq!(venue.in_flight(), 0, "no longer waiting for a fill");
}

#[test]
fn nothing_is_resent_to_a_bridge_that_restarted() {
    let (mut venue, bridge) = connected();
    let id = match venue
        .submit(OrderIntent::limit(Side::Buy, dec(1), dec(124_000)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    bridge.take_sent();
    bridge.disconnect();
    venue.poll();

    bridge.connect("bridge-2");
    let events = venue.poll();
    assert_eq!(
        notices(&events),
        vec![
            VenueNotice::OutcomeUnknown { id: Some(id) },
            VenueNotice::LinkRestored
        ]
    );
    let sent = bridge.take_sent();
    assert!(
        sent.iter().all(|line| line["type"] == "watch"),
        "a new bridge has no memory, so nothing is resent: {sent:?}"
    );
    assert!(
        venue.working_orders().is_empty(),
        "the next state says what exists"
    );

    // If it did reach the broker, the state shows it, and it is adopted.
    bridge.push(
        &Snapshot::new()
            .order(order(640, "buy", "limit", 1, 124_000, MAGIC))
            .line(),
    );
    venue.poll();
    assert_eq!(venue.working_orders().len(), 1);
}

#[test]
fn a_lost_link_keeps_the_last_state_labelled_stale_and_refuses_writes() {
    let (mut venue, bridge) = connected();
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 1, 125_000, MAGIC))
            .line(),
    );
    venue.poll();
    assert!(!venue.is_stale());

    bridge.disconnect();
    let events = venue.poll();
    assert_eq!(notices(&events), vec![VenueNotice::LinkLost]);
    assert_eq!(venue.link_state(), LinkState::Lost);
    assert!(venue.is_stale());
    assert_eq!(
        venue.position().map(|p| p.quantity),
        Some(dec(1)),
        "last known, kept"
    );
    assert_eq!(venue.close(CloseAmount::All), blocked(LINK_DOWN));
    assert_eq!(venue.flatten(), blocked(LINK_DOWN));

    bridge.connect("bridge-1");
    let events = venue.poll();
    assert_eq!(notices(&events), vec![VenueNotice::LinkRestored]);
    assert!(venue.is_stale(), "up, but no fresh state yet");
    bridge.push(&Snapshot::new().line());
    venue.poll();
    assert!(!venue.is_stale());
    assert!(venue.position().is_none(), "the fresh state wins");
}

#[test]
fn a_login_change_is_announced_and_nothing_from_the_old_account_survives() {
    let (mut venue, bridge) = connected();
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 1, 125_000, MAGIC))
            .line(),
    );
    venue.poll();

    bridge.push(
        &Snapshot::new()
            .account(account(LOGIN + 1, "real", "exchange"))
            .line(),
    );
    let events = venue.poll();
    assert_eq!(notices(&events), vec![VenueNotice::AccountChanged]);
    assert!(venue.position().is_none());
    let facts = venue.account().expect("the new account");
    assert_eq!(facts.login, LOGIN + 1);
    assert!(facts.trade_mode.is_real_money());
}

#[test]
fn a_log_out_then_in_as_someone_else_is_still_a_change() {
    let (mut venue, bridge) = connected();
    let mut logged_out = Snapshot::new().line();
    logged_out["account"] = serde_json::Value::Null;
    bridge.push(&logged_out);
    venue.poll();
    assert!(venue.account().is_none());
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(1))),
        blocked(NO_ACCOUNT)
    );
    bridge.push(
        &Snapshot::new()
            .account(account(999, "demo", "netting"))
            .line(),
    );
    assert_eq!(notices(&venue.poll()), vec![VenueNotice::AccountChanged]);
}

#[test]
fn another_programs_exposure_blocks_new_entries_and_is_never_touched() {
    let (mut venue, bridge) = connected();
    venue.seed(&print_at(1, dec(125_000), 1_000));
    bridge.push(
        &Snapshot::new()
            .position(position(88, "sell", 2, 125_100, 0))
            .order(order(650, "buy", "limit", 1, 124_000, 424_242))
            .line(),
    );
    venue.poll();
    let foreign = venue.foreign_exposure();
    assert_eq!(foreign.positions.len(), 1);
    assert_eq!(
        (foreign.positions[0].ticket, foreign.positions[0].magic),
        (88, 0)
    );
    assert_eq!(foreign.orders[0].magic, 424_242);
    assert!(
        venue.position().is_none(),
        "someone else's position is not ours"
    );
    assert!(venue.working_orders().is_empty());
    assert_eq!(venue.entry_block(), Some(FOREIGN_EXPOSURE));

    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(1))),
        blocked(FOREIGN_EXPOSURE)
    );
    assert_eq!(
        venue.submit(OrderIntent::limit(Side::Sell, dec(1), dec(125_500))),
        blocked(FOREIGN_EXPOSURE)
    );
    assert!(
        venue.close(CloseAmount::All)[0] == VenueEvent::Rejected(RejectReason::NoPosition),
        "there is nothing of ours to close"
    );
    assert!(venue.flatten().is_empty());
    assert!(
        bridge.take_sent().is_empty(),
        "not one request reached the bridge"
    );

    bridge.push(&Snapshot::new().line());
    venue.poll();
    assert_eq!(venue.entry_block(), None, "gone, unblocked");
}

#[test]
fn a_hedging_account_is_refused_at_connect_with_the_reason() {
    let (mut venue, bridge) = support::venue();
    bridge.connect("bridge-1");
    bridge.push(
        &Snapshot::new()
            .account(account(LOGIN, "real", "hedging"))
            .line(),
    );
    let events = venue.poll();
    assert_eq!(
        notices(&events),
        vec![VenueNotice::AccountRefused(NOT_NETTING)]
    );
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(1))),
        blocked(NOT_NETTING)
    );
    // Said once, not on every state.
    bridge.push(
        &Snapshot::new()
            .account(account(LOGIN, "real", "hedging"))
            .line(),
    );
    assert!(notices(&venue.poll()).is_empty());
}

#[test]
fn a_reset_and_a_symbol_switch_close_nothing_at_the_broker_and_say_so() {
    let (mut venue, bridge) = connected();
    venue.seed(&print_at(1, dec(125_000), 1_000));
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 1, 125_000, MAGIC))
            .order(order(660, "sell", "limit", 1, 125_400, MAGIC))
            .line(),
    );
    venue.poll();

    let events = venue.reset();
    assert_eq!(
        events,
        vec![VenueEvent::Notice(VenueNotice::ResetLeftOpen {
            working: 1,
            position: true
        })]
    );
    assert!(bridge.take_sent().is_empty(), "no cancel, no close");
    assert!(venue.mark_price().is_none(), "the chart's timeline is gone");
    assert!(venue.position().is_some(), "the account's position is not");

    let events = venue.set_symbol("WDOZ26");
    assert_eq!(notices(&events).len(), 1);
    let sent = bridge.take_sent();
    assert_eq!(ids(&sent).len(), 1);
    assert_eq!(only(&sent, "watch")["symbol"], "WDOZ26");
    assert!(venue.position().is_none() && venue.working_orders().is_empty());
    assert!(venue.is_stale(), "until the new symbol's state arrives");
}

#[test]
fn what_the_broker_cannot_hold_natively_is_refused_not_emulated() {
    let (mut venue, bridge) = connected();
    let ladder = Bracket::ladder(&[
        ExitPart {
            quantity: Some(dec(1)),
            stop_loss: Some(dec(124_800)),
            take_profit: None,
        },
        ExitPart {
            quantity: Some(dec(1)),
            stop_loss: Some(dec(124_800)),
            take_profit: Some(dec(125_400)),
        },
    ])
    .expect("a valid ladder");
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(2)).with_bracket(ladder)),
        blocked(NO_LADDER)
    );
    assert_eq!(
        venue.submit(
            OrderIntent::limit(Side::Buy, dec(1), dec(124_900)).with_cancel_at(Some(dec(125_200)))
        ),
        blocked(NO_CANCEL_AT)
    );
    assert_eq!(
        venue.submit(OrderIntent::limit(Side::Buy, dec(1), dec(124_900)).only_when_flat()),
        blocked(NO_FLAT_ONLY)
    );
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(0))),
        vec![VenueEvent::Rejected(RejectReason::QuantityNotPositive)]
    );
    assert!(bridge.take_sent().is_empty());
}

#[test]
fn an_unreadable_line_is_counted_and_changes_nothing() {
    let (mut venue, bridge) = connected();
    bridge.push_raw("{not json");
    bridge.push_raw("{\"type\":\"shout\"}");
    assert!(venue.poll().is_empty());
    assert_eq!(venue.core().malformed_lines(), 2);
}

#[test]
fn a_bridge_of_another_protocol_version_is_never_written_to() {
    let (mut venue, bridge) = support::venue();
    bridge.push(&serde_json::json!({"type": "exec_hello", "schema": 99, "instance": "x"}));
    venue.poll();
    assert!(bridge.take_sent().is_empty());
    assert!(venue.entry_block().is_some());
}
