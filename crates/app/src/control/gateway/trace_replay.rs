//! Replaying a recording's control trace: the walk over its entries, and
//! which of them came due this frame. `ControlAccess::service_replay_trace`
//! re-injects those.
//!
//! Contract §11: a replayed session performs the actions the recorded one
//! did, at their logical replay time, through the same `invoke_local_action`
//! the hotkey uses -- so a rerun is a fixture and not a re-enactment. This
//! file owns when an entry is due, how a rewind or a second tab on the same
//! recording is handled, and what an unreadable or unfinished sidecar means.
//! It never records: writing the trace is the action door's business.
//!
//! Rate class: per frame, one comparison per live tab; the `Vec` of due
//! entries allocates only when one came due, a human gesture's worth of
//! times per session.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::app::arrangement_host::ArrangementHost;

use super::super::trace::{TraceEntry, TraceReplay};
pub(super) use quantick_control_schema::trace_replay::{ReplayPosition, TraceReinjection};

/// Read a session's sidecar for re-injection, naming an unreadable or an
/// unfinished trace in the log: either way that run is not a fixture.
fn load_trace_for_reinjection(session_path: &std::path::Path) -> TraceReplay {
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

/// The control trace of every replaying recording, keyed by session path,
/// loaded once and walked by logical replay time; a tab without a replay
/// costs the frame one comparison.
#[derive(Default)]
pub(super) struct TraceWalks(BTreeMap<PathBuf, TraceReinjection>);

impl TraceWalks {
    /// The walk of the recording at `session_path`, once a tab loaded it.
    pub(super) fn get_mut(&mut self, session_path: &Path) -> Option<&mut TraceReinjection> {
        self.0.get_mut(session_path)
    }

    /// The traced entries that came due this frame, across every recording a
    /// tab is playing. The trace is loaded once per recording and walked
    /// forward by the tab that loaded it; a restart or seek rewinds the walk
    /// so the rerun injects the same actions again, switching tabs neither
    /// repeats nor skips an injection, and a second tab on the same
    /// recording adds nothing. Loading a sidecar moves `next_trace_sequence`
    /// past every sequence it holds.
    pub(super) fn collect_due(
        &mut self,
        tabs: &ArrangementHost,
        next_trace_sequence: &mut u64,
    ) -> Vec<TraceEntry> {
        // The Vec allocates only when an entry came due, a human gesture's
        // worth of times per session.
        let mut due: Vec<TraceEntry> = Vec::new();
        if !self.0.is_empty() {
            self.0.retain(|path, _| {
                tabs.iter().any(|tab| {
                    tab.replay
                        .as_ref()
                        .is_some_and(|link| link.session.path == *path)
                })
            });
        }
        for (tab_id, tab) in tabs.iter_with_ids() {
            let Some(link) = tab.replay.as_ref() else {
                continue;
            };
            let position = ReplayPosition {
                elapsed_ms: link.status.elapsed_ms(),
                rewinds: link.status.rewinds(),
                rewind_target_elapsed_ms: link.status.rewind_target_elapsed_ms(),
            };
            let path = &link.session.path;
            match self.0.get_mut(path) {
                Some(state) if state.owner_tab_id == tab_id => {
                    state.collect_due(position, &mut due);
                }
                // One walk per recording: the tab that loaded it drives.
                // Another tab on the same file adopts the walk only once
                // the owner let go of the session.
                Some(state) => {
                    let owner_still_plays_it = tabs.iter_with_ids().any(|(other_id, other)| {
                        other_id == state.owner_tab_id
                            && other
                                .replay
                                .as_ref()
                                .is_some_and(|link| link.session.path == *path)
                    });
                    if !owner_still_plays_it {
                        state.owner_tab_id = tab_id;
                        state.collect_due(position, &mut due);
                    }
                }
                None => {
                    let loaded = load_trace_for_reinjection(path);
                    // Trace sequences continue where the sidecar left
                    // off, so a later run appending to the same file
                    // never reuses one.
                    *next_trace_sequence =
                        (*next_trace_sequence).max(loaded.max_sequence.saturating_add(1));
                    let mut state = TraceReinjection {
                        owner_tab_id: tab_id,
                        entries: loaded.completed,
                        next_index: 0,
                        last_elapsed_ms: i64::MIN,
                        last_rewinds: position.rewinds,
                        executed_this_pass: Vec::new(),
                    };
                    state.collect_due(position, &mut due);
                    self.0.insert(path.clone(), state);
                }
            }
        }
        due
    }
}
