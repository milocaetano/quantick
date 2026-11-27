//! The entry pause's words and the limits a feed may set.

use super::*;

fn desync(reading: SyncReading) -> Desync {
    Desync {
        reading,
        link: LinkState::Up,
        latency_ms: Some(900),
        latency_hop: Some("MT5"),
        latency_at_source: false,
        tape_age_ms: Some(7_250),
        limits: SyncLimits::default(),
        notice: None,
    }
}

#[test]
fn each_sentence_names_its_reading_value_and_limit() {
    let latency = desync(SyncReading::Latency).sentence();
    assert!(
        latency.contains("prints arrive 900 ms late (limit 500 ms), most of it in MT5"),
        "{latency}"
    );
    let age = desync(SyncReading::TapeAge).sentence();
    assert!(
        age.contains("nothing new has arrived for 7.2 s (limit 5.0 s)"),
        "{age}"
    );
    assert!(desync(SyncReading::Latency).offers_reconnect());
    assert!(
        latency.ends_with("Reconnect sits beside the tape"),
        "{latency}"
    );
    let offline = Desync {
        link: LinkState::Offline,
        notice: Some("MetaTrader 5 disconnected".to_owned()),
        ..desync(SyncReading::Link)
    };
    let said = offline.sentence();
    assert!(
        said.contains("the feed is offline - MetaTrader 5 disconnected. Orders that"),
        "{said}"
    );
    assert!(
        !said.contains("Reconnect"),
        "offline offers its own: {said}"
    );
    assert!(!offline.offers_reconnect(), "offline offers its own");
}

/// A new connection reads the same late source, so a pause for latency the
/// source owns offers no Reconnect; a late wire, and every other reading on a
/// link that is up, still does.
#[test]
fn latency_the_source_owns_offers_no_reconnect() {
    for (reading, at_source, offered) in [
        (SyncReading::Latency, true, false),
        (SyncReading::Latency, false, true),
        (SyncReading::TapeAge, true, true),
    ] {
        let gate = Desync {
            latency_at_source: at_source,
            ..desync(reading)
        };
        let row = format!("{reading:?}, at source {at_source}");
        assert_eq!(gate.offers_reconnect(), offered, "{row}");
        assert_eq!(gate.sentence().contains("Reconnect"), offered, "{row}");
    }
}

#[test]
fn a_limit_under_a_millisecond_is_refused_at_load() {
    assert_eq!(SyncLimits::default().validate("mt5"), Ok(()));
    let zero = SyncLimits {
        max_tape_age_ms: 0,
        ..SyncLimits::default()
    };
    let refused = zero.validate("mt5").expect_err("zero pauses everything");
    assert!(refused.contains("sync.max_tape_age_ms"), "{refused}");
}
