//! Windowed lookback kernels: `ta.highest`, `ta.lowest`, `ta.highestbars`,
//! `ta.lowestbars`, `ta.stdev`, `math.sum`, `ta.valuewhen`, `ta.barssince`.

use std::collections::VecDeque;

use super::{Window, zero_if_nan};

/// Shared engine of the four extreme kernels: a monotonic deque of
/// `(push index, value)` pairs, the classic O(1)-amortized sliding-window
/// max/min. The deque holds candidates in decreasing (max) or increasing
/// (min) order; the front is always the current extreme. Ties keep the most
/// recent occurrence (our documented choice for `*bars` offsets).
#[derive(Debug, Clone)]
struct ExtremeCore {
    window: Window,
    deque: VecDeque<(u64, f64)>,
    seen: u64,
    /// true = track the maximum, false = the minimum.
    is_max: bool,
}

impl ExtremeCore {
    fn new(len: usize, is_max: bool) -> Self {
        Self {
            window: Window::new(len),
            // `len + 1`: a push appends before the out-of-window entry is
            // popped, so on a monotone series the deque transiently holds one
            // more than the window. Sizing for the peak keeps the per-bar
            // path allocation-free instead of paying one regrowth.
            deque: VecDeque::with_capacity(len + 1),
            seen: 0,
            is_max,
        }
    }

    /// Advance one bar; `Some((index, value))` of the extreme when the window
    /// is full and NaN-free, else `None`.
    fn push(&mut self, x: f64) -> Option<(u64, f64)> {
        let i = self.seen;
        self.seen += 1;
        self.window.push(x);

        // NaN is never a candidate; it still occupies a window slot, and the
        // window's NaN count gates the output below.
        if !x.is_nan() {
            while let Some(&(_, v)) = self.deque.back() {
                let dominated = if self.is_max { v <= x } else { v >= x };
                if dominated {
                    self.deque.pop_back();
                } else {
                    break;
                }
            }
            self.deque.push_back((i, x));
        }

        // Drop candidates that slid out of the lookback.
        let min_index = (i + 1).saturating_sub(self.window.capacity() as u64);
        while let Some(&(j, _)) = self.deque.front() {
            if j < min_index {
                self.deque.pop_front();
            } else {
                break;
            }
        }

        if self.window.is_full() && !self.window.has_nan() {
            self.deque.front().copied()
        } else {
            None
        }
    }

    /// Offset from the current bar to the extreme: `0` when the current bar
    /// is the extreme, negative otherwise (Pine's `*bars` convention).
    fn offset(&self, extreme_index: u64) -> f64 {
        // seen was already advanced; current bar index is seen - 1.
        -((self.seen - 1 - extreme_index) as f64)
    }
}

/// `ta.highest(src, len)` — highest of the last `len` values.
#[derive(Debug, Clone)]
pub struct Highest {
    core: ExtremeCore,
}

impl Highest {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            core: ExtremeCore::new(len, true),
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        self.core.push(x).map_or(f64::NAN, |(_, v)| v)
    }
}

/// `ta.lowest(src, len)` — lowest of the last `len` values.
#[derive(Debug, Clone)]
pub struct Lowest {
    core: ExtremeCore,
}

impl Lowest {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            core: ExtremeCore::new(len, false),
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        self.core.push(x).map_or(f64::NAN, |(_, v)| v)
    }
}

/// `ta.highestbars(src, len)` — negative offset to the highest value of the
/// last `len` bars (0 = the current bar; ties → the most recent).
#[derive(Debug, Clone)]
pub struct HighestBars {
    core: ExtremeCore,
}

impl HighestBars {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            core: ExtremeCore::new(len, true),
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        match self.core.push(x) {
            Some((i, _)) => self.core.offset(i),
            None => f64::NAN,
        }
    }
}

/// `ta.lowestbars(src, len)` — negative offset to the lowest value of the
/// last `len` bars (0 = the current bar; ties → the most recent).
#[derive(Debug, Clone)]
pub struct LowestBars {
    core: ExtremeCore,
}

impl LowestBars {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            core: ExtremeCore::new(len, false),
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        match self.core.push(x) {
            Some((i, _)) => self.core.offset(i),
            None => f64::NAN,
        }
    }
}

