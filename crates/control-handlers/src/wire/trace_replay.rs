//! The control trace re-injected into a replay: which recorded actions are
//! due at the playhead, walked once per pass over the session and rewound
//! with it. The window loads the sidecar, reads the playhead and runs the
//! due actions; this is the walk.
//!
//! Contract §11: a replayed session performs the actions the recorded one
//! did, at their logical replay time, so a rerun is a fixture and not a
//! re-enactment. This file owns when an entry is due, how a rewind or a
//! second tab on the same recording is handled, and when a sidecar is read.
//! It never records: writing the trace is the action door's business.
//!
//! Rate class: per frame, one comparison per live tab; the `Vec` of due
//! entries allocates only when one came due, a human gesture's worth of
//! times per session.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quantick_control_host::trace::{TraceEntry, TraceReplay};

/// One recording's control trace, loaded once and walked by logical replay
/// time. Keyed by the session path: two tabs on the same recording share one
/// walk, driven by the tab that loaded it.
pub struct TraceReinjection {
    /// The tab whose playhead drives the walk.
    pub owner_tab_id: u64,
    /// Completed entries in `(replay_elapsed_ms, sequence)` order — the
    /// sidecar's at load time plus the actions this run recorded since, so
    /// an in-session restart replays exactly what a fresh process would.
    pub entries: Vec<TraceEntry>,
    /// The first entry not yet injected on this pass over the session.
    pub next_index: usize,
    /// Where the playhead was last frame; a smaller value now means it moved
    /// backwards and the walk rewinds.
    pub last_elapsed_ms: i64,
    /// The worker's rewind count last frame; a different value now means a
    /// restart or seek happened, even if the rerun already advanced past
    /// `last_elapsed_ms`.
    pub last_rewinds: u64,
    /// Sequences of the actions this run took during the current pass: they
    /// joined `entries` for the next rerun and are not injected back on the
    /// spot. Cleared by a rewind.
    pub executed_this_pass: Vec<u64>,
}

/// What the replay link publishes that the walk reads once per frame.
#[derive(Clone, Copy)]
pub struct ReplayPosition {
    pub elapsed_ms: i64,
    pub rewinds: u64,
    pub rewind_target_elapsed_ms: i64,
}

impl TraceReinjection {
    /// Move the entries due at the position into `due`, exactly once per
    /// pass over the session. A rewind — the worker counted a restart or a
    /// seek, or the playhead is behind last frame's sample — moves the walk
    /// back to the first entry at or after where the rerun began, so the
    /// rerun injects the same actions again.
    pub fn collect_due(&mut self, position: ReplayPosition, due: &mut Vec<TraceEntry>) {
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
    pub fn record_this_pass(&mut self, entry: TraceEntry) {
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

/// One tab playing a recording, as the walks read it once per frame.
#[derive(Clone, Copy)]
pub struct PlayingTab<'a> {
    pub tab_id: u64,
    /// The recording's session file; its trace sidecar sits beside it.
    pub session_path: &'a Path,
    pub position: ReplayPosition,
}

/// The control trace of every replaying recording, keyed by session path,
/// loaded once and walked by logical replay time; a tab without a replay
/// costs the frame one comparison.
#[derive(Default)]
pub struct TraceWalks(BTreeMap<PathBuf, TraceReinjection>);

impl TraceWalks {
    /// The walk of the recording at `session_path`, once a tab loaded it.
    pub fn get_mut(&mut self, session_path: &Path) -> Option<&mut TraceReinjection> {
        self.0.get_mut(session_path)
    }

    /// The traced entries that came due this frame, across every recording a
    /// tab is playing. `playing` lists those tabs in tab order, and is asked
    /// again whenever the walk needs another look. The trace is loaded once
    /// per recording, through `load`, and walked forward by the tab that
    /// loaded it; a restart or seek rewinds the walk so the rerun injects
    /// the same actions again, switching tabs neither repeats nor skips an
    /// injection, and a second tab on the same recording adds nothing.
    /// Loading a sidecar moves `next_trace_sequence` past every sequence it
    /// holds.
    pub fn collect_due<'a, I>(
        &mut self,
        playing: impl Fn() -> I,
        next_trace_sequence: &mut u64,
        mut load: impl FnMut(&Path) -> TraceReplay,
    ) -> Vec<TraceEntry>
    where
        I: Iterator<Item = PlayingTab<'a>>,
    {
        // The Vec allocates only when an entry came due, a human gesture's
        // worth of times per session.
        let mut due: Vec<TraceEntry> = Vec::new();
        if !self.0.is_empty() {
            self.0
                .retain(|path, _| playing().any(|tab| tab.session_path == path.as_path()));
        }
        for tab in playing() {
            let (tab_id, path, position) = (tab.tab_id, tab.session_path, tab.position);
            match self.0.get_mut(path) {
                Some(state) if state.owner_tab_id == tab_id => {
                    state.collect_due(position, &mut due);
                }
                // One walk per recording: the tab that loaded it drives.
                // Another tab on the same file adopts the walk only once
                // the owner let go of the session.
                Some(state) => {
                    let owner_still_plays_it = playing().any(|other| {
                        other.tab_id == state.owner_tab_id && other.session_path == path
                    });
                    if !owner_still_plays_it {
                        state.owner_tab_id = tab_id;
                        state.collect_due(position, &mut due);
                    }
                }
                None => {
                    let loaded = load(path);
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
                    self.0.insert(path.to_path_buf(), state);
                }
            }
        }
        due
    }
}
