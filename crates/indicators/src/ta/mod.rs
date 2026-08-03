//! Incremental `ta.*` kernels.
//!
//! Every kernel is a small `Clone` struct advanced one bar at a time:
//! `push(x) -> f64`, amortized O(1) per bar. **No kernel re-scans its window
//! on push** — windowed kernels keep running sums plus a ring of the last
//! `len` inputs; `highest`/`lowest` use a monotonic deque. This is what makes
//! the per-bar commit budget (§5 of the plan) achievable.
//!
//! Two consequences of that choice, stated because they are easy to assume
//! away:
//!
//! - **`Clone` is cheap, not free.** A windowed kernel owns a `Vec<f64>` of
//!   `len`, so a preview snapshot of `ta.sma(close, 200)` copies 1.6 KB; the
//!   extremes additionally carry a deque. Recursive kernels (`ema`, `rma`)
//!   really are a few `f64`s. Size a preview budget from the windows in play,
//!   not from "a kernel is small".
//! - **A running sum carries its whole history.** The value at bar N is a
//!   function of every input pushed, not only of the `len` still in the
//!   window, so a kernel replayed from a different starting bar — history
//!   paging, a trimmed backlog, a replay seek — can differ in the last ulps
//!   from one fed the full stream. That is the price of not re-scanning;
//!   determinism holds for a *given* sequence of pushes, which is what the
//!   golden tests replay.
//!
//! # Semantics (ours, documented — TradingView parity is a non-goal)
//!
//! - **Warmup**: windowed kernels return `na` (NaN) until `len` inputs have
//!   been seen; `ema`/`rma` seed with the SMA of the first `len` values.
//! - **NaN, windowed kernels**: a NaN input occupies its window slot and the
//!   result is NaN while any NaN remains in the window; once it slides out,
//!   values resume. (Running sums substitute 0.0 for NaN internally and gate
//!   the output on a NaN count, so recovery is exact.)
//! - **NaN, recursive kernels** (`ema`, `rma`, and `rsi`/`atr` built on
//!   them): a NaN input returns NaN and leaves the state unchanged — a NaN
//!   folded into a recursive average would poison every future bar, which
//!   helps nobody; skip-and-hold is recoverable and honest.
//! - **NaN, `cum`**: poisons permanently (a running total that silently
//!   skipped an input would be a lie).
//! - Comparisons with NaN are `false` (IEEE), which is exactly Pine's rule
//!   for `crossover`/`crossunder`.

mod flow;
mod pivot;
mod smooth;
mod window;

pub use flow::{Atr, Change, Cross, CrossOver, CrossUnder, Cum, Mom, Rsi, Tr};
pub use pivot::{PivotHigh, PivotLow};
pub use smooth::{Ema, Rma, Sma, Vwma, Wma};
pub use window::{
    BarsSince, Highest, HighestBars, Lowest, LowestBars, Stdev, ValueWhen, WindowSum,
};

/// Ring of the last `len` inputs with NaN bookkeeping — the shared chassis of
/// every windowed kernel. Tracks how many NaNs currently sit in the window so
/// kernels can gate their output without ever re-scanning.
#[derive(Debug, Clone)]
pub(crate) struct Window {
    buf: Vec<f64>,
    /// Next write position; when the window is full this is also the oldest
    /// element's slot.
    head: usize,
    filled: usize,
    nan_in_window: usize,
    /// How many non-zero values sit in the window. A running sum cannot
    /// answer "is every input zero?": eviction leaves a rounding residue, so
    /// `sum == 0.0` reads false for a window of zeros whose neighbours were
    /// added and subtracted in floating point. Counting is exact.
    nonzero_in_window: usize,
}

impl Window {
    /// # Panics
    ///
    /// Panics if `len == 0` — a zero-length window is meaningless, and
    /// silently coercing it would violate the data-honesty rule.
    pub(crate) fn new(len: usize) -> Self {
        assert!(len >= 1, "window length must be >= 1, got {len}");
        Self {
            buf: vec![0.0; len],
            head: 0,
            filled: 0,
            nan_in_window: 0,
            nonzero_in_window: 0,
        }
    }

    /// Push `x`, returning the value it evicted once the window is full.
    pub(crate) fn push(&mut self, x: f64) -> Option<f64> {
        let evicted = if self.filled == self.buf.len() {
            Some(self.buf[self.head])
        } else {
            self.filled += 1;
            None
        };
        if let Some(e) = evicted {
            if e.is_nan() {
                self.nan_in_window -= 1;
            } else if e != 0.0 {
                self.nonzero_in_window -= 1;
            }
        }
        if x.is_nan() {
            self.nan_in_window += 1;
        } else if x != 0.0 {
            self.nonzero_in_window += 1;
        }
        self.buf[self.head] = x;
        self.head = (self.head + 1) % self.buf.len();
        evicted
    }

    /// The oldest value in the window, only once the window is full (that is
    /// the only moment "oldest" is well-defined for a fixed-length lookback).
    pub(crate) fn oldest(&self) -> Option<f64> {
        if self.is_full() {
            Some(self.buf[self.head])
        } else {
            None
        }
    }

    pub(crate) fn is_full(&self) -> bool {
        self.filled == self.buf.len()
    }

    pub(crate) fn has_nan(&self) -> bool {
        self.nan_in_window > 0
    }

    /// True while at least one non-zero number sits in the window.
    pub(crate) fn has_nonzero(&self) -> bool {
        self.nonzero_in_window > 0
    }

    pub(crate) fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub(crate) fn filled(&self) -> usize {
        self.filled
    }
}

/// NaN → 0.0, the substitution windowed running sums use internally (output
/// is gated on the NaN count, so the substitute never leaks).
pub(crate) fn zero_if_nan(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_evicts_in_fifo_order() {
        let mut w = Window::new(2);
        assert_eq!(w.push(1.0), None);
        assert_eq!(w.push(2.0), None);
        assert!(w.is_full());
        assert_eq!(w.oldest(), Some(1.0));
        assert_eq!(w.push(3.0), Some(1.0));
        assert_eq!(w.oldest(), Some(2.0));
    }

    #[test]
    fn window_tracks_nans_in_and_out() {
        let mut w = Window::new(2);
        w.push(f64::NAN);
        assert!(w.has_nan());
        w.push(1.0);
        assert!(w.has_nan(), "NaN still inside the window");
        let evicted = w.push(2.0);
        assert!(evicted.unwrap().is_nan());
        assert!(!w.has_nan(), "NaN slid out");
    }

    #[test]
    #[should_panic(expected = "must be >= 1")]
    fn zero_length_window_is_rejected() {
        let _ = Window::new(0);
    }
}
