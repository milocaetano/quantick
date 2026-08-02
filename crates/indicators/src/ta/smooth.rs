//! Smoothing kernels: `ta.sma`, `ta.ema`, `ta.rma`, `ta.wma`, `ta.vwma`.

use super::{Window, zero_if_nan};

/// `ta.sma(src, len)` — arithmetic mean of the last `len` inputs.
///
/// Incremental: running sum plus the ring, O(1) per push.
#[derive(Debug, Clone)]
pub struct Sma {
    window: Window,
    sum: f64,
}

impl Sma {
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

    /// Advance one bar; returns the mean, or NaN during warmup / while a NaN
    /// input remains in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        if let Some(evicted) = self.window.push(x) {
            self.sum -= zero_if_nan(evicted);
        }
        self.sum += zero_if_nan(x);
        if self.window.is_full() && !self.window.has_nan() {
            self.sum / self.window.capacity() as f64
        } else {
            f64::NAN
        }
    }
}

/// Shared engine of `ema`/`rma`: exponential smoothing seeded with the SMA of
/// the first `len` values (our documented seed — deterministic and testable;
/// TradingView parity is a non-goal).
///
/// NaN rule: a NaN input returns NaN and leaves the state untouched
/// (skip-and-hold) — folding NaN into the recursion would poison every
/// future bar.
#[derive(Debug, Clone)]
struct ExpSmooth {
    alpha: f64,
    len: usize,
    seed_sum: f64,
    seed_count: usize,
    /// NaN until seeded.
    value: f64,
}

impl ExpSmooth {
    fn new(len: usize, alpha: f64) -> Self {
        assert!(len >= 1, "smoothing length must be >= 1, got {len}");
        Self {
            alpha,
            len,
            seed_sum: 0.0,
            seed_count: 0,
            value: f64::NAN,
        }
    }

    fn push(&mut self, x: f64) -> f64 {
        if x.is_nan() {
            return f64::NAN;
        }
        if self.value.is_nan() {
            self.seed_sum += x;
            self.seed_count += 1;
            if self.seed_count == self.len {
                self.value = self.seed_sum / self.len as f64;
            }
            return self.value;
        }
        self.value = self.alpha * x + (1.0 - self.alpha) * self.value;
        self.value
    }
}

/// `ta.ema(src, len)` — exponential moving average, `α = 2 / (len + 1)`,
/// seed = SMA of the first `len` values, NaN until seeded.
#[derive(Debug, Clone)]
pub struct Ema {
    inner: ExpSmooth,
}

impl Ema {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            inner: ExpSmooth::new(len, 2.0 / (len as f64 + 1.0)),
        }
    }

    /// Advance one bar (see [`Ema`] for the seed and NaN rules).
    pub fn push(&mut self, x: f64) -> f64 {
        self.inner.push(x)
    }
}

/// `ta.rma(src, len)` — Wilder's smoothing, `α = 1 / len`, seed = SMA of the
/// first `len` values, NaN until seeded. The building block of `rsi`/`atr`.
#[derive(Debug, Clone)]
pub struct Rma {
    inner: ExpSmooth,
}

impl Rma {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            inner: ExpSmooth::new(len, 1.0 / len as f64),
        }
    }

    /// Advance one bar (see [`Rma`] for the seed and NaN rules).
    pub fn push(&mut self, x: f64) -> f64 {
        self.inner.push(x)
    }
}

/// `ta.wma(src, len)` — weighted moving average, linear weights `1..=len`,
/// newest heaviest.
///
/// Incremental via the classic recurrences (no window re-scan):
/// `numerator' = numerator - sum + len·x` once full, `sum' = sum - evicted + x`.
#[derive(Debug, Clone)]
pub struct Wma {
    window: Window,
    sum: f64,
    numerator: f64,
    weight_total: f64,
}

impl Wma {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            window: Window::new(len),
            sum: 0.0,
            numerator: 0.0,
            // 1 + 2 + … + len; small enough that f64 is exact.
            weight_total: (len * (len + 1) / 2) as f64,
        }
    }

    /// Advance one bar; NaN during warmup / while a NaN is in the window.
    pub fn push(&mut self, x: f64) -> f64 {
        let clean = zero_if_nan(x);
        let evicted = self.window.push(x);
        match evicted {
            // Full window: every kept value's weight drops by one (− sum),
            // the evicted value's weight-1 contribution left with it, and the
            // newcomer enters at the top weight.
            Some(e) => {
                let sum_before = self.sum;
                self.numerator =
                    self.numerator - sum_before + self.window.capacity() as f64 * clean;
                self.sum = sum_before - zero_if_nan(e) + clean;
            }
            // Warmup: existing values keep their weights (oldest = 1); the
            // newcomer takes the next weight up.
            None => {
                self.numerator += self.window.filled() as f64 * clean;
                self.sum += clean;
            }
        }
        if self.window.is_full() && !self.window.has_nan() {
            self.numerator / self.weight_total
        } else {
            f64::NAN
        }
    }
}

