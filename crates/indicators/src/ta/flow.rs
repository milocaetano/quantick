//! Momentum / flow kernels: `ta.cum`, `ta.change`, `ta.mom`, `ta.rsi`,
//! `ta.tr`, `ta.atr`, `ta.crossover`, `ta.crossunder`, `ta.cross`.

use super::Window;
use super::smooth::Rma;

/// `ta.cum(src)` — running sum from the first bar, no warmup.
///
/// NaN poisons the total permanently: a running total that silently skipped
/// an input would be a lie (data honesty). The host's shared `cvd` avoids
/// this by construction — bar deltas are never NaN.
#[derive(Debug, Clone)]
pub struct Cum {
    sum: f64,
}

impl Cum {
    /// A running total starting at zero.
    #[must_use]
    pub fn new() -> Self {
        Self { sum: 0.0 }
    }

    /// Advance one bar; returns the total including `x`.
    pub fn push(&mut self, x: f64) -> f64 {
        self.sum += x;
        self.sum
    }
}

impl Default for Cum {
    fn default() -> Self {
        Self::new()
    }
}

/// `ta.change(src, n)` — `src − src[n]` (default `n = 1`); NaN until `n + 1`
/// inputs have been seen. NaN inputs propagate naturally through the
/// subtraction.
#[derive(Debug, Clone)]
pub struct Change {
    window: Window,
}

impl Change {
    /// # Panics
    ///
    /// Panics if `n == 0` — `src − src[0]` is identically zero, which is a
    /// bug in the caller, not a series.
    #[must_use]
    pub fn new(n: usize) -> Self {
        assert!(n >= 1, "change lookback must be >= 1, got {n}");
        Self {
            window: Window::new(n + 1),
        }
    }

    /// Advance one bar; returns `x − x[n]`.
    pub fn push(&mut self, x: f64) -> f64 {
        self.window.push(x);
        match self.window.oldest() {
            Some(old) => x - old,
            None => f64::NAN,
        }
    }
}

/// `ta.mom(src, len)` — momentum is exactly `change(src, len)`.
pub type Mom = Change;

/// `ta.rsi(src, len)` — `100 − 100 / (1 + rma(up, len) / rma(down, len))`
/// over the bar-to-bar changes of `src`.
///
/// Edge values fall straight out of IEEE arithmetic, no special cases: all
/// gains → 100, all losses → 0, a fully flat window → NaN (0/0 — an RSI of
/// nothing is honestly undefined).
#[derive(Debug, Clone)]
pub struct Rsi {
    prev: f64,
    up: Rma,
    down: Rma,
}

impl Rsi {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            prev: f64::NAN,
            up: Rma::new(len),
            down: Rma::new(len),
        }
    }

    /// Advance one bar; NaN until `len + 1` inputs seeded the smoothers.
    /// A NaN input returns NaN and holds all state (the recursive-kernel
    /// rule).
    pub fn push(&mut self, x: f64) -> f64 {
        if x.is_nan() {
            return f64::NAN;
        }
        if self.prev.is_nan() {
            self.prev = x;
            return f64::NAN;
        }
        let chg = x - self.prev;
        self.prev = x;
        let up = self.up.push(chg.max(0.0));
        let down = self.down.push((-chg).max(0.0));
        100.0 - 100.0 / (1.0 + up / down)
    }
}

/// `ta.tr(handle_na)` — true range:
/// `max(high − low, |high − close[1]|, |low − close[1]|)`.
///
/// On the first bar there is no previous close; `handle_na = true` degrades
/// honestly to `high − low`, `false` reports NaN.
#[derive(Debug, Clone)]
pub struct Tr {
    prev_close: f64,
    handle_na: bool,
}

impl Tr {
    /// `handle_na` chooses the first-bar behavior (see type docs).
    #[must_use]
    pub fn new(handle_na: bool) -> Self {
        Self {
            prev_close: f64::NAN,
            handle_na,
        }
    }

