//! The force-bar ruler: is this closed bar an institutional push?
//!
//! The ruler is the one the trader already trusts on the chart — the
//! embedded `force_bar.pine` (itself a port of the TradingView original in
//! `.claude/refs/`): a bar is *force* when its body sits inside a closed
//! band of `min_factor`× to `max_factor`× the simple average of the last
//! `window` bodies, **including the bar being judged**, exactly like
//! `ta.sma(body, window)` on the closing bar. A body above the band is
//! *exhaustion* — too big to chase, by design — and a body below it is
//! quiet. The average needs a full window before any verdict counts, and a
//! flat average (all-doji history) yields no verdict at all rather than a
//! division by zero dressed as one.

use std::collections::VecDeque;

use quantick_engine::{Bar, Side};
use rust_decimal::Decimal;

/// The force-bar band, mirroring the `force_bar.pine` inputs — plus one
/// gate the script never needed on time candles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForceParams {
    /// How many bodies the average looks back over (the script's
    /// "Periodo da media"). Must be at least 1.
    pub window: usize,
    /// Lower edge of the band, as a multiple of the average body.
    pub min_factor: Decimal,
    /// Upper edge of the band (inclusive). Above it the bar is exhaustion,
    /// not force.
    pub max_factor: Decimal,
    /// Absolute floor on the body, in price units. Zero disables it.
    ///
    /// The relative band alone is honest on time candles but promiscuous
    /// on activity-cut bars: a volume bar carries the same volume as every
    /// neighbour by construction, congestion shrinks the average body, and
    /// a 35-point body reads "1.7× force". Measured on a WINV26 session,
    /// the bare band marked 247 of 1,355 bars as force; a 100-point floor
    /// left 7. An elephant has a size, not only a ratio.
    pub min_body: Decimal,
}

impl ForceParams {
    /// The script's shipped defaults: body between 1.5× and 2.5× the
    /// average of 20 bodies, no absolute floor — faithful to the
    /// TradingView original. Consumers pick their own floor per
    /// instrument (the app's preset form defaults to 100 points).
    #[must_use]
    pub fn default_band() -> Self {
        Self {
            window: 20,
            min_factor: Decimal::new(15, 1),
            max_factor: Decimal::new(25, 1),
            min_body: Decimal::ZERO,
        }
    }
}

/// A closed bar the ruler judged to be force, with the measurements the
/// projection needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForceBar {
    /// Direction of the push: `Buy` when the bar closed above its open.
    pub side: Side,
    /// `|close - open|`.
    pub body: Decimal,
    /// `high - low` — the projection ruler for brackets.
    pub range: Decimal,
    /// `body / average body`, the band position that made it force.
    pub ratio: Decimal,
}

/// Why a closed bar did or did not fire, for badges and tooltips — the
/// data-honesty companion to [`ForceWindow::classify`]. A strategy that
/// stays silent about *why* it is not firing reads as broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarVerdict {
    /// Fewer than `window` bodies seen; the average is not real yet.
    Warmup {
        seen: usize,
        window: usize,
    },
    /// The average body is zero (an all-doji window): no ratio exists.
    FlatAverage,
    /// The bar has no direction (`close == open`), so it cannot push.
    NoSide,
    /// Body below the band.
    Quiet {
        ratio: Decimal,
    },
    Force(ForceBar),
    /// Ratio inside the band but body under the absolute floor — the bar
    /// the *relative* ruler would call force and the elephant gate holds.
    /// Distinct from [`BarVerdict::Quiet`] so the badge can say which rule
    /// held it: "quiet 1.7×" over a bar visibly inside the band reads as a
    /// broken ruler.
    UnderFloor {
        ratio: Decimal,
        body: Decimal,
    },
    /// Body above the band — too big to chase.
    Exhaustion {
        side: Side,
        ratio: Decimal,
    },
}

/// Upper bound on the *pre-allocation* a window asks for, not on the
/// window itself — far above any real ruler (the shipped input caps at
/// 500), low enough that a corrupt config cannot turn a click into a
/// multi-gigabyte allocation.
const PREALLOC_CAP: usize = 4096;

/// Incremental body window: the running state behind the ruler.
///
/// Push every closed bar exactly once, in series order; the verdict for a
/// bar includes that bar's own body in the average, like the script's
/// `ta.sma` on the closing bar.
#[derive(Debug, Clone)]
pub struct ForceWindow {
    params: ForceParams,
    bodies: VecDeque<Decimal>,
    sum: Decimal,
}