/// `ta.vwma(src, volume, len)` — volume-weighted moving average:
/// `sma(src·volume, len) / sma(volume, len)`.
///
/// A window whose volume sums to zero reads NaN (a volume-weighted price of
/// no volume does not exist; IEEE would say ±inf, which is a lie on a chart).
#[derive(Debug, Clone)]
pub struct Vwma {
    weighted: Sma,
    volume: Sma,
}

impl Vwma {
    /// # Panics
    ///
    /// Panics if `len == 0`.
    #[must_use]
    pub fn new(len: usize) -> Self {
        Self {
            weighted: Sma::new(len),
            volume: Sma::new(len),
        }
    }

    /// Advance one bar with the bar's source value and volume.
    pub fn push(&mut self, src: f64, volume: f64) -> f64 {
        let num = self.weighted.push(src * volume);
        let den = self.volume.push(volume);
        if den == 0.0 { f64::NAN } else { num / den }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sma_warms_up_then_slides() {
        let mut sma = Sma::new(3);
        assert!(sma.push(3.0).is_nan());
        assert!(sma.push(6.0).is_nan());
        assert_eq!(sma.push(9.0), 6.0);
        assert_eq!(sma.push(12.0), 9.0);
        assert_eq!(sma.push(3.0), 8.0);
    }

    #[test]
    fn sma_nan_poisons_only_while_in_window() {
        let mut sma = Sma::new(2);
        sma.push(1.0);
        assert!(sma.push(f64::NAN).is_nan());
        assert!(sma.push(3.0).is_nan(), "NaN still in window");
        assert_eq!(sma.push(5.0), 4.0, "recovers exactly once NaN leaves");
    }

    #[test]
    fn ema_seeds_with_sma_then_smooths() {
        // len 3 => alpha = 0.5; all values dyadic => exact assertions.
        let mut ema = Ema::new(3);
        assert!(ema.push(3.0).is_nan());
        assert!(ema.push(6.0).is_nan());
        assert_eq!(ema.push(9.0), 6.0, "seed = SMA of first 3");
        assert_eq!(ema.push(12.0), 9.0);
        assert_eq!(ema.push(15.0), 12.0);
    }

    #[test]
    fn ema_nan_skips_and_holds() {
        let mut ema = Ema::new(3);
        ema.push(3.0);
        ema.push(6.0);
        assert_eq!(ema.push(9.0), 6.0);
        assert!(ema.push(f64::NAN).is_nan());
        assert_eq!(ema.push(12.0), 9.0, "state did not advance on the NaN");
    }

    #[test]
    fn ema_nan_during_warmup_does_not_count() {
        let mut ema = Ema::new(2);
        ema.push(1.0);
        assert!(ema.push(f64::NAN).is_nan());
        assert_eq!(ema.push(3.0), 2.0, "seed = SMA of first 2 non-NaN values");
    }

    #[test]
    fn rma_uses_one_over_len() {
        // len 2 => alpha = 0.5, dyadic again.
        let mut rma = Rma::new(2);
        assert!(rma.push(2.0).is_nan());
        assert_eq!(rma.push(4.0), 3.0, "seed = SMA of first 2");
        assert_eq!(rma.push(5.0), 4.0);
    }

    #[test]
    fn wma_weights_newest_heaviest() {
        let mut wma = Wma::new(3);
        assert!(wma.push(3.0).is_nan());
        assert!(wma.push(6.0).is_nan());
        // (1*3 + 2*6 + 3*9) / 6 = 42/6
        assert_eq!(wma.push(9.0), 7.0);
        // (1*6 + 2*9 + 3*12) / 6 = 60/6
        assert_eq!(wma.push(12.0), 10.0);
        // (1*9 + 2*12 + 3*3) / 6 = 42/6
        assert_eq!(wma.push(3.0), 7.0);
    }

    #[test]
    fn wma_recovers_after_nan_leaves() {
        let mut wma = Wma::new(2);
        wma.push(1.0);
        assert!(wma.push(f64::NAN).is_nan());
        assert!(wma.push(4.0).is_nan(), "NaN still in window");
        // window [4, 6]: (1*4 + 2*6) / 3 = 16/3
        assert_eq!(wma.push(6.0), 16.0 / 3.0);
    }

    #[test]
    fn vwma_weights_by_volume() {
        let mut vwma = Vwma::new(2);
        assert!(vwma.push(10.0, 1.0).is_nan());
        // (10*1 + 20*3) / (1 + 3) = 70/4
        assert_eq!(vwma.push(20.0, 3.0), 17.5);
    }

    #[test]
    fn vwma_zero_volume_window_is_na_not_inf() {
        let mut vwma = Vwma::new(2);
        vwma.push(10.0, 0.0);
        assert!(vwma.push(20.0, 0.0).is_nan());
    }
}