    /// Advance one bar with the bar's high, low and close.
    pub fn push(&mut self, high: f64, low: f64, close: f64) -> f64 {
        let result = if self.prev_close.is_nan() {
            if self.handle_na { high - low } else { f64::NAN }
        } else {
            (high - low)
                .max((high - self.prev_close).abs())
                .max((low - self.prev_close).abs())
        };
        // A NaN close holds the previous close rather than erasing it —
        // the recursive-kernel skip-and-hold rule.
        if !close.is_nan() {
            self.prev_close = close;
        }
        result
    }
}

/// `ta.atr(len)` — `rma(tr(true), len)`.
#[derive(Debug, Clone)]
pub struct Atr {
    tr: Tr,
    rma: Rma,
}

impl Atr {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            tr: Tr::new(true),
            rma: Rma::new(len),
        }
    }

    /// Advance one bar with the bar's high, low and close.
    pub fn push(&mut self, high: f64, low: f64, close: f64) -> f64 {
        let tr = self.tr.push(high, low, close);
        self.rma.push(tr)
    }
}

/// `ta.crossover(a, b)` — true when `a > b` on this bar and `a <= b` on the
/// previous one. Any NaN involved compares false (IEEE), which is Pine's
/// rule too.
#[derive(Debug, Clone)]
pub struct CrossOver {
    prev_a: f64,
    prev_b: f64,
}

impl CrossOver {
    /// Fresh state: the first bar can never signal (no previous values).
    #[must_use]
    pub fn new() -> Self {
        Self {
            prev_a: f64::NAN,
            prev_b: f64::NAN,
        }
    }

    /// Advance one bar with both series' current values.
    pub fn push(&mut self, a: f64, b: f64) -> bool {
        let fired = a > b && self.prev_a <= self.prev_b;
        self.prev_a = a;
        self.prev_b = b;
        fired
    }
}

impl Default for CrossOver {
    fn default() -> Self {
        Self::new()
    }
}

/// `ta.crossunder(a, b)` — true when `a < b` on this bar and `a >= b` on the
/// previous one. NaN rules as in [`CrossOver`].
#[derive(Debug, Clone)]
pub struct CrossUnder {
    prev_a: f64,
    prev_b: f64,
}

impl CrossUnder {
    /// Fresh state: the first bar can never signal.
    #[must_use]
    pub fn new() -> Self {
        Self {
            prev_a: f64::NAN,
            prev_b: f64::NAN,
        }
    }

    /// Advance one bar with both series' current values.
    pub fn push(&mut self, a: f64, b: f64) -> bool {
        let fired = a < b && self.prev_a >= self.prev_b;
        self.prev_a = a;
        self.prev_b = b;
        fired
    }
}

impl Default for CrossUnder {
    fn default() -> Self {
        Self::new()
    }
}

/// `ta.cross(a, b)` — true when the series crossed in either direction this
/// bar. NaN rules as in [`CrossOver`].
#[derive(Debug, Clone)]
pub struct Cross {
    over: CrossOver,
    under: CrossUnder,
}

impl Cross {
    /// Fresh state: the first bar can never signal.
    #[must_use]
    pub fn new() -> Self {
        Self {
            over: CrossOver::new(),
            under: CrossUnder::new(),
        }
    }

    /// Advance one bar with both series' current values.
    pub fn push(&mut self, a: f64, b: f64) -> bool {
        // Both sub-kernels must advance every bar; `|` (not `||`) keeps the
        // second from being short-circuited into stale state.
        self.over.push(a, b) | self.under.push(a, b)
    }
}

impl Default for Cross {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cum_has_no_warmup() {
        let mut cum = Cum::new();
        assert_eq!(cum.push(1.5), 1.5);
        assert_eq!(cum.push(-2.0), -0.5);
    }

    #[test]
    fn cum_nan_poisons_permanently() {
        let mut cum = Cum::new();
        cum.push(1.0);
        assert!(cum.push(f64::NAN).is_nan());
        assert!(cum.push(1.0).is_nan(), "a skipped input would be a lie");
    }

    #[test]
    fn change_needs_n_plus_one_inputs() {
        let mut chg = Change::new(2);
        assert!(chg.push(10.0).is_nan());
        assert!(chg.push(11.0).is_nan());
        assert_eq!(chg.push(14.0), 4.0, "14 - 10");
        assert_eq!(chg.push(11.0), 0.0, "11 - 11");
    }

