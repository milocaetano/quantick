//! A bridge that hangs keeps its socket open and says nothing: the state
//! heartbeat is what tells it from a quiet account. The venue counts the
//! silence only against times it is handed, and declares the link lost after
//! missed heartbeats.

mod support;

use quantick_broker_mt5::LinkState;
use quantick_broker_mt5::machine::{LINK_DOWN, STALE_AFTER_MS};
use quantick_engine::Side;
use quantick_trading::{OrderIntent, RejectReason, TradingVenue, VenueEvent, VenueNotice};
use support::{Snapshot, connected, dec, notices};

#[test]
fn a_bridge_silent_for_missed_heartbeats_is_declared_lost() {
    let (mut venue, bridge) = connected();
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
