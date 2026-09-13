//! The prints of a bar still forming, and the bounded fold that samples them.
//!
//! A consumer that needs the bar a prefix of the tape would have formed —
//! the chart's live lane asking an indicator what it showed mid-bar — holds
//! the forming bar's own prints here, appends each once, and cuts the run
//! when the bar closes. A walk asks for at most a given number of *rungs*:
//! prefixes of the run at evenly spaced positions, plus the last print. The
//! positions depend on the run's current length, so every prefix must stay
//! reachable.
//!
//! **The limit.** Folding every prefix from the run's first print costs
//! O(forming prints) per walk, so a bar that formed for an hour on a dense
//! tape folds a million prints every time it is sampled. The run carries its
//! fold instead: a running bar, extended once per appended print, and a
//! checkpoint — the exact fold so far — every [`CHECKPOINT_SPACING`] prints.
//! A walk folds forward from rung to rung and jumps to the checkpoint at or
//! below a rung whenever that lies past where it stands, so a rung folds at
//! most `CHECKPOINT_SPACING - 1` prints and the last rung is the running bar
//! itself. A walk of `r` rungs over `len` prints therefore folds at most
//! `min(len, 63 r)` prints — never more than one pass over the run — and
//! appending folds each print once.
//!
//! **Identity.** [`Bar::extend`] is a sequential fold over plain data, so a
//! checkpoint is the same intermediate state a fold from the first print
//! passes through, and every prefix is the same bar, byte for byte, as
//! [`Bar::opened_by`] plus [`Bar::extend`] over the same prints —
//! `tests/forming_run.rs` holds that unbounded fold as its oracle.

use std::cell::Cell;

use crate::{Bar, Trade};

/// Prints between two retained checkpoints of the run's fold.
///
/// Sixty-four keeps the checkpoints at about 2 bytes per print held (one
/// 120-byte bar per 64 prints of 56 bytes) while bounding a 64-rung walk to
/// 4,032 folds — the cost of one pass over a run of 4,032 prints, a few
/// seconds of a burst.
pub const CHECKPOINT_SPACING: usize = 64;

/// The prints of a forming bar, in occurrence order, with their fold carried
/// along. Cut it with `FormingRun::default()`, which also gives its storage
/// back.
#[derive(Debug, Default)]
pub struct FormingRun {
    trades: Vec<Trade>,
    /// `checkpoints[i]` is the fold of `trades[..(i + 1) * CHECKPOINT_SPACING]`.
    checkpoints: Vec<Bar>,
    /// The fold of every print held: the last rung of any walk.
    running: Option<Bar>,
    /// Prints folded into a bar by this run, appends and walks alike.
    folds: Cell<u64>,
}

impl FormingRun {
    /// Append newly arrived prints, in order, folding each once.
    pub fn extend(&mut self, run: Vec<Trade>) {
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
    #[must_use]
    pub fn len(&self) -> usize {
        self.trades.len()
    }

    /// Whether the run holds no print.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.trades.is_empty()
    }

    /// Prints the run's storage can hold without growing — what a cut run
    /// must give back.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.trades.capacity()
    }

    /// Prints this run has folded into a bar, appends and walks alike: the
    /// work a walk's budget is stated in, readable so a consumer's test can
    /// hold it there.
    #[must_use]
    pub fn folds(&self) -> u64 {
        self.folds.get()
    }

    fn count_folds(&self, prints: usize) {
        self.folds.set(self.folds.get() + prints as u64);
    }

    /// The run's forming-bar prefixes, at most `rungs` of them, oldest first.
    ///
    /// The prints are the forming bar's own, in occurrence order, so folding
    /// all of them reproduces the forming bar — which is why the last print
    /// always gets a rung: the right edge of a lane is the live edge, and
    /// stopping short of it would draw the tape as if the newest prints had
    /// not happened. The rungs sit every `ceil(len / rungs)` prints, plus the
    /// last print. No run or no rung is no prefix.
    #[must_use]
    pub fn prefixes(&self, rungs: usize) -> Vec<Bar> {
        let len = self.trades.len();
        let Some(running) = &self.running else {
            return Vec::new();
        };
        if rungs == 0 {
            return Vec::new();
        }
        let step = len.div_ceil(rungs.min(len)).max(1);
        let mut prefixes = Vec::with_capacity(len.div_ceil(step) + 1);
        // The fold walks forward from rung to rung and jumps to a checkpoint
        // whenever one lies past where it stands, so a rung folds at most
        // `step` prints and never more than `CHECKPOINT_SPACING - 1`: a short
        // run costs one pass, a long one the budget.
        let mut folded: Option<(Bar, usize)> = None;
        let mut end = step;
        while end < len {
            let base = end - end % CHECKPOINT_SPACING;
            let (mut bar, from) = match folded.take() {
                Some((bar, at)) if at >= base => (bar, at),
                _ if base > 0 => (
                    self.checkpoints[base / CHECKPOINT_SPACING - 1].clone(),
                    base,
                ),
                _ => {
                    self.count_folds(1);
                    (Bar::opened_by(&self.trades[0]), 1)
                }
            };
            debug_assert!(
                end - from < CHECKPOINT_SPACING,
                "a rung folds past its checkpoint"
            );
            for trade in &self.trades[from..end] {
                bar.extend(trade);
            }
            self.count_folds(end - from);
            prefixes.push(bar.clone());
            folded = Some((bar, end));
            end += step;
        }
        prefixes.push(running.clone());
        prefixes
    }
}