    #[test]
    fn rsi_saturates_and_stays_honest() {
        // len 2: three rising closes seed both smoothers.
        let mut rsi = Rsi::new(2);
        assert!(rsi.push(1.0).is_nan());
        assert!(rsi.push(2.0).is_nan(), "only one change so far");
        assert_eq!(rsi.push(3.0), 100.0, "all gains");

        let mut falling = Rsi::new(2);
        falling.push(3.0);
        falling.push(2.0);
        assert_eq!(falling.push(1.0), 0.0, "all losses");

        let mut flat = Rsi::new(2);
        flat.push(5.0);
        flat.push(5.0);
        assert!(flat.push(5.0).is_nan(), "flat window: 0/0 is undefined");
    }

    #[test]
    fn rsi_mixed_moves_land_between() {
        // len 2, closes 1, 3, 2: changes +2, -1.
        // up rma seed = (2 + 0)/2 = 1; down rma seed = (0 + 1)/2 = 0.5
        // rsi = 100 - 100/(1 + 1/0.5) = 100 - 100/3
        let mut rsi = Rsi::new(2);
        rsi.push(1.0);
        rsi.push(3.0);
        assert_eq!(rsi.push(2.0), 100.0 - 100.0 / 3.0);
    }

    #[test]
    fn tr_first_bar_honors_handle_na() {
        let mut with = Tr::new(true);
        assert_eq!(with.push(10.0, 8.0, 9.0), 2.0, "degrades to high - low");
        let mut without = Tr::new(false);
        assert!(without.push(10.0, 8.0, 9.0).is_nan());
    }

    #[test]
    fn tr_uses_previous_close_for_gaps() {
        let mut tr = Tr::new(true);
        tr.push(10.0, 8.0, 9.0);
        // Gap up: high-low = 2, |high - prev_close| = 4, |low - prev_close| = 2.
        assert_eq!(tr.push(13.0, 11.0, 12.0), 4.0);
        // Gap down: |low - prev_close| dominates: |7 - 12| = 5.
        assert_eq!(tr.push(9.0, 7.0, 8.0), 5.0);
    }

    #[test]
    fn atr_is_rma_of_true_range() {
        // len 2, dyadic: TRs 2.0 and 4.0 seed rma = 3.0; next TR 5.0 -> 4.0.
        let mut atr = Atr::new(2);
        assert!(atr.push(10.0, 8.0, 9.0).is_nan());
        assert_eq!(atr.push(13.0, 11.0, 12.0), 3.0);
        assert_eq!(atr.push(9.0, 7.0, 8.0), 4.0);
    }

    #[test]
    fn crossover_fires_on_the_crossing_bar_only() {
        let mut co = CrossOver::new();
        assert!(!co.push(1.0, 2.0), "below");
        assert!(co.push(3.0, 2.0), "crossed above");
        assert!(!co.push(4.0, 2.0), "still above, no re-fire");

        let mut touch = CrossOver::new();
        touch.push(2.0, 2.0);
        assert!(touch.push(3.0, 2.0), "equal counts as 'was not above'");
    }

    #[test]
    fn crossunder_mirrors_crossover() {
        let mut cu = CrossUnder::new();
        assert!(!cu.push(3.0, 2.0));
        assert!(cu.push(1.0, 2.0));
        assert!(!cu.push(0.5, 2.0));
    }

    #[test]
    fn cross_fires_both_directions() {
        let mut cross = Cross::new();
        assert!(!cross.push(1.0, 2.0));
        assert!(cross.push(3.0, 2.0), "up-cross");
        assert!(cross.push(1.0, 2.0), "down-cross");
    }

    #[test]
    fn nan_never_signals_a_cross() {
        let mut co = CrossOver::new();
        co.push(1.0, 2.0);
        assert!(!co.push(f64::NAN, 2.0));
        assert!(!co.push(3.0, 2.0), "previous a is NaN: comparison false");
        assert!(!co.push(4.0, 2.0), "already above on a real bar: no fire");

        // The same series without the NaN *does* fire on the 3.0 bar — the
        // NaN suppressed exactly one signal, nothing else.
        let mut clean = CrossOver::new();
        clean.push(1.0, 2.0);
        assert!(clean.push(3.0, 2.0));
    }
}
