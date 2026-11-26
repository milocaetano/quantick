//! The prints of a bar still forming, and the bounded fold that samples them. A checkpoint
//! every [`CHECKPOINT_SPACING`] prints bounds a rung's folds; every prefix equals [`Bar::opened_by`]
//! plus [`Bar::extend`] over the same prints (`tests/forming_run.rs` holds the oracle).

use std::cell::Cell;

use crate::{Bar, Trade};

/// Prints between two retained checkpoints of the run's fold: about 2 bytes held per print,
/// and a 64-rung walk folds at most 4,032 prints.
pub const CHECKPOINT_SPACING: usize = 64;

/// The prints of a forming bar, in order, with their fold carried along.
/// Cut it with `FormingRun::default()`, which also frees its storage.
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

    /// The run's forming-bar prefixes, at most `rungs`, oldest first.
    /// Rungs sit every `ceil(len / rungs)` prints and always include the last print (the live edge).
    /// No run or no rung is no prefix.
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
        // Fold forward from rung to rung, jumping to a checkpoint when one lies past the cursor:
        // a rung folds at most `step` prints, never more than `CHECKPOINT_SPACING - 1`.
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
