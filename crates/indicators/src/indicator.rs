//! The [`Indicator`] contract — the heart of the design.
//!
//! Pine's execution model, adapted to quantick's forming bar:
//!
//! - **Commit run** ([`on_close`](Indicator::on_close)) — when a bar closes,
//!   the indicator evaluates once and every state mutation persists: plot
//!   buffers gain exactly one row, kernels advance, `var` slots keep their
//!   assignments.
//! - **Preview run** ([`preview`](Indicator::preview)) — while a bar is
//!   forming, the indicator evaluates against the partial bar *starting from
//!   the last committed state*, and every mutation is discarded afterwards.
//!   This is Pine's rollback: an EMA never advances twice inside one bar.
//!   The returned [`PreviewFrame`] is kept only for rendering.
//!
//! Both native indicators (hand-written Rust) and scripted ones (compiled
//! `.pine`) implement this trait; the host and every consumer — chart,
//! backtest, bot — see only the trait. That is the "one engine, three
//! consumers" rule applied to indicators.

use crate::bar::IndicatorBar;
use crate::input::InputSpec;
use crate::output::{PlotBuffer, PlotSpec, PreviewFrame};

/// Static description of an indicator: what it plots, what it asks the user,
/// where it renders. Fixed at load/registration time — plots are never added
/// or removed while running (Pine's top-level-`plot` rule).
#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorDescriptor {
    /// Full display name.
    pub title: String,
    /// Compact name for tight UI spots (pane label); falls back to `title`.
    pub short_title: Option<String>,
    /// True = draw over the price chart; false = own sub-pane below it.
    pub overlay: bool,
    /// The declared plots, in [`PlotId`](crate::PlotId) order.
    pub plots: Vec<PlotSpec>,
    /// The declared user inputs, in declaration order.
    pub inputs: Vec<InputSpec>,
}

/// Per-evaluation context the host provides: values that belong to the chart
/// as a whole rather than to one indicator.
#[derive(Debug)]
pub struct Ctx<'a> {
    /// 0-based index of the bar being evaluated. For a commit run this is the
    /// index the closing bar will occupy; for a preview run it is the index
    /// the forming bar would occupy (== committed bar count).
    pub bar_index: usize,
    /// The host-maintained cumulative volume delta, one value per bar up to
    /// and including the bar being evaluated (`cvd[bar_index]` is current;
    /// for previews the last element is the forming bar's staged value).
    /// Host-maintained so every indicator sees the same series without each
    /// paying for its own running sum.
    pub cvd: &'a [f64],
}

impl Ctx<'_> {
    /// The cvd of the bar being evaluated; `na` before any bar exists.
    #[must_use]
    pub fn cvd_now(&self) -> f64 {
        self.cvd.last().copied().unwrap_or(f64::NAN)
    }

    /// History read on cvd: `back = 0` is the current bar. Out of range reads
    /// `na`, consistent with every other series.
    #[must_use]
    pub fn cvd_back(&self, back: usize) -> f64 {
        self.cvd
            .len()
            .checked_sub(back + 1)
            .map_or(f64::NAN, |i| self.cvd[i])
    }
}

/// A runtime evaluation failure. One failing indicator enters an error state
/// the UI shows (bar index + message) and never poisons its neighbours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalError {
    /// Index of the bar being evaluated when the failure happened.
    pub bar_index: usize,
    /// Human-readable description (for scripts: rendered from the span-carrying
    /// script error).
    pub message: String,
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "indicator error at bar {}: {}",
            self.bar_index, self.message
        )
    }
}

impl std::error::Error for EvalError {}

/// A computation from bars to plot columns, evaluated bar by bar.
///
/// Implementations must be **deterministic**: same bars in, same plots out,
/// bit-exact — no wall clock, no randomness, no iteration-order leakage.
/// Golden tests replay fixtures twice and diff the byte output to enforce it.
pub trait Indicator {
    /// Static description: plots declared, inputs declared, overlay flag.
    fn descriptor(&self) -> &IndicatorDescriptor;

    /// The committed plot columns: one row per closed bar evaluated so far.
    /// This is the read path shared by the chart renderer, golden tests and
    /// future backtest/bot consumers.
    fn plots(&self) -> &PlotBuffer;

    /// Commit run for a closed bar. Appends exactly one row to the plot
    /// buffer — **as the last step**, so an `Err` never leaves a half-
    /// appended row behind.
    ///
    /// # Errors
    ///
    /// A runtime failure (script type error, arity violation, loop budget)
    /// puts this indicator in an error state; the host stops evaluating it
    /// and surfaces the error in the UI.
    fn on_close(&mut self, bar: &IndicatorBar, ctx: &mut Ctx<'_>) -> Result<(), EvalError>;

    /// Preview run for the forming bar. Must not observably mutate state:
    /// implementations stage into their own buffers and roll back before
    /// returning (`&mut self` is deliberate — staging is cheaper and simpler
    /// than interior mutability, but the *contract* is "no observable state
    /// change").
    ///
    /// # Errors
    ///
    /// Same failure handling as [`on_close`](Indicator::on_close).
    fn preview(
        &mut self,
        partial: &IndicatorBar,
        ctx: &mut Ctx<'_>,
    ) -> Result<PreviewFrame, EvalError>;

    /// Drop all state, ready for a full replay (spec switch, replay seek).
    fn reset(&mut self);

    /// The retained draw objects, for indicators that draw any (scripts
    /// with `line`/`box`/`label`). `None` — the default — costs nothing.
    fn objects(&self) -> Option<&crate::objects::ObjectStore> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_cvd_reads_count_back_from_current() {
        let cvd = [1.0, -2.0, 3.5];
        let ctx = Ctx {
            bar_index: 2,
            cvd: &cvd,
        };
        assert_eq!(ctx.cvd_now(), 3.5);
        assert_eq!(ctx.cvd_back(0), 3.5);
        assert_eq!(ctx.cvd_back(2), 1.0);
        assert!(ctx.cvd_back(3).is_nan(), "before history => na");
    }

    #[test]
    fn empty_cvd_reads_na() {
        let ctx = Ctx {
            bar_index: 0,
            cvd: &[],
        };
        assert!(ctx.cvd_now().is_nan());
    }

    #[test]
    fn eval_error_displays_bar_and_message() {
        let err = EvalError {
            bar_index: 41,
            message: "division of incompatible kinds".into(),
        };
        let text = err.to_string();
        assert!(text.contains("bar 41"), "{text}");
        assert!(text.contains("incompatible kinds"), "{text}");
    }
}
