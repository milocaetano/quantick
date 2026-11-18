//! Whether the chart is close enough to its feed for a new entry.
//!
//! A trader sizes an entry from the tape on screen. When that tape is late —
//! the link down or still trying, prints arriving past the latency limit, or
//! nothing new for longer than the tape-age limit — the chart is showing a
//! market that has moved on, and an order sent from it is sent blind. So the
//! tab's paper desk pauses new entries while this judgement holds, on the
//! simulator and on a broker route alike; what only reduces a position
//! never pauses.
//!
//! Beside [`crate::stall`], and like it a pure function told everything: the
//! caller reads the clock and the chart's link, the decision reads neither.
//! The latency it judges is the one the tape cell calls latency
//! ([`FeedLatency::measured_ms`]); a clock difference never pauses anything.

use quantick_operability::intake_lag::IntakeLagSummary;

use crate::{FeedLatency, FeedNotice};

pub use quantick_sources::sync::{
    DEFAULT_MAX_LATENCY_MS, DEFAULT_MAX_TAPE_AGE_MS, Desync, LinkState, SyncLimits, SyncReading,
};

/// Everything the judgement reads, gathered by the caller.
#[derive(Debug, Clone, Copy)]
pub struct SyncInput<'a> {
    /// A recording plays, or the source has no transport: nothing to be
    /// behind of, so never paused.
    pub replaying: bool,
    pub link: LinkState,
    /// How long the transport has been in its current state.
    pub link_for_ms: i64,
    /// The provider's latency split, when it cuts one.
    pub latency: Option<FeedLatency>,
    /// Wall clock minus when the newest print or book event arrived here, by
    /// the local clock; `None` before any.
    pub tape_age_ms: Option<i64>,
    pub notice: &'a FeedNotice,
    pub limits: SyncLimits,
}

/// Why new entries pause, or `None` while the chart keeps up. The link
/// outranks latency, and latency the tape's age: the first reading that
/// trips is the one the refusal names.
#[must_use]
pub fn assess(input: &SyncInput<'_>) -> Option<Desync> {
    if input.replaying {
        return None;
    }
    let limits = input.limits;
    let latency_ms = input.latency.as_ref().and_then(FeedLatency::measured_ms);
    let link_down = match input.link {
        LinkState::Up => false,
        LinkState::Attempting => input.link_for_ms >= limits.max_connecting_ms,
        LinkState::Offline => true,
    };
    let reading = if link_down {
        SyncReading::Link
    } else if latency_ms.is_some_and(|ms| ms >= limits.max_latency_ms) {
        SyncReading::Latency
    } else if input
        .tape_age_ms
        .is_some_and(|ms| ms >= limits.max_tape_age_ms)
    {
        SyncReading::TapeAge
    } else {
        return None;
    };
    Some(Desync {
        reading,
        link: input.link,
        latency_ms,
        latency_hop: input.latency.and_then(|split| split.hop),
        tape_age_ms: input.tape_age_ms,
        limits,
        notice: headline(input.notice).map(str::to_owned),
    })
}

fn headline(notice: &FeedNotice) -> Option<&str> {
    match notice {
        FeedNotice::Reconnecting { headline }
        | FeedNotice::Working { headline }
        | FeedNotice::Attention { headline, .. } => Some(headline),
        FeedNotice::Connected | FeedNotice::Clear => None,
    }
}

/// How long the pause of a feed's first connect may run before the log says
/// so: every launch starts with one, and the stall judgement's own budget is
/// where a first connect stops being progress.
pub const FIRST_CONNECT_QUIET_MS: i64 = crate::stall::FIRST_CONNECT_BUDGET_MS;

/// When entries paused, so leaving can say for how long, and whether the log
/// has said it paused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncWatch {
    since_ms: Option<i64>,
    logged: bool,
}

/// An edge of the pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncChange {
    Entered,
    Left { paused_ms: i64 },
}

impl SyncWatch {
    /// Take this observation; an edge for the log when the pause began or
    /// ended. A pause on a `first_connect` — a feed that has never been up —
    /// is said only once it outlasts [`FIRST_CONNECT_QUIET_MS`] or turns into
    /// another cause, and its end only if its start was said.
    pub fn step(&mut self, paused: bool, first_connect: bool, now_ms: i64) -> Option<SyncChange> {
        match (self.since_ms, paused) {
            (None, true) => {
                self.since_ms = Some(now_ms);
                self.logged = !first_connect;
                self.logged.then_some(SyncChange::Entered)
            }
            (Some(since), true) if !self.logged => {
                let outlasted = now_ms.saturating_sub(since) >= FIRST_CONNECT_QUIET_MS;
                self.logged = !first_connect || outlasted;
                self.logged.then_some(SyncChange::Entered)
            }
            (Some(since), false) => {
                self.since_ms = None;
                let paused_ms = now_ms.saturating_sub(since).max(0);
                std::mem::take(&mut self.logged).then_some(SyncChange::Left { paused_ms })
            }
            _ => None,
        }
    }
}

/// Say an edge of the pause in the log: every cause field and the intake lag
/// on the way in, how long it held on the way out.
pub fn log_change(
    change: SyncChange,
    desync: Option<&Desync>,
    symbol: &str,
    intake: Option<IntakeLagSummary>,
) {
    match (change, desync) {
        (SyncChange::Entered, Some(desync)) => tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_DESYNC_ENTERED",
            symbol,
            reading = desync.reading.word(),
            link = desync.link.word(),
            latency_ms = desync.latency_ms,
            latency_hop = desync.latency_hop,
            tape_age_ms = desync.tape_age_ms,
            max_latency_ms = desync.limits.max_latency_ms,
            max_tape_age_ms = desync.limits.max_tape_age_ms,
            notice = desync.notice.as_deref(),
            intake_p50_ms = intake.map(|lag| lag.p50_ms),
            intake_p99_ms = intake.map(|lag| lag.p99_ms),
            intake_max_ms = intake.map(|lag| lag.max_ms),
            intake_samples = intake.map(|lag| lag.samples),
            action = "pause_new_entries",
            "the chart is out of sync with its feed"
        ),
        (SyncChange::Left { paused_ms }, _) => tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FEED_DESYNC_LEFT",
            symbol,
            paused_ms,
            action = "resume_new_entries",
            "the chart is back in sync with its feed"
        ),
        (SyncChange::Entered, None) => {}
    }
}

#[cfg(test)]
mod tests {
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
}