impl ForceWindow {
    /// A fresh window. `window == 0` is clamped to 1: an average over
    /// nothing is not a thing this ruler pretends to compute.
    #[must_use]
    pub fn new(params: ForceParams) -> Self {
        let params = ForceParams {
            window: params.window.max(1),
            ..params
        };
        Self {
            // Capacity is a hint, never a promise to a hand-edited config:
            // an absurd window still *works* (the deque grows), it just
            // does not pre-allocate the absurdity.
            bodies: VecDeque::with_capacity(params.window.min(PREALLOC_CAP)),
            sum: Decimal::ZERO,
            params,
        }
    }

    #[must_use]
    pub fn params(&self) -> &ForceParams {
        &self.params
    }

    /// Bodies currently in the window (for warmup badges).
    #[must_use]
    pub fn seen(&self) -> usize {
        self.bodies.len()
    }

    /// Fold one closed bar in and judge it.
    pub fn classify(&mut self, bar: &Bar) -> BarVerdict {
        let body = (bar.close - bar.open).abs();
        self.bodies.push_back(body);
        self.sum = self.sum.saturating_add(body);
        if self.bodies.len() > self.params.window
            && let Some(evicted) = self.bodies.pop_front()
        {
            self.sum -= evicted;
        }

        if self.bodies.len() < self.params.window {
            return BarVerdict::Warmup {
                seen: self.bodies.len(),
                window: self.params.window,
            };
        }
        let average = self.sum / Decimal::from(self.params.window as u64);
        if average <= Decimal::ZERO {
            return BarVerdict::FlatAverage;
        }
        let ratio = body / average;
        let side = if bar.close > bar.open {
            Side::Buy
        } else if bar.close < bar.open {
            Side::Sell
        } else {
            return BarVerdict::NoSide;
        };
        let (min, max) = ordered_band(&self.params);
        if ratio > max {
            BarVerdict::Exhaustion { side, ratio }
        } else if ratio < min {
            BarVerdict::Quiet { ratio }
        } else if body >= self.params.min_body {
            BarVerdict::Force(ForceBar {
                side,
                body,
                range: bar.high - bar.low,
                ratio,
            })
        } else {
            // Inside the band but under the absolute floor: a ratio without
            // a size is not an elephant — and the verdict says which rule
            // held it, because "quiet" over a band-clearing bar is a lie.
            BarVerdict::UnderFloor { ratio, body }
        }
    }
}

