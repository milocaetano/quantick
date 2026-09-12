//! Replaying a recording's control trace: the walk over its entries, and the
//! per-frame service that re-injects the ones that came due.
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

use crate::app::QuantickApp;

use super::super::trace::{TraceEntry, TraceReplay};
use super::{ActionOrigin, ControlAccess, RecordedActor};

/// A replay session whose control trace is being re-injected: the entries
/// still due, in replay-time order.
/// One recording's control trace, loaded once and walked by logical replay
/// time. Keyed by the session path: two tabs on the same recording share one
/// walk, driven by the tab that loaded it.
pub(super) struct TraceReinjection {
    /// The tab whose playhead drives the walk.
    owner_tab_id: u64,
    /// Completed entries in `(replay_elapsed_ms, sequence)` order — the
    /// sidecar's at load time plus the actions this run recorded since, so
    /// an in-session restart replays exactly what a fresh process would.
    entries: Vec<TraceEntry>,
    /// The first entry not yet injected on this pass over the session.
    next_index: usize,
    /// Where the playhead was last frame; a smaller value now means it moved
    /// backwards and the walk rewinds.
    last_elapsed_ms: i64,
    /// The worker's rewind count last frame; a different value now means a
    /// restart or seek happened, even if the rerun already advanced past
    /// `last_elapsed_ms`.
    last_rewinds: u64,
    /// Sequences of the actions this run took during the current pass: they
    /// joined `entries` for the next rerun and are not injected back on the
    /// spot. Cleared by a rewind.
    executed_this_pass: Vec<u64>,
}

/// What the replay link publishes that the walk reads once per frame.
#[derive(Clone, Copy)]
struct ReplayPosition {
    elapsed_ms: i64,
    rewinds: u64,
    rewind_target_elapsed_ms: i64,
}

impl ReplayPosition {
    fn of(status: &quantick_feed::replay::ReplayStatus) -> Self {
        Self {
            elapsed_ms: status.elapsed_ms(),
            rewinds: status.rewinds(),
            rewind_target_elapsed_ms: status.rewind_target_elapsed_ms(),
        }
    }
}

impl TraceReinjection {
    /// Move the entries due at the position into `due`, exactly once per
    /// pass over the session. A rewind — the worker counted a restart or a
    /// seek, or the playhead is behind last frame's sample — moves the walk
    /// back to the first entry at or after where the rerun began, so the
    /// rerun injects the same actions again.
    fn collect_due(&mut self, position: ReplayPosition, due: &mut Vec<TraceEntry>) {
        let rewound_to = if position.rewinds != self.last_rewinds {
            Some(position.rewind_target_elapsed_ms)
        } else if position.elapsed_ms < self.last_elapsed_ms {
            Some(position.elapsed_ms)
        } else {
            None
        };
        if let Some(start_elapsed_ms) = rewound_to {
            self.next_index = self
                .entries
                .partition_point(|entry| entry.replay_elapsed_ms < start_elapsed_ms);
            self.executed_this_pass.clear();
        }
        self.last_rewinds = position.rewinds;
        self.last_elapsed_ms = position.elapsed_ms;
        while let Some(entry) = self.entries.get(self.next_index)
            && entry.replay_elapsed_ms <= position.elapsed_ms
        {
            if !self.executed_this_pass.contains(&entry.sequence.get()) {
                due.push(entry.clone());
            }
            self.next_index += 1;
        }
    }

    /// An action this run just recorded to the sidecar joins the walk in
    /// replay-time order, marked as executed on this pass: the next rerun
    /// replays it, this one does not inject it back.
    pub(super) fn record_this_pass(&mut self, entry: TraceEntry) {
        let key = (entry.replay_elapsed_ms, entry.sequence.get());
        let position = self
            .entries
            .partition_point(|other| (other.replay_elapsed_ms, other.sequence.get()) < key);
        if position < self.next_index {
            self.next_index += 1;
        }
        self.executed_this_pass.push(entry.sequence.get());
        self.entries.insert(position, entry);
    }
}

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

impl ControlAccess {
    /// Each frame: every recording a tab is playing with a control trace
    /// beside it re-injects the recorded actions at their logical replay
    /// time (contract §11). The trace is loaded once per recording and walked
    /// forward by the tab that loaded it; a restart or seek rewinds the walk
    /// so the rerun injects the same actions again, switching tabs neither
    /// repeats nor skips an injection, and a second tab on the same
    /// recording adds nothing. A live tab costs one comparison. Runs whether
    /// or not local access is enabled: replay determinism does not depend on
    /// a client being connected.
    pub(crate) fn service_replay_trace(&mut self, app: &mut QuantickApp) {
        // The entries that came due this frame. The Vec allocates only when
        // one did, a human gesture's worth of times per session.
        let mut due: Vec<TraceEntry> = Vec::new();
        {
            let tabs = app.control_tabs();
            if !self.trace_reinjection.is_empty() {
                self.trace_reinjection.retain(|path, _| {
                    tabs.iter().any(|tab| {
                        tab.replay
                            .as_ref()
                            .is_some_and(|link| link.session.path == *path)
                    })
                });
            }
            for tab in tabs {
                let Some(link) = tab.replay.as_ref() else {
                    continue;
                };
                let position = ReplayPosition::of(&link.status);
                let path = &link.session.path;
                match self.trace_reinjection.get_mut(path) {
                    Some(state) if state.owner_tab_id == tab.id => {
                        state.collect_due(position, &mut due);
                    }
                    // One walk per recording: the tab that loaded it drives.
                    // Another tab on the same file adopts the walk only once
                    // the owner let go of the session.
                    Some(state) => {
                        let owner_still_plays_it = tabs.iter().any(|other| {
                            other.id == state.owner_tab_id
                                && other
                                    .replay
                                    .as_ref()
                                    .is_some_and(|link| link.session.path == *path)
                        });
                        if !owner_still_plays_it {
                            state.owner_tab_id = tab.id;
                            state.collect_due(position, &mut due);
                        }
                    }
                    None => {
                        let loaded = load_trace_for_reinjection(path);
                        // Trace sequences continue where the sidecar left
                        // off, so a later run appending to the same file
                        // never reuses one.
                        self.next_trace_sequence = self
                            .next_trace_sequence
                            .max(loaded.max_sequence.saturating_add(1));
                        let mut state = TraceReinjection {
                            owner_tab_id: tab.id,
                            entries: loaded.completed,
                            next_index: 0,
                            last_elapsed_ms: i64::MIN,
                            last_rewinds: position.rewinds,
                            executed_this_pass: Vec::new(),
                        };
                        state.collect_due(position, &mut due);
                        self.trace_reinjection.insert(path.clone(), state);
                    }
                }
            }
        }
        for entry in due {
            if let Err(error) = self.invoke_local_action(
                app,
                entry.capability_id.as_str(),
                entry.capability_version,
                entry.canonical_input,
                ActionOrigin::TraceReplay(Box::new(RecordedActor {
                    actor_kind: entry.actor_kind,
                    client_name: entry.client_name.clone(),
                })),
            ) {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_TRACE_REPLAY_REFUSED",
                    capability = %entry.capability_id,
                    version = entry.capability_version,
                    code = %error.code,
                    "a traced action was refused on replay"
                );
            }
        }
    }
}