/// `ta.stdev(src, len)` — **population** standard deviation over the window
/// (divide by `len`, not `len − 1`; documented in the dialect reference).
///
/// Incremental via running sum and sum of squares, both accumulated relative
/// to an `offset` captured from the first real input. Variance is invariant
/// under a shift, so this is the same number — but the sums then live at
/// deviation scale instead of price scale, and `sum_sq/len − mean²` stops
/// being a difference of two nearly equal large quantities. Without it, a
/// tight spread around a 1e5 price loses most of its significant digits to
/// cancellation, and the `.max(0.0)` below would be absorbing real error
/// rather than the last-ulp negative it is meant for.
#[derive(Debug, Clone)]
pub struct Stdev {
    window: Window,
    /// Shift applied to every accumulated value; NaN until the first real
    /// input sets it.
    offset: f64,
    sum: f64,
    sum_sq: f64,
}

impl Stdev {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            window: Window::new(len),
            offset: f64::NAN,
            sum: 0.0,
            sum_sq: 0.0,
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        if self.offset.is_nan() && !x.is_nan() {
            self.offset = x;
        }
        // A NaN contributes 0.0 in shifted space, on the way in and on the way
        // out — the same substitution `zero_if_nan` makes, expressed so that
        // adding a value and later evicting it cancel exactly even though the
        // offset was still unset when a leading NaN went in. The output is
        // gated on the NaN count either way, so the substitute never leaks.
        let shifted = |v: f64| if v.is_nan() { 0.0 } else { v - self.offset };
        let clean = shifted(x);
        if let Some(evicted) = self.window.push(x) {
            let e = shifted(evicted);
            self.sum -= e;
            self.sum_sq -= e * e;
        }
        self.sum += clean;
        self.sum_sq += clean * clean;
        if self.window.is_full() && !self.window.has_nan() {
            let len = self.window.capacity() as f64;
            let mean = self.sum / len;
            let variance = (self.sum_sq / len - mean * mean).max(0.0);
            variance.sqrt()
        } else {
            f64::NAN
        }
    }
}

/// `math.sum(src, len)` — windowed sum (stateful; lives in the `math`
/// namespace in scripts but is a window kernel like any other here).
#[derive(Debug, Clone)]
pub struct WindowSum {
    window: Window,
    sum: f64,
}

impl WindowSum {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            window: Window::new(len),
            sum: 0.0,
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        if let Some(evicted) = self.window.push(x) {
            self.sum -= zero_if_nan(evicted);
        }
        self.sum += zero_if_nan(x);
        if self.window.is_full() && !self.window.has_nan() {
            self.sum
        } else {
            f64::NAN
        }
    }
}

/// `ta.valuewhen(cond, src, occurrence)` — the value `src` had on the
/// `occurrence`-th most recent bar where `cond` was true (0 = the most
/// recent, which may be the current bar). NaN until that many occurrences
/// exist.
///
/// Keeps only the last `occurrence + 1` occurrence values — O(occurrence)
/// memory regardless of history length.
#[derive(Debug, Clone)]
pub struct ValueWhen {
    occurrence: usize,
    values: VecDeque<f64>,
}

impl ValueWhen {
    /// `occurrence` is 0-based, like Pine's.
    #[must_use]
    pub fn new(occurrence: usize) -> Self {
        Self {
            occurrence,
            values: VecDeque::with_capacity(occurrence + 1),
        }
    }

    /// Advance one bar with this bar's condition and source value.
    pub fn push(&mut self, cond: bool, src: f64) -> f64 {
        if cond {
            if self.values.len() == self.occurrence + 1 {
                self.values.pop_back();
            }
            self.values.push_front(src);
        }
        self.values
            .get(self.occurrence)
            .copied()
            .unwrap_or(f64::NAN)
    }
}

/// `ta.barssince(cond)` — bars since `cond` was last true (0 = true on the
/// current bar); NaN until the first occurrence.
#[derive(Debug, Clone)]
pub struct BarsSince {
    since: Option<u64>,
}

impl BarsSince {
    /// Fresh state: no occurrence seen yet.
    #[must_use]
    pub fn new() -> Self {
        Self { since: None }
    }

    /// Advance one bar with this bar's condition.
    pub fn push(&mut self, cond: bool) -> f64 {
        if cond {
            self.since = Some(0);
        } else if let Some(c) = self.since {
            self.since = Some(c + 1);
        }
        self.since.map_or(f64::NAN, |c| c as f64)
    }
}

