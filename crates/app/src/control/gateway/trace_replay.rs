//! Replaying a recording's control trace: which tabs play a recording, and
//! how its sidecar is read. When an entry is due, and which tab's playhead
//! drives a recording's walk, is [`TraceWalks`]'s, in
//! `quantick_control_schema::trace_replay`; `ControlAccess::service_replay_trace`
//! re-injects what came due through the same `invoke_local_action` the
//! hotkey uses.

use crate::app::arrangement_host::ArrangementHost;

use super::super::trace::TraceReplay;
pub(super) use quantick_control_schema::trace_replay::{PlayingTab, ReplayPosition, TraceWalks};

/// Read a session's sidecar for re-injection, naming an unreadable or an
/// unfinished trace in the log: either way that run is not a fixture.
pub(super) fn load_trace_for_reinjection(session_path: &std::path::Path) -> TraceReplay {
    let loaded = match TraceReplay::load(session_path) {
        Ok(loaded) => loaded,
        Err(error) => {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_TRACE_UNREADABLE",
                error = %error,
                "the replay's control trace could not be read; the run is not a fixture"
            );
            TraceReplay::default()
        }
    };
    if !loaded.is_complete() {
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_TRACE_INCOMPLETE",
            incomplete = ?loaded.incomplete,
            "the replay's control trace has unfinished intents; the run is not a fixture"
        );
    }
    loaded
}

/// Every tab playing a recording, in tab order, with the playhead its
/// replay link publishes.
pub(super) fn playing(tabs: &ArrangementHost) -> impl Iterator<Item = PlayingTab<'_>> {
    tabs.iter_with_ids().filter_map(|(tab_id, tab)| {
        let link = tab.link.replay.as_ref()?;
        Some(PlayingTab {
            tab_id,
            session_path: &link.session.path,
            position: ReplayPosition {
                elapsed_ms: link.status.elapsed_ms(),
                rewinds: link.status.rewinds(),
                rewind_target_elapsed_ms: link.status.rewind_target_elapsed_ms(),
            },
        })
    })
}
