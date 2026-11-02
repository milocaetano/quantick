//! The connection state on the wire: what `feed.status` reports for one tab.

use quantick_chart::connection_state::{ConnectionTracker, Link};
use quantick_control_schema::feed::ConnectionSnapshot;

/// The tab's connection state on the wire. Time-bound fields travel only with a
/// clock: the revision key reads with none, and they would otherwise move it on
/// every frame the transport is up.
#[must_use]
pub fn snapshot(tracker: &ConnectionTracker, now_ms: Option<i64>) -> ConnectionSnapshot {
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
        reconnect_offered: state.link == Link::Offline && now_ms.is_some(),
        status_word: word.map(|word| word.word.to_owned()),
        overlay_up: now_ms.is_some_and(|now| tracker.overlay_up(now)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no clock nothing time-bound travels, or the revision key would move
    /// on every frame.
    #[test]
    fn nothing_time_bound_travels_without_a_clock() {
        let snapshot = snapshot(&ConnectionTracker::new(), None);
        assert_eq!(snapshot.link, "attempting");
        assert_eq!(snapshot.status_word, None);
        assert!(!snapshot.overlay_up && !snapshot.reconnect_offered);
    }
}
