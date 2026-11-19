//! The connection state on the wire, and when it offers Reconnect.

use super::*;

/// With no clock nothing time-bound travels, or the revision key would move
/// on every frame.
#[test]
fn nothing_time_bound_travels_without_a_clock() {
    let snapshot = snapshot(&ConnectionTracker::new(), None, None);
    assert_eq!(snapshot.link, "attempting");
    assert_eq!(snapshot.status_word, None);
    assert!(!snapshot.overlay_up && !snapshot.reconnect_offered);
}

/// Late on a link that is up, the tape cell offers Reconnect, and the
/// wire says so; offline offers its own, and a pause on a link that is
/// down adds none.
#[test]
fn a_late_link_that_is_up_offers_reconnect_on_the_wire() {
    use quantick_sources::sync::{LinkState, SyncLimits, SyncReading};
    let late = Desync {
        reading: SyncReading::Latency,
        link: LinkState::Up,
        latency_ms: Some(900),
        latency_hop: Some("MT5"),
        tape_age_ms: None,
        limits: SyncLimits::default(),
        notice: None,
    };
    let attempting = Desync {
        reading: SyncReading::Link,
        link: LinkState::Attempting,
        ..late.clone()
    };
    assert!(reconnect_offered(Link::Live, Some(&late)));
    assert!(!reconnect_offered(Link::Attempting, Some(&attempting)));
    assert!(!reconnect_offered(Link::Live, None));
    assert!(reconnect_offered(Link::Offline, None));
    let wire = snapshot(&ConnectionTracker::new(), Some(&late), Some(0));
    assert!(wire.reconnect_offered);
    let no_clock = snapshot(&ConnectionTracker::new(), Some(&late), None);
    assert!(
        !no_clock.reconnect_offered,
        "nothing time-bound without a clock"
    );
}
