//! The forming bar's own prints, as the indicator worker holds them, and the
//! fold that samples them into lane rungs.
//!
//! The producer sends each print once ([`super::LaneTransport`]); the worker
//! appends it here and cuts the run when the bar closes, the series is
//! rebuilt, or the lane goes away. A walk asks for at most
//! [`super::MAX_LANE_RUNGS`] prefixes of the run — the bar the tape would
//! have formed after its first *n* prints — and the prefix positions depend
//! on the run's current length, so every prefix must stay reachable.

use quantick_engine::{Bar, Trade};

/// Prints between two retained checkpoints of the run's fold: the most a
/// single rung will have to fold once the walk is bounded.
#[allow(dead_code)] // Read by the bounded walk; until then only by its tests.
pub(crate) const CHECKPOINT_SPACING: usize = 64;

/// The forming run, in occurrence order.
#[derive(Default)]
pub(crate) struct FormingRun {
    trades: Vec<Trade>,
    /// Prints folded into a bar so far, counted so a test can hold a walk to
    /// its budget.
    #[cfg(test)]
    folds: std::cell::Cell<u64>,
}

impl FormingRun {
    /// Append newly sent prints, in order.
    pub(crate) fn extend(&mut self, run: Vec<Trade>) {
        self.trades.extend(run);
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

    /// Fold the run into forming-bar prefixes, at most `rungs` of them.
    ///
    /// The trades are the forming bar's own, in occurrence order, so folding
    /// all of them reproduces the bar the chart is drawing — which is why the
    /// last print always gets a rung: the lane's right edge is the live edge,
    /// and stopping a sample short of it would draw the tape as if the newest
    /// prints had not happened.
    pub(crate) fn prefixes(&self, rungs: usize) -> Vec<Bar> {
        let run = &self.trades;
        if run.is_empty() || rungs == 0 {
            return Vec::new();
        }
        let step = run.len().div_ceil(rungs.min(run.len())).max(1);
        let mut prefixes = Vec::with_capacity(run.len().div_ceil(step) + 1);
        let mut forming: Option<Bar> = None;
        for (index, trade) in run.iter().enumerate() {
            match &mut forming {
                None => forming = Some(Bar::opened_by(trade)),
                Some(bar) => bar.extend(trade),
            }
            let last = index + 1 == run.len();
            if last || (index + 1) % step == 0 {
                prefixes.push(forming.clone().expect("a trade opened the bar"));
            }
        }
        self.count_folds(run.len());
        prefixes
    }
}
