//! The forming bar's own prints, as the indicator worker holds them, and the
//! bounded fold that samples them into lane rungs.
//!
//! The producer sends each print once ([`super::LaneTransport`]); the worker
//! appends it here and cuts the run when the bar closes, the series is
//! rebuilt, or the lane goes away. A walk asks for at most
//! [`super::MAX_LANE_RUNGS`] prefixes of the run — the bar the tape would
//! have formed after its first *n* prints — and the prefix positions depend
//! on the run's current length, so every prefix must stay reachable.
//!
//! **The limit.** Folding every prefix from the run's first print made a walk
//! cost O(forming prints) on every drained batch, so a bar that formed for an
//! hour on a dense tape (a `time:1h` chart, a large dollar bar) folded a
//! million prints sixty times a second. The run now carries its fold: a
//! running bar, extended once per appended print, and a checkpoint — the
//! exact fold so far — every [`CHECKPOINT_SPACING`] prints. A rung starts from
//! the checkpoint at or below its position and folds at most
//! `CHECKPOINT_SPACING - 1` prints; the last rung is the running bar itself.
//! A walk of `r` rungs therefore folds at most `r × (CHECKPOINT_SPACING - 1)`
//! prints whatever the run's length, and appending
//! folds each print once. [`Bar::extend`] is a sequential fold over plain
//! data, so a checkpoint is the same intermediate state the unbounded fold
//! passed through and every prefix is the same bar, byte for byte
//! (`forming_run_tests` holds the old fold as the oracle).

use quantick_engine::{Bar, Trade};

/// Prints between two retained checkpoints of the run's fold.
///
/// Sixty-four keeps the checkpoints at about 2 bytes per print held (one
/// 120-byte bar per 64 prints of 56 bytes) while bounding a full
/// [`super::MAX_LANE_RUNGS`] walk to 4,032 folds — the cost of the old walk
/// over a run of 4,032 prints, a few seconds of a burst.
pub(crate) const CHECKPOINT_SPACING: usize = 64;

/// The forming run, in occurrence order, with its fold carried along.
#[derive(Default)]
pub(crate) struct FormingRun {
    trades: Vec<Trade>,
    /// `checkpoints[i]` is the fold of `trades[..(i + 1) * CHECKPOINT_SPACING]`.
    checkpoints: Vec<Bar>,
    /// The fold of every print held: the last rung of any walk.
    running: Option<Bar>,
    /// Prints folded into a bar so far, counted so a test can hold a walk to
    /// its budget.
    #[cfg(test)]
    folds: std::cell::Cell<u64>,
}

impl FormingRun {
    /// Append newly sent prints, in order, folding each once.
    pub(crate) fn extend(&mut self, run: Vec<Trade>) {
        let appended = run.len();
        self.trades.reserve(appended);
        for trade in run {
            match &mut self.running {
                None => self.running = Some(Bar::opened_by(&trade)),
                Some(bar) => bar.extend(&trade),
            }
            self.trades.push(trade);
            if self.trades.len().is_multiple_of(CHECKPOINT_SPACING) {
                self.checkpoints
                    .push(self.running.clone().expect("a print opened the bar"));
            }
        }
        self.count_folds(appended);
    }

    /// Prints held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.trades.len()
    }

    /// Prints the run's storage can hold without growing — what a retired
    /// epoch must give back.
    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.trades.capacity()
    }

    /// Prints folded into a bar since the run was created.
    #[cfg(test)]
    pub(crate) fn folds(&self) -> u64 {
        self.folds.get()
    }

    #[cfg(test)]
    fn count_folds(&self, prints: usize) {
        self.folds.set(self.folds.get() + prints as u64);
    }

    #[cfg(not(test))]
    fn count_folds(&self, _prints: usize) {}

    /// The fold of the run's first `end` prints (`1 ≤ end ≤ len`), from the
    /// checkpoint at or below `end`.
    fn prefix(&self, end: usize) -> Bar {
        let full = end / CHECKPOINT_SPACING;
        let (mut bar, from) = match full.checked_sub(1) {
            Some(index) => (self.checkpoints[index].clone(), full * CHECKPOINT_SPACING),
            None => (Bar::opened_by(&self.trades[0]), 1),
        };
        debug_assert!(
            end - from < CHECKPOINT_SPACING,
            "a rung folds past its checkpoint"
        );
        for trade in &self.trades[from..end] {
            bar.extend(trade);
        }
        self.count_folds(end - from + usize::from(full == 0));
        bar
    }

    /// The run's forming-bar prefixes, at most `rungs` of them, oldest first.
    ///
    /// The trades are the forming bar's own, in occurrence order, so folding
    /// all of them reproduces the bar the chart is drawing — which is why the
    /// last print always gets a rung: the lane's right edge is the live edge,
    /// and stopping a sample short of it would draw the tape as if the newest
    /// prints had not happened. The rungs sit every `ceil(len / rungs)`
    /// prints, plus the last print.
    pub(crate) fn prefixes(&self, rungs: usize) -> Vec<Bar> {
        let len = self.trades.len();
        let Some(running) = &self.running else {
            return Vec::new();
        };
        if rungs == 0 {
            return Vec::new();
        }
        let step = len.div_ceil(rungs.min(len)).max(1);
        let mut prefixes = Vec::with_capacity(len.div_ceil(step) + 1);
        let mut end = step;
        while end < len {
            prefixes.push(self.prefix(end));
            end += step;
        }
        prefixes.push(running.clone());
        prefixes
    }
}
