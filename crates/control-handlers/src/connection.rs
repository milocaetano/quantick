//! The connection state on the wire: what `feed.status` reports for one tab.

use quantick_chart::connection_state::{ConnectionTracker, Link};
use quantick_control_schema::feed::ConnectionSnapshot;
use quantick_sources::sync::Desync;

/// Whether a one-click Reconnect stands on screen outside the popup: the
/// overlay's or the status word's while the link is offline, else the tape
/// cell's while entries pause on a link that is up. One answer for the
/// scene, `feed.status` and the status bar.
#[must_use]
pub fn reconnect_offered(link: Link, gate: Option<&Desync>) -> bool {
    link == Link::Offline || gate.is_some_and(Desync::offers_reconnect)
}

/// The tab's connection state on the wire. Time-bound fields travel only with a
/// clock: the revision key reads with none, and they would otherwise move it on
/// every frame the transport is up.
#[must_use]
pub fn snapshot(
    tracker: &ConnectionTracker,
    gate: Option<&Desync>,
    now_ms: Option<i64>,
) -> ConnectionSnapshot {
    let state = tracker.state();
    let word = now_ms.and_then(|now| tracker.status_word(now));
    ConnectionSnapshot {
        link: match state.link {
            Link::Live => "live",
            Link::Attempting => "attempting",
            Link::Offline => "offline",
            Link::Quiet => "quiet",
            Link::NoTransport => "no_transport",
        }
        .to_owned(),
        feed: state.feed.word().to_owned(),
        book: state.book.word().to_owned(),
        trades: state.trades.word().to_owned(),
        last_connected_unix_ms: now_ms.and(state.last_connected_at),
        current_job: state.current_job.map(|job| job.task.label().to_owned()),
        reconnect_offered: reconnect_offered(state.link, gate) && now_ms.is_some(),
        status_word: word.map(|word| word.word.to_owned()),
        overlay_up: now_ms.is_some_and(|now| tracker.overlay_up(now)),
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