/// The script tolerates swapped band edges (`math.min`/`math.max` on the
/// inputs); so does the ruler.
fn ordered_band(params: &ForceParams) -> (Decimal, Decimal) {
    if params.min_factor <= params.max_factor {
        (params.min_factor, params.max_factor)
    } else {
        (params.max_factor, params.min_factor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    /// A closed bar with the given open/close (and a high/low wrapping them
    /// by one point either side, so range = |body| + 2).
    fn bar(open: &str, close: &str) -> Bar {
        let open = dec(open);
        let close = dec(close);
        Bar {
            open_time: 0,
            close_time: 0,
            open,
            high: open.max(close) + Decimal::ONE,
            low: open.min(close) - Decimal::ONE,
            close,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    fn window(len: usize, min: &str, max: &str) -> ForceWindow {
        ForceWindow::new(ForceParams {
            window: len,
            min_factor: dec(min),
            max_factor: dec(max),
            min_body: Decimal::ZERO,
        })
    }

    #[test]
    fn warmup_until_the_window_is_full() {
        let mut w = window(3, "1.5", "2.5");
        assert_eq!(
            w.classify(&bar("100", "101")),
            BarVerdict::Warmup { seen: 1, window: 3 }
        );
        assert_eq!(
            w.classify(&bar("101", "102")),
            BarVerdict::Warmup { seen: 2, window: 3 }
        );
        // Third bar: window full, verdict is real (ratio 1 → quiet).
        assert_eq!(
            w.classify(&bar("102", "103")),
            BarVerdict::Quiet {
                ratio: Decimal::ONE
            }
        );
    }

    #[test]
    fn the_judged_bar_is_inside_its_own_average_like_ta_sma() {
        // Bodies 1, 1, then 4: average = (1 + 1 + 4) / 3 = 2, ratio = 2.
        // Were the judged bar excluded (average 1), the ratio would be 4 and
        // the verdict exhaustion — this pins the ta.sma semantics.
        let mut w = window(3, "1.5", "2.5");
        w.classify(&bar("100", "101"));
        w.classify(&bar("101", "102"));
        match w.classify(&bar("102", "106")) {
            BarVerdict::Force(force) => {
                assert_eq!(force.ratio, dec("2"));
                assert_eq!(force.side, Side::Buy);
                assert_eq!(force.body, dec("4"));
                assert_eq!(force.range, dec("6"));
            }
            other => panic!("expected force, got {other:?}"),
        }
    }

    #[test]
    fn the_band_is_closed_and_above_it_is_exhaustion() {
        // Bodies 1, 1, 1, then 9 over window 4: sum 12, average 3, ratio 3.
        let mut w = window(4, "1.5", "2.5");
        for _ in 0..3 {
            w.classify(&bar("100", "101"));
        }
        match w.classify(&bar("100", "91")) {
            BarVerdict::Exhaustion { side, ratio } => {
                assert_eq!(side, Side::Sell);
                assert_eq!(ratio, dec("3"));
            }
            other => panic!("expected exhaustion, got {other:?}"),
        }
    }

    #[test]
    fn exactly_the_band_edges_count_as_force() {
        // Bodies 2, 2, 2, X with window 4. For ratio r: X = 6r / (4 - r).
        // r = 1.5 → X = 3.6; r = 2.5 → X = 10.
        let mut w = window(4, "1.5", "2.5");
        for _ in 0..3 {
            w.classify(&bar("100", "102"));
        }
        match w.classify(&bar("100", "103.6")) {
            BarVerdict::Force(force) => assert_eq!(force.ratio, dec("1.5")),
            other => panic!("expected force at the lower edge, got {other:?}"),
        }

        let mut w = window(4, "1.5", "2.5");
        for _ in 0..3 {
            w.classify(&bar("100", "102"));
        }
        match w.classify(&bar("100", "110")) {
            BarVerdict::Force(force) => assert_eq!(force.ratio, dec("2.5")),
            other => panic!("expected force at the upper edge, got {other:?}"),
        }
    }

    #[test]
    fn a_doji_has_no_side_and_an_all_doji_window_has_no_average() {
        let mut w = window(2, "1.5", "2.5");
        w.classify(&bar("100", "101"));
        assert_eq!(w.classify(&bar("100", "100")), BarVerdict::NoSide);

        let mut w = window(2, "1.5", "2.5");
        w.classify(&bar("100", "100"));
        assert_eq!(w.classify(&bar("100", "100")), BarVerdict::FlatAverage);
    }

    #[test]
    fn the_window_slides_and_evicts_old_bodies() {
        // Window 2: bodies 8, 2 → avg 5; then 2, 3 → avg 2.5, ratio 1.2.
        let mut w = window(2, "1.1", "2.5");
        w.classify(&bar("100", "108"));
        w.classify(&bar("100", "102"));
        match w.classify(&bar("100", "103")) {
            BarVerdict::Force(force) => assert_eq!(force.ratio, dec("1.2")),
            other => panic!("expected force after eviction, got {other:?}"),
        }
    }

    /// The absolute floor: a ratio without a size is not an elephant. In
    /// congestion the average body shrinks and modest bars clear the
    /// relative band — measured on a real WINV26 session, the bare band
    /// marked 247 of 1,355 volume bars as force; this gate is what turns
    /// the ruler back into "elephant".
    #[test]
    fn the_body_floor_holds_quiet_what_the_band_alone_would_call_force() {
        let mut gated = ForceWindow::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_body: dec("100"),
        });
        // Bodies 30, 30, then 60: ratio 1.5 — band says force, floor says
        // no, and the verdict names the floor rather than calling it quiet.
        gated.classify(&bar("100", "130"));
        gated.classify(&bar("130", "160"));
        assert_eq!(
            gated.classify(&bar("160", "220")),
            BarVerdict::UnderFloor {
                ratio: dec("1.5"),
                body: dec("60"),
            },
            "60 points of body is not an elephant under a 100-point floor"
        );

        // Bodies 100, 100, then 200: ratio 1.5 and body 200 — both gates pass.
        let mut gated = ForceWindow::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_body: dec("100"),
        });
        gated.classify(&bar("1000", "1100"));
        gated.classify(&bar("1100", "1200"));
        match gated.classify(&bar("1200", "1400")) {
            BarVerdict::Force(force) => {
                assert_eq!(force.body, dec("200"));
                assert_eq!(force.ratio, dec("1.5"));
            }
            other => panic!("a 200-point body at 1.5x is force, got {other:?}"),
        }
    }

    #[test]
    fn swapped_band_edges_behave_like_the_scripts_min_max() {
        let mut swapped = window(3, "2.5", "1.5");
        let mut straight = window(3, "1.5", "2.5");
        for b in [bar("100", "101"), bar("101", "102"), bar("102", "106")] {
            assert_eq!(swapped.classify(&b), straight.classify(&b));
        }
    }
}
