//! The window's side of fixed-range volume profile drawings.
//!
//! The refresh pass itself — [`refresh`], [`RefreshInputs`], [`TOOL_ID`] and
//! [`DEFAULT_FOLD_BUDGET`] — lives in `quantick_drawings::frvp`, with its
//! tests, and is re-exported here for the pane. What stays is what belongs
//! to the process: [`fold_budget`], the per-pass fold budget the pane hands
//! every refresh, and the `QUANTICK_FRVP_FOLD_BUDGET` capture hook that
//! overrides it.

#[cfg(test)]
use crate::drawings::{Drawings, FrvpPayload};
#[cfg(test)]
use crate::state::ChartState;
#[cfg(test)]
use quantick_anchored_studies::RangeProfile as FrvpCache;

pub use quantick_drawings::frvp::{DEFAULT_FOLD_BUDGET, RefreshInputs, TOOL_ID, refresh};

/// The budget the *process* was launched with: [`DEFAULT_FOLD_BUDGET`], or
/// whatever `QUANTICK_FRVP_FOLD_BUDGET` names.
///
/// The override is not a preference knob, it is the door onto a *state*. At a
/// budget of one bar a fold advances one bar per frame, so the filling
/// profile and its progress line stay on screen as long as an operator — or a
/// capture run — needs to look at them; without it that state lasts a third of
/// a second and no screenshot can be aimed at it. A non-positive or
/// unparseable value is refused rather than guessed, and the default stands.
///
/// Read once per process by the pane, which then hands the number to every
/// refresh ([`RefreshInputs::budget`]) — so the fold never touches the
/// environment on a frame, and a test states the budget it means instead of
/// inheriting whatever the shell exported.
#[must_use]
pub fn fold_budget() -> usize {
    static BUDGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        // A capture hook: compiled only with the scenario harness (or under
        // test); a default build folds at the default budget.
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(budget) = crate::hooks::captured::var("QUANTICK_FRVP_FOLD_BUDGET")
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|budget| *budget > 0)
        {
            return budget;
        }
        DEFAULT_FOLD_BUDGET
    })
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_FRVP_FOLD_BUDGET"];

#[cfg(test)]
mod tests;
