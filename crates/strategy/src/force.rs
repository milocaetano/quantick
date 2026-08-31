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
    /// Absolute floor on the candle's full `high - low` **range**
    /// (wicks included), in price units. Zero disables it. Named to match
    /// [`ForceBar::range`], which is the same measurement on a bar that passed.
    ///
    /// The relative band alone is honest on time candles but promiscuous
    /// on activity-cut bars: a volume bar carries the same volume as every
    /// neighbour by construction, congestion shrinks the average body, and
    /// a 35-point body reads "1.7× force". An elephant has a size, not
    /// only a ratio.
    ///
    /// **The measurement that justified this floor was taken against the
    /// body, not the range.** On a WINV26 session the bare band marked 247
    /// of 1,355 bars as force and a 100-point *body* floor left 7. That
    /// figure does not carry over: a range floor admits every bar the body
    /// floor did and more, because `high - low >= |close - open|` always.
    /// Quoting 7 for this gate would be an inferred number wearing a
    /// measured one's clothes. The honest statement is that this floor is
    /// **looser than the one that was measured**, by an amount nobody has
    /// measured yet, and `a_congested_tape_admits_more_than_the_body_floor_did`
    /// shows the shape of the difference.
    ///
    /// **This floor measures the whole candle while the ratio band above
    /// measures the body**, and the mismatch is deliberate — a trader's
    /// decision, recorded rather than inferred. The two gates object to
    /// different things: the band refuses a bar that is small *next to its
    /// neighbours*, the floor refuses one that is small *in price*. A body
    /// of 95 on a candle reaching 140 is a real push held by neither.
    ///
    /// The cost is named because it is real: a wick-dominated doji reaches
    /// far and closes nowhere, so it clears this floor on reach alone. What
    /// still has to admit it is the ratio band, and a doji's body makes a
    /// small ratio, so the two gates in series stay narrower than this one
    /// read alone. A setup wanting the *body* itself to carry a minimum
    /// size needs a second floor, which this ruler does not ship until
    /// someone asks for it.
    pub min_range: Decimal,
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
            min_range: Decimal::ZERO,
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
    /// Ratio inside the band but the candle's range under the absolute
    /// floor — the bar the *relative* ruler would call force and the
    /// elephant gate holds. Distinct from [`BarVerdict::Quiet`] so the
    /// badge can say which rule held it: "quiet 1.7×" over a bar visibly
    /// inside the band reads as a broken ruler.
    ///
    /// `range` is the measurement that was refused (`high - low`), so a
    /// badge can print the number the trader would have to change. It is
    /// deliberately not the body: reporting a figure the gate did not
    /// consult is how a trader ends up tuning the wrong input.
    UnderFloor {
        ratio: Decimal,
        range: Decimal,
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
        // Read the verdict *before* folding, out of the arithmetic the fold
        // is about to perform. The closed bar's answer and the forming
        // bar's provisional one then come from one ruler rather than two
        // that agree today — see [`Self::weigh`].
        let verdict = self.weigh(bar);
        let body = (bar.close - bar.open).abs();
        self.bodies.push_back(body);
        self.sum = self.sum.saturating_add(body);
        if self.bodies.len() > self.params.window
            && let Some(evicted) = self.bodies.pop_front()
        {
            self.sum -= evicted;
        }
        verdict
    }

    /// Judge a bar **without** folding it in.
    ///
    /// This is how the bar still forming is read. The alarm needs a verdict
    /// on a bar that has not closed, and folding one in would drop a body
    /// that is still moving into the average every later bar is measured
    /// against — the ruler would be judging against its own unfinished
    /// readings. The answer is deliberately the *same* one
    /// [`Self::classify`] gives, because that method is written in terms of
    /// this one: a provisional reading from a second ruler would be a
    /// different signal wearing the same name.
    #[must_use]
    pub fn weigh(&self, bar: &Bar) -> BarVerdict {
        let body = (bar.close - bar.open).abs();
        // The candle's own range, which the absolute floor judges and the
        // bracket projection rides on. Read once so the gate and the
        // `ForceBar` it builds can never disagree about the same bar.
        let range = bar.high - bar.low;
        // What the window would hold with this bar folded in: one body
        // longer, capped at the window, oldest evicted once it is full.
        let filled = (self.bodies.len() + 1).min(self.params.window);
        let mut sum = self.sum.saturating_add(body);
        if self.bodies.len() + 1 > self.params.window
            && let Some(oldest) = self.bodies.front()
        {
            sum -= *oldest;
        }

        if filled < self.params.window {
            return BarVerdict::Warmup {
                seen: filled,
                window: self.params.window,
            };
        }
        let average = sum / Decimal::from(self.params.window as u64);
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
        } else if range >= self.params.min_range {
            BarVerdict::Force(ForceBar {
                side,
                body,
                range,
                ratio,
            })
        } else {
            // Inside the band but under the absolute floor: a ratio without
            // a size is not an elephant — and the verdict says which rule
            // held it, because "quiet" over a band-clearing bar is a lie.
            BarVerdict::UnderFloor { ratio, range }
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

    /// A bar whose shadows reach past its body, so a fixture can separate
    /// the body the ratio band reads from the candle the floor measures.
    fn wicked(open: &str, close: &str, high: &str, low: &str) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: dec(open),
            high: dec(high),
            low: dec(low),
            close: dec(close),
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
            min_range: Decimal::ZERO,
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

    /// The absolute floor: a ratio without a size is not an elephant. (The
    /// 247/1,355 figures below were measured against the *body* floor this
    /// gate replaced; see [`ForceParams::min_range`].) In
    /// congestion the average body shrinks and modest bars clear the
    /// relative band — measured on a real WINV26 session, the bare band
    /// marked 247 of 1,355 volume bars as force; this gate is what turns
    /// the ruler back into "elephant".
    #[test]
    fn the_candle_floor_holds_quiet_what_the_band_alone_would_call_force() {
        let mut gated = ForceWindow::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: dec("100"),
        });
        // Bodies 30, 30, then 60: ratio 1.5 — band says force, floor says
        // no, and the verdict names the floor rather than calling it quiet.
        gated.classify(&bar("100", "130"));
        gated.classify(&bar("130", "160"));
        assert_eq!(
            gated.classify(&bar("160", "220")),
            BarVerdict::UnderFloor {
                ratio: dec("1.5"),
                range: dec("62"),
            },
            "a 62-point candle is not an elephant under a 100-point floor"
        );

        // Bodies 100, 100, then 200: ratio 1.5 and body 200 — both gates pass.
        let mut gated = ForceWindow::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: dec("100"),
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

    /// One ruler, two ways of asking it. Weighing a bar must give the
    /// verdict folding it in would give — through the warmup, across the
    /// eviction boundary, and on every verdict the band can return —
    /// because the alarm reads the forming bar through `weigh` and the
    /// strategy reads the closed one through `classify`. A trader must
    /// never hear an alarm the bar's own close then denies for a reason
    /// no chart shows.
    #[test]
    fn weighing_a_bar_answers_exactly_what_folding_it_in_would() {
        // Bodies 1, 1, 4, 1, 1, 20 (exhaustion), 0 (doji), 2: a walk
        // through warmup, force, quiet, exhaustion, no-side, and well past
        // the point the window starts evicting.
        let tape = [
            bar("100", "101"),
            bar("101", "102"),
            bar("102", "106"),
            bar("106", "107"),
            bar("107", "108"),
            bar("108", "128"),
            bar("128", "128"),
            bar("128", "130"),
        ];
        let mut window = window(3, "1.5", "2.5");
        for (index, bar) in tape.iter().enumerate() {
            let weighed = window.weigh(bar);
            // Weighing twice must also be idempotent: nothing was folded.
            assert_eq!(weighed, window.weigh(bar), "weigh mutated the ruler");
            assert_eq!(
                weighed,
                window.classify(bar),
                "the two readings of bar {index} disagree"
            );
        }
    }

    /// The trader's own rule, and the bar it was written for: the floor
    /// measures the **candle**, not the body.
    ///
    /// A 95-point body sitting inside the band, on a candle reaching 140.
    /// Under the old body floor of 100 this was `UnderFloor` and no order
    /// was placed — the setup the trader watched go by at the top of a sell
    /// zone. The candle is plainly an elephant; the body alone was what
    /// could not see it.
    #[test]
    fn the_floor_measures_the_whole_candle_not_the_body() {
        let mut gated = ForceWindow::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: dec("100"),
        });
        // Two 30-point bodies warm the window, so the average is 30 until
        // the judged bar folds itself in.
        gated.classify(&bar("100", "130"));
        gated.classify(&bar("130", "160"));
        // Body 95 (ratio ~1.84, inside the band) on a 140-point candle.
        match gated.classify(&wicked("200", "105", "210", "70")) {
            BarVerdict::Force(force) => {
                assert_eq!(force.body, dec("95"), "the body the band judged");
                assert_eq!(force.range, dec("140"), "the candle the floor cleared");
                assert_eq!(force.side, Side::Sell, "close under open is a push down");
            }
            other => panic!(
                "a 95-point body on a 140-point candle clears a 100-point candle floor, got                  {other:?}"
            ),
        }
    }

    /// The exposure the candle floor takes on, written down as a test so it
    /// is a known position rather than a surprise.
    ///
    /// A bar that reaches far and closes nowhere clears the *floor* on reach
    /// alone, where a body floor refused it. What still has to admit it is
    /// the ratio band — and a doji's body makes a small ratio, so the two
    /// gates in series stay narrower than this one read by itself. This test
    /// is what would fail first if that second gate were ever loosened.
    #[test]
    fn a_wick_dominated_candle_clears_the_floor_and_the_band_still_refuses_it() {
        let mut gated = ForceWindow::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: dec("100"),
        });
        gated.classify(&bar("100", "130"));
        gated.classify(&bar("130", "160"));
        // Body 5 on a 200-point candle: the floor sees an elephant, the
        // band sees the doji it is.
        match gated.classify(&wicked("160", "165", "300", "100")) {
            BarVerdict::Quiet { ratio } => assert!(
                ratio < dec("1.5"),
                "the band is what refuses a candle the size floor let through, got {ratio}"
            ),
            other => panic!("a 5-point body is not force at any candle size, got {other:?}"),
        }
    }

    /// What the range floor lets through that the body floor did not, in
    /// the regime the floor was added for.
    ///
    /// The doji test above warms with 30-point bodies, so its ratio is far
    /// under the band and the *band* refuses the bar whatever the floor
    /// says — which proves the gates compose, and proves nothing about
    /// congestion. This one is congestion: a shrunken average body, so the
    /// ratio clears easily, and a bar whose body would have been refused
    /// outright. It is the cost of the trader's decision written down as an
    /// executable fact, so that "the floor is looser now" is a number
    /// somebody can read rather than a worry somebody has.
    #[test]
    fn a_congested_tape_admits_more_than_the_body_floor_did() {
        let params = ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: dec("100"),
        };
        // Congestion: two 10-point bodies, so the average is small and a
        // modest body clears the band with room to spare.
        let warm = [bar("100", "110"), bar("110", "120")];
        // Body 25 (ratio 1.67, inside the band) on a 140-point candle.
        let judged = wicked("180000", "179975", "180100", "179960");

        let mut ranged = ForceWindow::new(params.clone());
        for bar in &warm {
            ranged.classify(bar);
        }
        match ranged.classify(&judged) {
            BarVerdict::Force(force) => {
                assert_eq!(force.body, dec("25"));
                assert_eq!(force.range, dec("140"));
            }
            other => panic!("the range floor admits this bar, got {other:?}"),
        }

        // The very same bar under a floor of the same number read against
        // the body: refused. This is the whole delta, in one assertion.
        let mut bodied = ForceWindow::new(params);
        for bar in &warm {
            bodied.classify(bar);
        }
        let body_floor_would_refuse = (judged.close - judged.open).abs() < dec("100");
        assert!(
            body_floor_would_refuse,
            "the fixture must be a bar the old body floor refused, or it              proves nothing"
        );
        assert!(
            matches!(bodied.classify(&judged), BarVerdict::Force(_)),
            "and the range floor is what admits it now"
        );
    }
}
