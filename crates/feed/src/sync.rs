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
#[path = "sync_tests.rs"]
mod tests;
