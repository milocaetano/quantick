//! The entry pause's judgement and the watch that logs its edges.

use super::*;

fn split(arrival: i64) -> FeedLatency {
    FeedLatency {
        arrival_lag_ms: arrival,
        source_lag_ms: Some(arrival - 100),
        source_lag_peak_ms: None,
        transport_lag_ms: Some(100),
        hop: Some("MT5"),
        prints: 3,
    }
}

fn input(notice: &FeedNotice) -> SyncInput<'_> {
    SyncInput {
        replaying: false,
        link: LinkState::Up,
        link_for_ms: 0,
        latency: Some(split(120)),
        tape_age_ms: Some(300),
        notice,
        limits: SyncLimits::default(),
    }
}

fn reading(input: &SyncInput<'_>) -> Option<SyncReading> {
    assess(input).map(|desync| desync.reading)
}

#[test]
fn each_limit_pauses_at_its_value_and_not_a_millisecond_before() {
    let clear = FeedNotice::Clear;
    assert_eq!(reading(&input(&clear)), None, "a chart that keeps up");
    for (latency, expected) in [
        (DEFAULT_MAX_LATENCY_MS - 1, None),
        (DEFAULT_MAX_LATENCY_MS, Some(SyncReading::Latency)),
    ] {
        let at = SyncInput {
            latency: Some(split(latency)),
            ..input(&clear)
        };
        assert_eq!(reading(&at), expected, "latency {latency}");
    }
    for (age, expected) in [
        (DEFAULT_MAX_TAPE_AGE_MS - 1, None),
        (DEFAULT_MAX_TAPE_AGE_MS, Some(SyncReading::TapeAge)),
    ] {
        let at = SyncInput {
            tape_age_ms: Some(age),
            ..input(&clear)
        };
        assert_eq!(reading(&at), expected, "tape age {age}");
    }
    let limits = SyncLimits {
        max_latency_ms: 2_000,
        max_tape_age_ms: 60_000,
        max_connecting_ms: 3_000,
    };
    let loose = SyncInput {
        latency: Some(split(1_999)),
        tape_age_ms: Some(59_999),
        limits,
        ..input(&clear)
    };
    assert_eq!(reading(&loose), None, "the feed config's limits rule");
}

#[test]
fn a_link_that_is_not_up_pauses_and_outranks_the_other_readings() {
    let notice = FeedNotice::reconnecting("MetaTrader 5 disconnected");
    for link in [LinkState::Attempting, LinkState::Offline] {
        let down = SyncInput {
            link,
            latency: Some(split(900)),
            tape_age_ms: Some(9_000),
            ..input(&notice)
        };
        let desync = assess(&down).expect("a link that is not up");
        assert_eq!(desync.reading, SyncReading::Link);
        assert_eq!(desync.link, link);
        assert_eq!(desync.latency_ms, Some(900), "every reading is kept");
        assert_eq!(desync.latency_hop, Some("MT5"));
        assert_eq!(desync.tape_age_ms, Some(9_000));
        assert_eq!(desync.notice.as_deref(), Some("MetaTrader 5 disconnected"));
    }
}

/// A connect may run for the feed's `max_connecting_ms` before entries
/// pause; an offline link pauses whatever the limit.
#[test]
fn a_connect_pauses_once_its_limit_runs_out() {
    let clear = FeedNotice::Clear;
    let limits = SyncLimits {
        max_connecting_ms: 2_000,
        ..SyncLimits::default()
    };
    for (link, link_for_ms, expected) in [
        (LinkState::Attempting, 1_999, None),
        (LinkState::Attempting, 2_000, Some(SyncReading::Link)),
        (LinkState::Offline, 0, Some(SyncReading::Link)),
    ] {
        let at = SyncInput {
            link,
            link_for_ms,
            limits,
            ..input(&clear)
        };
        assert_eq!(reading(&at), expected, "{link:?} for {link_for_ms}");
    }
}

#[test]
fn a_replay_is_never_paused() {
    let clear = FeedNotice::Clear;
    let replay = SyncInput {
        replaying: true,
        link: LinkState::Offline,
        latency: Some(split(90_000)),
        tape_age_ms: Some(3_600_000),
        ..input(&clear)
    };
    assert_eq!(assess(&replay), None);
}

/// The tape cell's rule: an unsplit or negative figure is a clock
/// difference, and a clock difference pauses nothing.
#[test]
fn a_clock_difference_is_not_latency() {
    let clear = FeedNotice::Clear;
    let unsplit = FeedLatency {
        source_lag_ms: None,
        transport_lag_ms: None,
        ..split(9_000)
    };
    for latency in [unsplit, split(-9_000)] {
        let at = SyncInput {
            latency: Some(latency),
            ..input(&clear)
        };
        assert_eq!(reading(&at), None, "{latency:?}");
    }
}

#[test]
fn the_watch_reports_each_edge_once_with_the_pause_length() {
    let mut watch = SyncWatch::default();
    assert_eq!(watch.step(false, false, 0), None);
    assert_eq!(watch.step(true, false, 1_000), Some(SyncChange::Entered));
    assert_eq!(watch.step(true, false, 2_000), None, "still paused");
    assert_eq!(
        watch.step(false, false, 4_500),
        Some(SyncChange::Left { paused_ms: 3_500 })
    );
    assert_eq!(watch.step(false, false, 5_000), None);
}

/// Every launch starts paused on its first connect: the log says nothing
/// of one that ends in time, and says one that outlasts the budget or
/// turns into another cause.
#[test]
fn a_first_connect_pause_is_logged_only_once_it_outlasts_the_budget() {
    let mut watch = SyncWatch::default();
    assert_eq!(watch.step(true, true, 0), None);
    assert_eq!(watch.step(false, false, 148), None, "a launch is no news");
    assert_eq!(watch.step(true, true, 1_000), None);
    let late = 1_000 + FIRST_CONNECT_QUIET_MS;
    assert_eq!(watch.step(true, true, late - 1), None);
    assert_eq!(watch.step(true, true, late), Some(SyncChange::Entered));
    assert_eq!(
        watch.step(false, false, late + 5),
        Some(SyncChange::Left {
            paused_ms: FIRST_CONNECT_QUIET_MS + 5
        })
    );
    assert_eq!(watch.step(true, true, 90_000), None);
    assert_eq!(
        watch.step(true, false, 90_100),
        Some(SyncChange::Entered),
        "connected and late is news"
    );
}
