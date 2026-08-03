//! Pivot kernels: `ta.pivothigh`, `ta.pivotlow`.
//!
//! A pivot high at bar `p` is a value strictly greater than the `left` bars
//! before it and the `right` bars after it. It can only be *known* once
//! those `right` bars have printed, so the kernel returns the pivot value on
//! the confirming bar — `right` bars after the pivot itself — and NaN
//! everywhere else (plan §2.5: the pivot sits at `bar_index − right`).

use super::Window;

/// Shared engine: a full window of `left + 1 + right` values; on each push
/// (window full, NaN-free) the candidate at distance `right` from the newest
/// is compared against everything else in the window.
#[derive(Debug, Clone)]
struct PivotCore {
    window: Window,
    values: Vec<f64>,
    /// Next write position in the circular `values` (mirrors the window).
    head: usize,
    /// Slots between the oldest window entry and the candidate; the right
    /// side is implied by the window length.
    left: usize,
    is_high: bool,
}

impl PivotCore {
    fn new(left: usize, right: usize, is_high: bool) -> Self {
        let len = left + 1 + right;
        Self {
            window: Window::new(len),
            values: vec![0.0; len],
            head: 0,
            left,
            is_high,
        }
    }

    fn push(&mut self, x: f64) -> f64 {
        self.window.push(x);
        self.values[self.head] = x;
        let len = self.values.len();
        self.head = (self.head + 1) % len;
        if !self.window.is_full() || self.window.has_nan() {
            return f64::NAN;
        }
        // The candidate sits `right` slots before the newest. `head` points
        // at the oldest slot now; newest is head+len-1, candidate is
        // newest − right.
        let candidate_slot = (self.head + self.left) % len;
        let candidate = self.values[candidate_slot];
        for (slot, &value) in self.values.iter().enumerate() {
            if slot == candidate_slot {
                continue;
            }
            let beaten = if self.is_high {
                candidate > value
            } else {
                candidate < value
            };
            if !beaten {
                return f64::NAN;
            }
        }
        candidate
    }
}

/// `ta.pivothigh(src, left, right)` — the pivot value on its confirming bar,
/// NaN otherwise. Strict inequality: plateaus are not pivots.
#[derive(Debug, Clone)]
pub struct PivotHigh {
    core: PivotCore,
}

impl PivotHigh {
    /// # Panics
    ///
    /// Panics if `left + 1 + right == 0` overflows the window rule (a zero
    /// window is impossible here since the candidate itself counts).
    #[must_use]
    pub fn new(left: usize, right: usize) -> Self {
        Self {
            core: PivotCore::new(left, right, true),
        }
    }

    /// Advance one bar.
    pub fn push(&mut self, x: f64) -> f64 {
        self.core.push(x)
    }
}

/// `ta.pivotlow(src, left, right)` — mirrored [`PivotHigh`].
#[derive(Debug, Clone)]
pub struct PivotLow {
    core: PivotCore,
}

impl PivotLow {
    /// See [`PivotHigh::new`].
    #[must_use]
    pub fn new(left: usize, right: usize) -> Self {
        Self {
            core: PivotCore::new(left, right, false),
        }
    }

    /// Advance one bar.
    pub fn push(&mut self, x: f64) -> f64 {
        self.core.push(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pivot_confirms_right_bars_later() {
        // left=2, right=2 over 1,2,5,3,2: the 5 is confirmed on the last push.
        let mut ph = PivotHigh::new(2, 2);
        for x in [1.0, 2.0, 5.0, 3.0] {
            assert!(ph.push(x).is_nan());
        }
        assert_eq!(ph.push(2.0), 5.0);
        // Sliding on, no new pivot yet.
        assert!(ph.push(4.0).is_nan());
    }

    #[test]
    fn plateaus_are_not_pivots() {
        let mut ph = PivotHigh::new(1, 1);
        ph.push(1.0);
        ph.push(5.0);
        assert!(ph.push(5.0).is_nan(), "tie with the right neighbour");
        let mut strict = PivotHigh::new(1, 1);
        strict.push(1.0);
        strict.push(5.0);
        assert_eq!(strict.push(4.0), 5.0);
    }

    #[test]
    fn pivot_low_mirrors() {
        let mut pl = PivotLow::new(1, 1);
        pl.push(3.0);
        pl.push(1.0);
        assert_eq!(pl.push(2.0), 1.0);
    }

    #[test]
    fn nan_in_window_defers_confirmation() {
        let mut ph = PivotHigh::new(1, 1);
        ph.push(1.0);
        ph.push(5.0);
        assert!(ph.push(f64::NAN).is_nan());
        assert!(ph.push(2.0).is_nan(), "NaN still inside");
        // Window [2, 9, 3]: fresh pivot once clean again.
        ph.push(9.0);
        assert_eq!(ph.push(3.0), 9.0);
    }
}
