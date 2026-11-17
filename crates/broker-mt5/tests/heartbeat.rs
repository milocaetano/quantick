//! A bridge that hangs keeps its socket open and says nothing: the state
//! heartbeat is what tells it from a quiet account. The venue counts the
//! silence only against times it is handed, and declares the link lost after
//! missed heartbeats.

mod support;

use quantick_broker_mt5::LinkState;
use quantick_broker_mt5::machine::{LINK_DOWN, NO_CLOCK, STALE_AFTER_MS};
use quantick_engine::Side;
use quantick_trading::{
    CloseAmount, OrderIntent, RejectReason, TradingVenue, VenueEvent, VenueNotice,
};
use support::{MAGIC, Snapshot, connected, dec, notices, only, position, reply};

#[test]
fn a_bridge_silent_for_missed_heartbeats_is_declared_lost() {
    let (mut venue, bridge) = connected();
    bridge.push(&Snapshot::new().line());
    venue.poll();
    assert!(venue.tick(10_000).is_empty(), "heard just now");
    bridge.push(&Snapshot::new().line());
    venue.poll();
    assert!(venue.tick(12_000).is_empty());
    assert!(venue.tick(12_000 + STALE_AFTER_MS - 1).is_empty());

    let events = venue.tick(12_000 + STALE_AFTER_MS);
    assert_eq!(notices(&events), vec![VenueNotice::LinkLost]);
    assert_eq!(venue.link_state(), LinkState::Lost);
    assert!(venue.is_stale());
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(1))),
        vec![VenueEvent::Rejected(RejectReason::Blocked(LINK_DOWN))]
    );
    assert!(
        venue.tick(60_000).is_empty(),
        "lost is said once, not every frame"
    );

    // The bridge wakes up on the same socket: its next line restores the
    // link, and the state after it makes the reads current again.
    bridge.push(&Snapshot::new().line());
    let events = venue.poll();
    assert_eq!(notices(&events), vec![VenueNotice::LinkRestored]);
    assert_eq!(venue.link_state(), LinkState::Up);
    assert!(!venue.is_stale());
}

#[test]
fn a_quiet_account_that_keeps_its_heartbeat_stays_up() {
    let (mut venue, bridge) = connected();
    for beat in 0..20_i64 {
        bridge.push(&Snapshot::new().line());
        venue.poll();
        assert!(venue.tick(beat * 2_000).is_empty(), "beat {beat}");
    }
    assert_eq!(venue.link_state(), LinkState::Up);
}

#[test]
fn silence_before_any_bridge_dialed_is_not_a_lost_link() {
    let (mut venue, _bridge) = support::venue();
    assert!(venue.tick(0).is_empty());
    assert!(venue.tick(10 * STALE_AFTER_MS).is_empty());
    assert_eq!(venue.link_state(), LinkState::NeverConnected);
}

/// A link declared lost for its silence still has its socket, and the hung
/// bridge on the other end may wake and carry out what it was sent. Nothing
/// is waited for past the loss: every unanswered request's outcome is
/// unknown from then on, and its late answer changes nothing.
#[test]
fn a_silence_declared_loss_abandons_every_unanswered_request() {
    let (mut venue, bridge) = connected();
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 2, 125_000, MAGIC))
            .line(),
    );
    venue.poll();
    assert!(venue.tick(1_000).is_empty());
    let Some(VenueEvent::Placed(placed)) = venue
        .submit(OrderIntent::market(Side::Buy, dec(1)))
        .into_iter()
        .next()
    else {
        panic!("the entry is placed");
    };
    assert!(venue.close(CloseAmount::Partial(dec(1))).is_empty());
    let submit = only(&bridge.take_sent(), "submit");
    assert_eq!(venue.in_flight(), 2);

    let events = venue.tick(1_000 + STALE_AFTER_MS);
    assert_eq!(
        notices(&events),
        vec![
            VenueNotice::LinkLost,
            VenueNotice::OutcomeUnknown {
                id: Some(placed.id)
            },
            VenueNotice::OutcomeUnknown { id: None },
        ]
    );
    assert_eq!(venue.in_flight(), 0, "nothing is waited for any more");

    // The bridge wakes on the same socket and answers the old submit.
    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10009,
        "Request executed",
        501,
    ));
    let events = venue.poll();
    assert_eq!(notices(&events), vec![VenueNotice::LinkRestored]);
    assert_eq!(
        venue.ticket(placed.id),
        None,
        "the late answer binds nothing"
    );
}

/// Every order request carries the moment the link could first be declared
/// lost; the bridge executes nothing at or past it, so a request can never
/// run after the venue stopped waiting for it.
#[test]
fn every_order_request_expires_when_the_link_could_first_be_declared_lost() {
    let (mut venue, bridge) = connected();
    venue.submit(OrderIntent::market(Side::Buy, dec(1)));
    let first = only(&bridge.take_sent(), "submit");
    assert_eq!(
        first["expires_ms"], STALE_AFTER_MS,
        "alive at 0, nothing heard since"
    );

    assert!(venue.tick(1_500).is_empty());
    bridge.push(&Snapshot::new().line());
    venue.poll();
    venue.submit(OrderIntent::market(Side::Buy, dec(1)));
    let second = only(&bridge.take_sent(), "submit");
    assert_eq!(
        second["expires_ms"],
        1_500 + STALE_AFTER_MS,
        "heard since the clock read 1500: alive from then"
    );

    venue.set_symbol("WDOZ26");
    let watch = only(&bridge.take_sent(), "watch");
    assert!(watch.get("expires_ms").is_none(), "a watch trades nothing");
}

#[test]
fn nothing_reaches_the_bridge_before_the_venue_is_handed_the_time() {
    let (mut venue, bridge) = support::venue();
    bridge.connect("bridge-1");
    venue.poll();
    bridge.push(&Snapshot::new().line());
    venue.poll();
    bridge.take_sent();
    assert_eq!(
        venue.submit(OrderIntent::market(Side::Buy, dec(1))),
        vec![VenueEvent::Rejected(RejectReason::Blocked(NO_CLOCK))]
    );
    assert!(bridge.take_sent().is_empty());
    assert!(venue.tick(0).is_empty());
    assert_eq!(venue.core().write_block(), None);
}