impl Default for BarsSince {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highest_and_lowest_slide() {
        let mut hi = Highest::new(3);
        let mut lo = Lowest::new(3);
        for (x, expect_hi, expect_lo) in [
            (3.0, f64::NAN, f64::NAN),
            (1.0, f64::NAN, f64::NAN),
            (2.0, 3.0, 1.0),
            (0.5, 2.0, 0.5),
            (2.5, 2.5, 0.5),
        ] {
            let h = hi.push(x);
            let l = lo.push(x);
            if expect_hi.is_nan() {
                assert!(h.is_nan() && l.is_nan(), "warmup at {x}");
            } else {
                assert_eq!(h, expect_hi, "highest after {x}");
                assert_eq!(l, expect_lo, "lowest after {x}");
            }
        }
    }

    #[test]
    fn extreme_plateaus_prefer_the_most_recent() {
        let mut hb = HighestBars::new(3);
        hb.push(5.0);
        hb.push(5.0);
        // Window [5, 5, 1]: both fives tie; ties keep the most recent (−1).
        assert_eq!(hb.push(1.0), -1.0);
    }

    #[test]
    fn extreme_bars_offset_is_zero_on_fresh_extreme() {
        let mut hb = HighestBars::new(2);
        hb.push(1.0);
        assert_eq!(hb.push(2.0), 0.0, "current bar is the highest");
        assert_eq!(hb.push(1.5), -1.0, "the 2.0 one bar back");

        let mut lb = LowestBars::new(2);
        lb.push(2.0);
        assert_eq!(lb.push(1.0), 0.0);
        assert_eq!(lb.push(1.5), -1.0);
    }

    #[test]
    fn extremes_gate_on_nan_in_window() {
        let mut hi = Highest::new(2);
        hi.push(1.0);
        assert!(hi.push(f64::NAN).is_nan());
        assert!(hi.push(2.0).is_nan(), "NaN still in window");
        assert_eq!(hi.push(3.0), 3.0, "recovers once NaN leaves");
    }

    #[test]
    fn stdev_is_population_stdev() {
        let mut sd = Stdev::new(3);
        sd.push(3.0);
        sd.push(6.0);
        // Window [3, 6, 9]: mean 6, variance (9+0+9)/3 = 6.
        assert_eq!(sd.push(9.0), 6.0_f64.sqrt());
        // Constant window has zero deviation.
        let mut flat = Stdev::new(2);
        flat.push(4.0);
        assert_eq!(flat.push(4.0), 0.0);
    }

    #[test]
    fn stdev_keeps_its_digits_at_instrument_scale() {
        // The case the exact-arithmetic test above cannot see: a tight spread
        // on a five-figure price. `sum_sq/len` and `mean²` are both ~1.09e10
        // while the variance is 4e-4, so accumulating raw values loses the
        // answer to cancellation — ulp(1.09e10) alone is ~2e-6, five orders
        // of magnitude above what the result needs to resolve.
        let base = 104_532.0_f64;
        let mut sd = Stdev::new(4);
        for step in [0.02, -0.02, 0.02, -0.02] {
            sd.push(base + step);
        }
        let deviation = sd.push(base + 0.02);
        assert!(
            (deviation - 0.02).abs() < 1e-9,
            "expected 0.02, got {deviation} — the accumulator lost its digits"
        );
    }

    #[test]
    fn window_sum_slides_and_recovers_from_nan() {
        let mut ws = WindowSum::new(2);
        assert!(ws.push(1.0).is_nan());
        assert_eq!(ws.push(2.0), 3.0);
        assert!(ws.push(f64::NAN).is_nan());
        assert!(ws.push(4.0).is_nan());
        assert_eq!(ws.push(6.0), 10.0);
    }

    #[test]
    fn valuewhen_tracks_occurrences() {
        let mut vw = ValueWhen::new(0);
        assert!(vw.push(false, 1.0).is_nan(), "no occurrence yet");
        assert_eq!(vw.push(true, 2.0), 2.0, "current bar counts");
        assert_eq!(vw.push(false, 3.0), 2.0, "held until the next");
        assert_eq!(vw.push(true, 4.0), 4.0);

        let mut second = ValueWhen::new(1);
        second.push(true, 10.0);
        assert!(second.push(false, 0.0).is_nan(), "only one occurrence");
        assert_eq!(second.push(true, 20.0), 10.0, "one before the latest");
    }

    #[test]
    fn barssince_counts_from_the_last_occurrence() {
        let mut bs = BarsSince::new();
        assert!(bs.push(false).is_nan(), "never true yet");
        assert_eq!(bs.push(true), 0.0);
        assert_eq!(bs.push(false), 1.0);
        assert_eq!(bs.push(false), 2.0);
        assert_eq!(bs.push(true), 0.0);
    }
}
