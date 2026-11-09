//! Caller-owned kernel state. Layouts are immutable; all evolving words,
//! including rings and monotonic queues, live in the supplied slice.
mod window;
use super::zero_if_nan;
use window::{full, push_window, read, write};

/// Native incremental operation represented by a packed state layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelKind {
    Sma,
    Ema,
    Rma,
    Wma,
    Vwma,
    Rsi,
    Tr,
    Atr,
    Stdev,
    Highest,
    Lowest,
    HighestBars,
    LowestBars,
    Change,
    Cum,
    CrossOver,
    CrossUnder,
    Cross,
    BarsSince,
    ValueWhen,
    PivotHigh,
    PivotLow,
    WindowSum,
    Fixnan,
}

/// Immutable description of a kernel's caller-owned word storage.
#[derive(Debug, Clone, Copy)]
pub struct KernelLayout {
    kind: KernelKind,
    length: usize,
    auxiliary: usize,
    words: usize,
}

impl KernelLayout {
    /// Length is the native lookback, except for TR (handle_na), valuewhen
    /// (occurrence), and pivots (left, with auxiliary holding right).
    /// Parameterless operations accept zero. Panics on zero lookbacks or
    /// arithmetic overflow in storage sizing.
    #[must_use]
    #[inline]
    pub fn new(kind: KernelKind, length: usize, auxiliary: usize) -> Self {
        use KernelKind::*;
        let window = |n: usize| n.checked_add(4).expect("kernel window size overflow");
        let words = match kind {
            Sma | Wma | Stdev | WindowSum => window(length).checked_add(3),
            Vwma => window(length).checked_add(3).and_then(|n| n.checked_mul(2)),
            Highest | Lowest | HighestBars | LowestBars => length
                .checked_add(1)
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(window(length)))
                .and_then(|n| n.checked_add(3)),
            Change | ValueWhen => length.checked_add(1).map(window),
            PivotHigh | PivotLow => length
                .checked_add(auxiliary)
                .and_then(|n| n.checked_add(1))
                .map(window),
            Ema | Rma => Some(3),
            Rsi => Some(7),
            Atr => Some(4),
            CrossOver | CrossUnder | Cross | BarsSince => Some(2),
            Tr | Cum | Fixnan => Some(1),
        }
        .expect("kernel state size overflow");
        if matches!(
            kind,
            Sma | Ema
                | Rma
                | Wma
                | Vwma
                | Rsi
                | Atr
                | Stdev
                | Highest
                | Lowest
                | HighestBars
                | LowestBars
                | Change
                | WindowSum
        ) {
            assert!(length >= 1, "kernel length must be >= 1");
        }
        Self {
            kind,
            length,
            auxiliary,
            words,
        }
    }
    /// Number of u64 words, including every ring and queue entry.
    #[must_use]
    #[inline]
    pub fn words(&self) -> usize {
        self.words
    }
    /// Initialize exactly this layout's state. Panics on a mismatched slice.
    pub fn initialize(&self, state: &mut [u64]) {
        assert_eq!(state.len(), self.words);
        state.fill(0);
        use KernelKind::*;
        match self.kind {
            Ema | Rma => write(state, 2, f64::NAN),
            Rsi => {
                write(state, 0, f64::NAN);
                write(state, 3, f64::NAN);
                write(state, 6, f64::NAN);
            }
            Tr | Fixnan => write(state, 0, f64::NAN),
            Atr => {
                write(state, 0, f64::NAN);
                write(state, 3, f64::NAN);
            }
            Stdev => write(state, 4 + self.length + 2, f64::NAN),
            CrossOver | CrossUnder | Cross => state.fill(f64::NAN.to_bits()),
            _ => {}
        }
    }
    /// Advance without allocation or hidden mutable state. Inputs follow native
    /// argument order; conditions use 1/0, and booleans return 1/0.
    /// Panics on a mismatched slice.
    #[inline]
    pub fn push(&self, s: &mut [u64], inputs: [f64; 3]) -> f64 {
        assert_eq!(s.len(), self.words);
        let [x, y, z] = inputs;
        let n = self.length;
        use KernelKind::*;
        match self.kind {
            Sma | Wma | Stdev | WindowSum => window::average(s, n, x, self.kind),
            Ema | Rma => exponential(s, n, x, self.kind == Ema),
            Vwma => {
                let (weighted, volume) = s.split_at_mut(n + 7);
                let num = window::average(weighted, n, x * y, Sma);
                let den = window::average(volume, n, y, Sma);
                if volume[3] != 0 { num / den } else { f64::NAN }
            }
            Rsi => {
                if x.is_nan() {
                    return f64::NAN;
                }
                let prev = read(s, 0);
                write(s, 0, x);
                if prev.is_nan() {
                    return f64::NAN;
                }
                let change = x - prev;
                let up = exponential(&mut s[1..4], n, change.max(0.0), false);
                let down = exponential(&mut s[4..7], n, (-change).max(0.0), false);
                100.0 - 100.0 / (1.0 + up / down)
            }
            Tr => true_range(s, n != 0, x, y, z),
            Atr => {
                let tr = true_range(&mut s[..1], true, x, y, z);
                exponential(&mut s[1..], n, tr, false)
            }
            Highest | Lowest | HighestBars | LowestBars => window::extreme(s, n, x, self.kind),
            Change => {
                push_window(s, n + 1, x);
                if full(s, n + 1) {
                    x - read(s, 4 + s[0] as usize)
                } else {
                    f64::NAN
                }
            }
            Cum => {
                let sum = read(s, 0) + x;
                write(s, 0, sum);
                sum
            }
            Fixnan => {
                if !x.is_nan() {
                    write(s, 0, x);
                }
                read(s, 0)
            }
            CrossOver | CrossUnder | Cross => {
                let over = x > y && read(s, 0) <= read(s, 1);
                let under = x < y && read(s, 0) >= read(s, 1);
                write(s, 0, x);
                write(s, 1, y);
                f64::from(match self.kind {
                    CrossOver => over,
                    CrossUnder => under,
                    _ => over | under,
                })
            }
            BarsSince => {
                if x != 0.0 {
                    s[0] = 1;
                    s[1] = 0;
                } else if s[0] != 0 {
                    s[1] += 1;
                }
                if s[0] != 0 { s[1] as f64 } else { f64::NAN }
            }
            ValueWhen => {
                if x != 0.0 {
                    push_window(s, n + 1, y);
                }
                if full(s, n + 1) {
                    read(s, 4 + s[0] as usize)
                } else {
                    f64::NAN
                }
            }
            PivotHigh | PivotLow => window::pivot(s, n, self.auxiliary, x, self.kind == PivotHigh),
        }
    }
}

fn exponential(s: &mut [u64], n: usize, x: f64, ema: bool) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    let mut value = read(s, 2);
    if value.is_nan() {
        let sum = read(s, 0) + x;
        write(s, 0, sum);
        s[1] += 1;
        if s[1] == n as u64 {
            value = sum / n as f64;
            write(s, 2, value);
        }
        return value;
    }
    let alpha = if ema {
        2.0 / (n as f64 + 1.0)
    } else {
        1.0 / n as f64
    };
    value = alpha * x + (1.0 - alpha) * value;
    write(s, 2, value);
    value
}

fn true_range(s: &mut [u64], handle_na: bool, high: f64, low: f64, close: f64) -> f64 {
    let prev = read(s, 0);
    let result = if prev.is_nan() {
        if handle_na { high - low } else { f64::NAN }
    } else {
        (high - low)
            .max((high - prev).abs())
            .max((low - prev).abs())
    };
    if !close.is_nan() {
        write(s, 0, close);
    }
    result
}
