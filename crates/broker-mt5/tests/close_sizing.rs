//! A close is sized against what is already on its way out: two clicks on
//! Close never send more than the open quantity in total, because on a
//! netting account the excess would open a position the other way.

mod support;

use quantick_broker_mt5::machine::CLOSE_PENDING;
use quantick_trading::{CloseAmount, RejectReason, TradingVenue, VenueEvent};
use serde_json::Value;
use support::{MAGIC, Snapshot, connected, dec, order, position, reply};

fn blocked(sentence: &'static str) -> Vec<VenueEvent> {
    vec![VenueEvent::Rejected(RejectReason::Blocked(sentence))]
}

fn closes(sent: &[Value]) -> Vec<Value> {
    sent.iter()
        .filter(|line| line["type"] == "close")
        .cloned()
        .collect()
}

fn holding(
    quantity: i64,
) -> (
    quantick_broker_mt5::Mt5Venue,
    quantick_broker_mt5::fake::FakeBridgeHandle,
) {
    let (mut venue, bridge) = connected();
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", quantity, 125_000, MAGIC))
            .line(),
    );
    venue.poll();
    bridge.take_sent();
    (venue, bridge)
}

#[test]
fn a_second_close_while_the_first_is_unanswered_is_refused() {
    let (mut venue, bridge) = holding(2);
    assert!(venue.close(CloseAmount::All).is_empty());
    assert_eq!(
        venue.close(CloseAmount::All),
        blocked(CLOSE_PENDING),
        "the whole position is already closing"
    );
    let sent = closes(&bridge.take_sent());
    assert_eq!(sent.len(), 1, "one close reached the bridge: {sent:?}");
    assert_eq!(sent[0]["volume"], "2");

    // Accepted, its deal not in yet: still closing.
    bridge.push(&reply(
        sent[0]["id"].as_str().unwrap(),
        10009,
        "Request executed",
        900,
    ));
    venue.poll();
    assert_eq!(venue.close(CloseAmount::All), blocked(CLOSE_PENDING));
    assert!(closes(&bridge.take_sent()).is_empty());
}

#[test]
fn a_flatten_during_a_close_cancels_the_orders_and_closes_nothing_more() {
    let (mut venue, bridge) = connected();
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 2, 125_000, MAGIC))
            .order(order(601, "sell", "limit", 1, 125_400, MAGIC))
            .line(),
    );
    venue.poll();
    bridge.take_sent();
    assert!(venue.close(CloseAmount::All).is_empty());
    assert!(venue.flatten().is_empty());
    let sent = bridge.take_sent();
    assert_eq!(closes(&sent).len(), 1, "never a second close: {sent:?}");
    assert!(
        sent.iter()
            .any(|line| line["type"] == "cancel" && line["order"] == 601),
        "the working order is still cancelled: {sent:?}"
    );
}

#[test]
fn partial_closes_are_sized_against_what_is_already_closing() {
    let (mut venue, bridge) = holding(3);
    assert!(venue.close(CloseAmount::Partial(dec(2))).is_empty());
    assert!(venue.close(CloseAmount::Partial(dec(2))).is_empty());
    assert_eq!(venue.close(CloseAmount::All), blocked(CLOSE_PENDING));
    let volumes: Vec<Value> = closes(&bridge.take_sent())
        .iter()
        .map(|line| line["volume"].clone())
        .collect();
    assert_eq!(volumes, ["2", "1"], "3 open, 3 closing, never more");
}

#[test]
fn a_refused_close_frees_its_quantity_for_the_next_one() {
    let (mut venue, bridge) = holding(2);
    venue.close(CloseAmount::All);
    let first = closes(&bridge.take_sent()).remove(0);
    bridge.push(&reply(first["id"].as_str().unwrap(), 10019, "No money", 0));
    venue.poll();
    assert!(venue.close(CloseAmount::All).is_empty());
    let again = closes(&bridge.take_sent());
    assert_eq!(again.len(), 1);
    assert_eq!(again[0]["volume"], "2");
}

#[test]
fn a_filled_close_leaves_the_rest_closable() {
    let (mut venue, bridge) = holding(3);
    venue.close(CloseAmount::Partial(dec(1)));
    let first = closes(&bridge.take_sent()).remove(0);
    bridge.push(&reply(
        first["id"].as_str().unwrap(),
        10009,
        "Request executed",
        900,
    ));
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 2, 125_000, MAGIC))
            .deal(support::deal(
                950, 900, 77, "sell", "out", "expert", 1, 125_100,
            ))
            .line(),
    );
    venue.poll();
    assert!(venue.close(CloseAmount::All).is_empty());
    let rest = closes(&bridge.take_sent());
    assert_eq!(rest[0]["volume"], "2", "the two still open, once");
}
