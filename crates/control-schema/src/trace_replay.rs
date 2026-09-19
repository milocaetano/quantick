//! The control trace re-injected into a replay: which recorded actions are
//! due at the playhead, walked once per pass over the session and rewound
//! with it. The window loads the sidecar, reads the playhead and runs the
//! due actions; this is the walk.

use quantick_control_host::trace::TraceEntry;

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
