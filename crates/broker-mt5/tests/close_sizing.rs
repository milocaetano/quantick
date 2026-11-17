//! A close is sized against what is already on its way out: two clicks on
//! Close never send more than the open quantity in total, because on a
//! netting account the excess would open a position the other way.

mod support;

use quantick_broker_mt5::machine::CLOSE_PENDING;
use quantick_engine::Side;
use quantick_trading::{
    CloseAmount, FillRole, OrderIntent, RejectReason, TradingVenue, VenueEvent,
};
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

/// A close sent with ORDER_FILLING_RETURN can fill in parts: its first deal
/// takes one contract off and the rest keeps working at the exchange. What
/// still works is still closing, and the order's later deals are still the
/// close's, never an entry's.
#[test]
fn a_partly_filled_close_keeps_its_remainder_closing_until_it_fills() {
    let (mut venue, bridge) = holding(3);
    venue.close(CloseAmount::All);
    let close = closes(&bridge.take_sent()).remove(0);
    assert_eq!(close["volume"], "3");
    bridge.push(&reply(
        close["id"].as_str().unwrap(),
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
    assert_eq!(
        venue.close(CloseAmount::All),
        blocked(CLOSE_PENDING),
        "two open, and the close's remainder of two still works"
    );
    assert!(closes(&bridge.take_sent()).is_empty(), "no second close");

    // A sell entry the broker answered without a ticket is in flight when
    // the close's remainder fills: the deal is the close's, not the entry's.
    let entry = OrderIntent::market(Side::Sell, dec(1));
    let Some(VenueEvent::Placed(placed)) = venue.submit(entry).into_iter().next() else {
        panic!("the entry is placed");
    };
    let submit = support::only(&bridge.take_sent(), "submit");
    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10009,
        "Request executed",
        0,
    ));
    bridge.push(
        &Snapshot::new()
            .deal(support::deal(
                951, 900, 77, "sell", "out", "expert", 2, 125_105,
            ))
            .line(),
    );
    let events = venue.poll();
    let roles: Vec<FillRole> = events
        .iter()
        .filter_map(|event| match event {
            VenueEvent::Filled(fill) => Some(fill.role),
            _ => None,
        })
        .collect();
    assert_eq!(roles, vec![FillRole::Close], "{events:?}");
    let mut in_flight = Vec::new();
    venue.in_flight_entries(&mut in_flight);
    assert_eq!(
        in_flight,
        vec![placed.id],
        "the entry still waits for its own fill"
    );
    assert_eq!(
        venue.in_flight(),
        1,
        "the close is done; only the entry is in flight"
    );
}

/// A close that filled in part and then ended unfilled frees what it never
/// took off, and says how much that was.
#[test]
fn a_partly_filled_close_that_ends_frees_its_remainder() {
    let (mut venue, bridge) = holding(3);
    venue.close(CloseAmount::All);
    let close = closes(&bridge.take_sent()).remove(0);
    bridge.push(&reply(
        close["id"].as_str().unwrap(),
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
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 2, 125_000, MAGIC))
            .closed(900, "canceled")
            .line(),
    );
    let events = venue.poll();
    assert!(
        events.iter().any(|event| matches!(
            event,
            VenueEvent::Rejected(RejectReason::Venue { message, .. })
                if message.to_string() == "close order 900 ended canceled with 2 unfilled"
        )),
        "{events:?}"
    );
    assert!(venue.close(CloseAmount::All).is_empty());
    assert_eq!(closes(&bridge.take_sent())[0]["volume"], "2");
}
