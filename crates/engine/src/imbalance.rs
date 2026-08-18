//! Imbalance bars — close a bar when aggressor imbalance exceeds a dynamic
//! threshold.
//!
//! Imbalance bars (López de Prado, *Advances in Financial Machine Learning*,
//! ch. 2) sample by **information arrival** rather than by raw activity: each
//! trade contributes a signed weight `s = ±w` (positive for a taker buy), and
//! the bar closes when the running imbalance `theta = sum(s)` becomes
//! unusually large relative to what recent history says is normal. Balanced
//! two-way flow produces long bars; a one-sided burst of aggression — new
//! information hitting the market — closes a bar almost immediately, so the
//! sampling rate itself tracks information flow.
//!
//! # Units
//!
//! [`ImbalanceUnit`] picks the weight `w`, giving the book's three variants:
//!
//! - `Trades` — `w = 1`: tick imbalance bars (TIB), the historical behavior.
//! - `Volume` — `w = quantity`: volume imbalance bars (VIB).
//! - `Dollar` — `w = price * quantity`: dollar imbalance bars (DIB).
//!
//! Everything counted in *trades* — the warm-up length, the hard cap and the
//! `E[T]` expectation — stays in trades for every unit, exactly as in the
//! book, where `T` is always the tick count of the bar. The unit changes what
//! `theta` accumulates, not what the target parameter means.
//!
//! # Closing rule
//!
//! A bar closes when `|theta| >= E[T] * |E[s]|`, floored at the scale
//! balanced flow actually reaches:
//!
//! - `E[T]` — expected trades per bar: **the `target_trades` parameter**,
//!   fixed. Not adaptive; see below for why.
//! - `E[s]` — expected signed weight per trade: a per-trade EWMA of `s` whose
//!   span is `target_trades` (weight `2 / (target_trades + 1)`), so the
//!   imbalance estimate looks back roughly one expected bar.
//! - the floor — `round(sqrt(target_trades))` typical trades' worth of
//!   weight; see [`ImbalanceBarBuilder::noise_floor`].
//!
//! In the trades unit `|E[s]|` is exactly the book's `|2P[b=1] - 1|`; in the
//! weighted units it estimates `|2v+ - E[v]|` the same way.
//!
//! # Why `E[T]` is fixed, and why the floor scales
//!
//! Both expectations used to adapt, and that made the knob mean nothing.
//!
//! The threshold is *linear* in `E[T]`, and `E[T]` was an EWMA of the bar
//! lengths that same threshold produced: a loop with no damping term. For a
//! driftless tape `theta` is a symmetric walk, so a bar of length `n` needs
//! `threshold ~ sqrt(n)` — self-consistency therefore sat at
//! `E[T] = 1 / floor^2`, a **repelling** fixed point that did not depend on
//! `target_trades` at all. Whatever the trader asked for, `E[T]` ran away
//! from it to one clamp or the other and stayed. The clamps did not prevent
//! the degeneracy the guards were written for; they decided which one you
//! got:
//!
//! - above the fixed point, `E[T]` pinned at `3 * target` and the threshold
//!   became a four-sigma excursion — no bar ever closed on imbalance, only on
//!   the hard cap, so an "imbalance" chart was a `3 * target`-tick chart;
//! - below it, `E[T]` pinned at `target / 4` and the threshold fell far under
//!   what a balanced walk reaches — a cascade of very short bars.
//!
//! Measured on a real WIN session under the tick rule the app runs live: at
//! `target = 1500` the same setting gave 3,720 trades per bar in one hour and
//! 226 in another, and `target = 1600` produced *shorter* bars than
//! `target = 1500` over the whole day. The parameter was neither monotone nor
//! reproducible.
//!
//! Fixing `E[T]` at `target_trades` removes the loop. The adaptivity that
//! carries information lives in `E[s]`, which is where the reference rule
//! puts it, and a fixed `E[T]` is what makes one knob mean one thing.
//!
//! The floor then has to carry balanced flow on its own, because that is
//! exactly when `|E[s]| -> 0`. A constant cannot: a walk of length `n`
//! reaches `|theta| ~ sqrt(n)` typical weights, so any fixed floor is
//! unreachable at large targets and trivial at small ones. At
//! `round(sqrt(target))` typical weights the floor's own expected first
//! passage is `~target` trades at every setting and in every unit — the
//! scale-free property the old constant could not have.
//!
//! **What the target delivers, measured.** The floor is a *lower bound* on
//! the threshold, not the threshold, so the length is not exactly the target
//! and this doc will not claim it is. In perfectly balanced flow
//! `E[T] * |E[s]|` is itself of order `sqrt(target)` — an EWMA of span `T`
//! has standard deviation `~1/sqrt(T)`, and `T` times that is `sqrt(T)` — so
//! both branches of the `max` are the same size and the winner sits above
//! either. Over a fixed-seed balanced tape that lands the bar at **1.3-1.7x
//! the target**, and flatly so across the whole range (100 -> 1.37x,
//! 1500 -> 1.44x, 5000 -> 1.34x). A real tape carries direction, which closes
//! bars early instead: on a recorded WIN session the same settings deliver
//! 0.6-0.65x. The parameter is a calibrated dial with a stable, monotone
//! response — not a promise of an exact count — and
//! `a_bar_is_about_the_target_long_in_balanced_flow` bounds it rather than
//! pinning it.
//!
//! **Evaluation order.** The trades unit folds the arriving trade into `E[s]`
//! *before* testing the threshold, and the weighted units do not. A
//! deliberate asymmetry: for `|s| = 1` the difference is second-order, while
//! for the weighted units it is first-order — a giant print folded into
//! `E[s]` first can raise the threshold by more than its own contribution to
//! `theta`, and the bar would survive exactly the elephant it exists to flag.
//! The weighted units therefore judge each trade against the expectations
//! formed *before* it (the book's `E_0[.]`), and fold it in afterwards.
//!
//! # Structural guards
//!
//! Two, both deterministic and part of the rule, not silent data repair:
//!
//! - a bar always closes after `3 * target_trades` trades ([`CAP_MULT`]). A
//!   backstop against a tape that offsets perfectly for an unbounded stretch,
//!   and nothing more: when it starts firing routinely, the threshold above
//!   it has stopped working;
//! - the floor is at least one typical trade's weight, so a bar cannot close
//!   on the trade that opened it for any `target_trades >= 4`. At the very
//!   bottom of the range it can, and honestly: `round(sqrt(2))` is 1, so a
//!   single print already meets the floor and about a third of bars run one
//!   trade at `target = 2` — which is a bar length of one against a target of
//!   two, not a degeneracy. A tape whose weights are all zero (a size unit
//!   over a size-less recording) has no measure to read: its threshold is
//!   zero, an imbalance close never fires, and only the trade cap bounds the
//!   bar. Weights are magnitudes; direction comes from the aggressor side
//!   alone.
//!
//! The **first** bar is a warm-up: with no history there is no meaningful
//! expectation, so it closes at exactly `target_trades` trades (like a tick
//! bar) while the EWMAs prime. Labeling the first bar's rule explicitly beats
//! pretending an uninformed threshold was informed.
//!
//! Everything is `Decimal` arithmetic and integer counts — no wall clock, no
//! randomness — so the builder stays deterministic per the engine's rules.

use rust_decimal::Decimal;

use crate::{
    Bar, BarBuilder, DollarMeasure, Measure as _, Side, TickMeasure, Trade, VolumeMeasure,
};

/// `sqrt(n)` rounded to the nearest integer, in exact integer arithmetic.
///
/// The floor is `sqrt(target)` typical weights, and truncating that square
/// root is not a rounding detail at the low end of the range: the error is
/// worst just below a perfect square, and the *bar length* it implies is the
/// floor squared. Truncated, `target = 3` floors at 1 — a bar closing on the
/// trade after the one that opened it, the cascade the floor exists to
/// prevent — and `target = 8` floors at 2, half the length asked for.
/// Rounded, they floor at 2 and 3, and the low end of the toolbar's range
/// means what it says.
///
/// `isqrt` gives `k` with `k^2 <= n`, so `n - k*k` is the distance past the
/// square and the nearest root is `k + 1` exactly when that distance exceeds
/// `k` (i.e. `n > (k + 0.5)^2`). Written as a subtraction rather than
/// `k*k + k` so a target near `u64::MAX` cannot overflow.
const fn rounded_isqrt(n: u64) -> u64 {
    let k = n.isqrt();
    if n - k * k > k { k + 1 } else { k }
}

/// A bar always closes after `CAP_MULT * target_trades` trades, whatever the
/// imbalance says.
///
/// A backstop against a tape that offsets perfectly for an unbounded stretch,
/// and nothing more: with the threshold below working, it should fire on its
/// own long before this does. When this guard becomes the *usual* way bars
/// close, the rule above it has stopped working — which is exactly what
/// happened before the `E[T]` feedback loop was removed.
const CAP_MULT: u64 = 3;

/// The measure a trade's aggression is weighed in — what `theta` accumulates.
///
/// See the [module docs](self): the unit changes the weight `w` of each
/// signed contribution, never the meaning of the `target_trades` parameter
/// (warm-up, cap and `E[T]` count trades in every unit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImbalanceUnit {
    /// `w = 1` — tick imbalance bars, the historical behavior.
    Trades,
    /// `w = quantity` — volume imbalance bars.
    Volume,
    /// `w = price * quantity` — dollar imbalance bars.
    Dollar,
}

impl ImbalanceUnit {
    /// Every unit, in the order the UI offers them.
    pub const ALL: [Self; 3] = [Self::Trades, Self::Volume, Self::Dollar];

    /// The spec token this unit is written as (`imbalance:volume:500`) —
    /// one vocabulary, owned here, so the chart and the backtest cannot
    /// drift apart on what a unit is called.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "trades",
            Self::Volume => "volume",
            Self::Dollar => "dollar",
        }
    }

    /// Parse a spec token back into a unit — the inverse of
    /// [`as_str`](Self::as_str). `None` for anything else; the caller owns
    /// the error message its spec dialect wants.
    #[must_use]
    pub fn parse_token(token: &str) -> Option<Self> {
        match token {
            "trades" => Some(Self::Trades),
            "volume" => Some(Self::Volume),
            "dollar" => Some(Self::Dollar),
            _ => None,
        }
    }

    /// The unsigned weight this trade contributes to `theta`: the magnitude
    /// of the engine's per-trade measures (tick / volume / dollar — one
    /// definition each, delegated, never re-derived here).
    ///
    /// Magnitude on purpose: direction comes from the aggressor side alone,
    /// so a signed-size export (negative quantity or price) can neither flip
    /// a print's side nor drive the `E[w]` floor negative. Saturating via
    /// the measures ([`Trade::notional`]): a notional beyond `Decimal`'s
    /// range clamps instead of panicking, the engine-wide feed-arithmetic
    /// policy.
    fn weight(self, trade: &Trade) -> Decimal {
        match self {
            Self::Trades => TickMeasure.of(trade),
            Self::Volume => VolumeMeasure.of(trade).abs(),
            Self::Dollar => DollarMeasure.of(trade).abs(),
        }
    }
}

/// Builds imbalance bars: a bar closes when `|theta|` — the running sum of
/// signed per-trade weights in the configured [`ImbalanceUnit`] — reaches the
/// adaptive threshold `E[T] * |E[s]|`.
///
/// See the [module docs](self) for the closing rule, the units, the warm-up
/// bar and the structural guards. Feed trades in order with
/// [`push`](BarBuilder::push); the in-progress bar is available via
/// [`partial`](BarBuilder::partial).
///
/// Like every builder, whole trades only: the trade that crosses the
/// threshold closes the bar it belongs to, and neither the imbalance nor the
/// trade count carries into the next bar.
#[derive(Debug, Clone)]
pub struct ImbalanceBarBuilder {
    target_trades: u64,
    /// What `theta` accumulates: signed ticks, volume or notional.
    unit: ImbalanceUnit,
    /// Per-trade EWMA weight for `E[s]` and `E[w]`: `2 / (target_trades + 1)`.
    alpha_b: Decimal,
    /// `1 - alpha_b`, precomputed once — the EWMA updates read it on every
    /// trade, and the subtraction rescales `ONE` to scale 28 each time it is
    /// redone inline.
    keep_b: Decimal,
    /// `E[T]` — expected trades per bar. The configured target, fixed.
    ///
    /// Deliberately *not* adaptive. It used to be an EWMA of the lengths the
    /// threshold produced, while the threshold was linear in it: a loop with
    /// no damping, whose fixed point sits at `1 / floor^2` regardless of what
    /// the trader asked for, and is unstable — so `E[T]` always ran to one of
    /// its clamps and stayed. The adaptivity that carries information lives
    /// in `E[s]`; taking it out of `E[T]` is what makes the knob mean one
    /// thing.
    e_t: Decimal,
    /// `round(sqrt(target_trades))` — how many *typical trades' worth* of
    /// weight the floor is. Integer [`rounded_isqrt`], so the engine keeps its
    /// exact arithmetic and no transcendental ever runs on the hot path.
    ///
    /// Never zero: `target_trades >= 1` is asserted at construction and
    /// `rounded_isqrt(1) == 1`, so no explicit clamp is needed.
    floor_trades: Decimal,
    /// Expected signed weight per trade, primed from zero as trades arrive.
    e_s: Decimal,
    /// Typical unsigned weight per trade: an EWMA primed with the first
    /// trade's weight (`None` until then). The trades unit never sets it —
    /// its weight is identically 1, so the floor is `floor_trades` alone.
    e_w: Option<Decimal>,
    /// Signed imbalance of the in-progress bar, in the configured unit.
    theta: Decimal,
    /// Trades in the in-progress bar.
    count: u64,
    /// Whether the warm-up bar has closed and the adaptive rule is active.
    warmed_up: bool,
    current: Option<Bar>,
}

impl ImbalanceBarBuilder {
    /// Create a builder targeting roughly `target_trades` trades per bar in
    /// balanced flow.
    ///
    /// `target_trades` seeds `E[T]`, sets the warm-up bar's length, the span
    /// of the `E[b]` EWMA, and the `3 *` hard cap — one knob calibrates the
    /// whole rule.
    ///
    /// # Panics
    ///
    /// Panics if `target_trades == 0`: a zero-trade expectation is
    /// meaningless, and coercing it silently would violate the data-honesty
    /// rule.
    #[must_use]
    pub fn new(target_trades: u64) -> Self {
        Self::with_unit(target_trades, ImbalanceUnit::Trades)
    }

    /// Create a builder whose `theta` accumulates in `unit` — trades gives
    /// the book's tick imbalance bars, volume and dollar the weighted
    /// variants. `target_trades` keeps the same meaning in every unit.
    ///
    /// # Panics
    ///
    /// Panics if `target_trades == 0`, exactly like [`new`](Self::new).
    #[must_use]
    pub fn with_unit(target_trades: u64, unit: ImbalanceUnit) -> Self {
        assert!(
            target_trades >= 1,
            "imbalance bar target_trades must be >= 1, got {target_trades}"
        );
        let alpha_b = Decimal::from(2) / Decimal::from(target_trades.saturating_add(1));
        Self {
            target_trades,
            unit,
            alpha_b,
            keep_b: Decimal::ONE - alpha_b,
            e_t: Decimal::from(target_trades),
            // `target_trades >= 1` is asserted above, and `rounded_isqrt(1)`
            // is 1, so the floor is never below a single typical trade — a
            // bar can never close on the very trade that opened it.
            floor_trades: Decimal::from(rounded_isqrt(target_trades)),
            e_s: Decimal::ZERO,
            e_w: None,
            theta: Decimal::ZERO,
            count: 0,
            warmed_up: false,
            current: None,
        }
    }

    /// The configured target (expected trades per bar in balanced flow).
    #[must_use]
    pub fn target_trades(&self) -> u64 {
        self.target_trades
    }

    /// The measure `theta` accumulates in.
    #[must_use]
    pub fn unit(&self) -> ImbalanceUnit {
        self.unit
    }

    /// The hard cap: a bar always closes at this many trades, whatever the
    /// imbalance says. Exposed so a consumer reporting *why* bars closed
    /// (the replay audit example) reads the rule instead of guessing it.
    #[must_use]
    pub fn hard_cap_trades(&self) -> u64 {
        CAP_MULT.saturating_mul(self.target_trades)
    }

    /// Does the in-progress bar close, given the current expectations?
    fn should_close(&self) -> bool {
        if !self.warmed_up {
            return self.count >= self.target_trades;
        }
        if self.count >= self.hard_cap_trades() {
            return true;
        }
        let threshold = self.threshold();
        // A tape whose weights are all zero (a size unit over a size-less
        // recording) has no measure to read: theta and the threshold are both
        // zero, and closing on `0 >= 0` would cascade one-trade bars — the
        // exact degeneration the floor exists to prevent. Such a bar closes
        // only at the cap. In the trades unit the threshold is structurally
        // positive, so this guard never fires there.
        threshold > Decimal::ZERO && self.theta.abs() >= threshold
    }

    /// The imbalance a bar must reach to close.
    ///
    /// `E[T] * |E[s]|` is the reference rule: the imbalance a *typical* bar
    /// ends on, so a bar closes as soon as it looks unusual. `E[s]` adapts
    /// over roughly one bar, so a burst arriving inside a bar is measured
    /// against the flow that came before it — the whole point of sampling
    /// this way.
    ///
    /// Floored at [`noise_floor`](Self::noise_floor) rather than at a
    /// fraction of `E[s]`: in balanced flow `|E[s]| -> 0` and the floor is
    /// the only thing left holding the rule up.
    ///
    /// Saturating like every arithmetic step feeding it: an adversarial print
    /// can pin `E[s]` near `Decimal::MAX`, and a threshold that saturates
    /// means "no imbalance close" — the trade cap above still bounds the bar
    /// — while a plain `*` would panic a builder on feed input, which the
    /// engine's arithmetic policy forbids.
    fn threshold(&self) -> Decimal {
        self.e_t
            .saturating_mul(self.e_s.abs())
            .max(self.noise_floor())
    }

    /// The imbalance balanced flow reaches on its own in `target_trades`
    /// trades: `round(sqrt(target))` typical trades' worth of weight.
    ///
    /// A driftless signed walk of `n` steps of typical size `E[w]` has
    /// `|theta| ~ E[w] * sqrt(n)`, so this is the level whose own expected
    /// first passage is `~target_trades` — at every setting and in every
    /// unit. A floor that did not scale this way was unreachable at large
    /// targets (every bar ran to the cap) and trivial at small ones. It is a
    /// lower bound, not the whole threshold; see the [module docs](self) for
    /// what the two branches together actually deliver.
    ///
    /// The trades unit skips the multiplication entirely: its weight is
    /// identically 1, so the floor is `round(sqrt(target))` and the per-trade
    /// hot path stays one field read. In the weighted units `e_w` is primed
    /// by the first `absorb`, which the `warmed_up` gate guarantees has run;
    /// the zero fallback keeps this total instead of trusting that ordering.
    fn noise_floor(&self) -> Decimal {
        match self.unit {
            ImbalanceUnit::Trades => self.floor_trades,
            _ => self
                .floor_trades
                .saturating_mul(self.e_w.unwrap_or(Decimal::ZERO)),
        }
    }

    /// Fold one trade's signed weight into the running expectations. The
    /// unsigned weight is `s.abs()` by construction — deriving it here keeps
    /// the two from ever being passed inconsistently.
    fn absorb(&mut self, s: Decimal) {
        self.e_s = self
            .alpha_b
            .saturating_mul(s)
            .saturating_add(self.keep_b.saturating_mul(self.e_s));
        // The trades unit skips the typical-weight EWMA: its weight is
        // identically 1, so `should_close` uses a constant floor instead and
        // the per-trade hot path saves these two multiplications.
        if self.unit != ImbalanceUnit::Trades {
            let w = s.abs();
            self.e_w = Some(match self.e_w {
                None => w,
                Some(prev) => self
                    .alpha_b
                    .saturating_mul(w)
                    .saturating_add(self.keep_b.saturating_mul(prev)),
            });
        }
    }

    /// Close the in-progress bar: reset the per-bar accumulators (no carry)
    /// and hand the bar out.
    ///
    /// `E[T]` is deliberately untouched here — folding the closed length back
    /// into it is precisely the feedback loop that made the target
    /// meaningless.
    fn close_bar(&mut self) -> Option<Bar> {
        self.warmed_up = true;
        self.theta = Decimal::ZERO;
        self.count = 0;
        self.current.take()
    }
}

impl BarBuilder for ImbalanceBarBuilder {
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        match &mut self.current {
            None => self.current = Some(Bar::opened_by(trade)),
            Some(bar) => bar.extend(trade),
        }
        self.count += 1;
        let w = self.unit.weight(trade);
        let s = match trade.side {
            Side::Buy => w,
            Side::Sell => -w,
        };
        self.theta = self.theta.saturating_add(s);

        // Per-unit evaluation order, pinned by tests and explained in the
        // module docs: trades folds the trade into the expectations first
        // (the shipped behavior, kept bit-exact); the weighted units judge
        // the trade against the expectations formed before it.
        let close = if self.unit == ImbalanceUnit::Trades {
            self.absorb(s);
            self.should_close()
        } else {
            let close = self.should_close();
            self.absorb(s);
            close
        };

        if close { self.close_bar() } else { None }
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn trade(agg_id: u64, side: Side) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: Decimal::from_str("100.0").unwrap(),
            quantity: Decimal::from_str("1.0").unwrap(),
            side,
        }
    }

    /// Feed `sides` in order, returning the closed bars.
    fn run(builder: &mut ImbalanceBarBuilder, sides: &[Side]) -> Vec<Bar> {
        sides
            .iter()
            .enumerate()
            .filter_map(|(i, side)| builder.push(&trade(i as u64, *side)))
            .collect()
    }

    /// A fixed-seed LCG. Not randomness the engine ever sees: it runs in the
    /// *test* to build a fixture tape, identical on every machine and every
    /// run, with no clock and no entropy. The determinism rule is about the
    /// builders, and a reproducible pseudo-tape is what lets these tests
    /// assert on first-passage behaviour at all.
    struct Lcg(u64);

    impl Lcg {
        /// `seed` distinguishes independent tapes. Two tapes drawn from the
        /// same seed are prefixes of one another, which quietly destroys any
        /// test that means to compare a lead-in against an unrelated
        /// measurement window.
        fn new(seed: u64) -> Self {
            Self(0x2545_f491_4f6c_dd1d ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        }

        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }
    }

    /// A tape with a chosen buy share, in per-mille, drawn from stream `seed`.
    ///
    /// It has to be a genuine walk rather than an evenly spread alternation:
    /// alternating sides pin `|theta|` at one and would test nothing but the
    /// hard cap. Real balanced flow wanders, and the wandering is exactly
    /// what the threshold is calibrated against.
    fn tape_seeded(len: usize, buy_per_mille: u64, seed: u64) -> Vec<Side> {
        let mut lcg = Lcg::new(seed);
        (0..len)
            .map(|_| {
                if lcg.next() % 1000 < buy_per_mille {
                    Side::Buy
                } else {
                    Side::Sell
                }
            })
            .collect()
    }

    /// The default tape stream.
    fn tape(len: usize, buy_per_mille: u64) -> Vec<Side> {
        tape_seeded(len, buy_per_mille, 0)
    }

    /// Mean trades per *closed* bar over `sides`, in the trades unit.
    ///
    /// Deliberately not `sides.len() / bars.len()`: the trades still sitting
    /// in the unclosed partial bar belong to no closed bar, and charging them
    /// to the closed population inflates exactly the ratio these tests
    /// certify — by up to a whole bar's worth at large targets.
    fn mean_len(target: u64, sides: &[Side]) -> f64 {
        let mut builder = ImbalanceBarBuilder::new(target);
        let bars = run(&mut builder, sides);
        assert!(!bars.is_empty(), "target {target} closed no bar at all");
        let closed: u64 = bars.iter().map(|b| b.trade_count).sum();
        closed as f64 / bars.len() as f64
    }

    /// The promise the knob makes, and the one it did not keep: in balanced
    /// flow a bar is about `target_trades` long.
    ///
    /// Before this rule was fixed, `E[T]` was an EWMA feeding a threshold
    /// linear in `E[T]` — a loop whose only resting places were the clamps at
    /// `target/4` and `3 * target`. Measured on a real WIN session under the
    /// tick rule the app runs live, target 1500 gave 3,720 trades per bar in
    /// hour 12 and 226 in hour 14: 14x apart, same setting, same day.
    #[test]
    fn a_bar_is_about_the_target_long_in_balanced_flow() {
        for target in [100_u64, 500, 2000, 5000] {
            let sides = tape(target as usize * 60, 500);
            let mean = mean_len(target, &sides);
            let ratio = mean / f64::from(u32::try_from(target).unwrap());
            assert!(
                (0.5..=2.0).contains(&ratio),
                "target {target}: mean bar length {mean:.0} is {ratio:.2}x the target"
            );
        }
    }

    /// Asking for longer bars must give longer bars.
    ///
    /// The old rule was not monotone on real data: on the trader's own tape
    /// under the tick rule, target 1500 gave 626 trades per bar and target
    /// 1600 gave 284 — a 100-trade nudge halved the bar, because the two
    /// settings landed in different attractors.
    #[test]
    fn a_larger_target_makes_longer_bars() {
        let sides = tape(400_000, 500);
        let mut previous = 0.0;
        for target in [100_u64, 500, 1500, 1600, 2000, 5000] {
            let mean = mean_len(target, &sides);
            assert!(
                mean > previous,
                "target {target} gave mean {mean:.0}, not longer than the previous {previous:.0}"
            );
            previous = mean;
        }
    }

    /// The same setting must mean the same thing whatever came before it.
    ///
    /// `E[T]`'s EWMA carried the previous regime into the next one and the
    /// clamps kept it there, so one target drifted more than an order of
    /// magnitude across a single session with nothing touched.
    #[test]
    fn the_bar_length_does_not_depend_on_the_regime_before_it() {
        let target = 2000;
        // The measurement window is stream 1 and the balanced lead is stream
        // 2, so the two arms differ only in the regime that came before —
        // drawing both from one stream would make the lead a byte-identical
        // prefix of the window and compare nothing.
        let measure = |lead: &[Side]| {
            let mut builder = ImbalanceBarBuilder::new(target);
            let _ = run(&mut builder, lead);
            let steady = tape_seeded(200_000, 500, 1);
            let bars = run(&mut builder, &steady);
            let closed: u64 = bars.iter().map(|b| b.trade_count).sum();
            closed as f64 / bars.len() as f64
        };
        let after_burst = measure(&vec![Side::Buy; 40_000]);
        let after_balance = measure(&tape_seeded(40_000, 500, 2));
        let ratio = after_burst / after_balance;
        assert!(
            (0.6..=1.6).contains(&ratio),
            "the same target settles at {after_burst:.0} after a burst and {after_balance:.0} \
             after balanced flow ({ratio:.2}x apart)"
        );
    }

    /// `sqrt` rounded, not truncated — and why the difference is not cosmetic.
    ///
    /// The floor is a bar-length target under a square root, so truncating it
    /// squares the error back up: a truncated `sqrt(3)` is 1, and a floor of
    /// one closes a bar on the trade after the one that opened it. The
    /// toolbar's minimum is 2, so this is inside the range a trader can set.
    #[test]
    fn the_floor_rounds_the_square_root_instead_of_truncating_it() {
        // Perfect squares are exact either way.
        assert_eq!(rounded_isqrt(1), 1);
        assert_eq!(rounded_isqrt(4), 2);
        assert_eq!(rounded_isqrt(2_250_000), 1500);
        // Just below a square is where truncation hurts most.
        assert_eq!(rounded_isqrt(3), 2, "truncating would floor at 1");
        assert_eq!(rounded_isqrt(8), 3, "truncating would floor at 2");
        assert_eq!(rounded_isqrt(80), 9, "truncating would floor at 8");
        // Below the half-step it still rounds down.
        assert_eq!(rounded_isqrt(2), 1);
        assert_eq!(rounded_isqrt(6), 2);
        // A target near the top of `u64` must not overflow the comparison.
        assert_eq!(rounded_isqrt(u64::MAX), 4_294_967_296);
    }

    /// The low end of the toolbar's range has to work too.
    ///
    /// The old minimum threshold was `(target/4) * FLOOR_B` = `target/80`,
    /// below 1 for every target under 80 — so the whole band the toolbar
    /// offers below 80 collapsed into a cascade of one- and two-trade bars.
    /// `2` is the toolbar's own minimum, so the range starts there.
    ///
    /// A mean-length band alone cannot see this: at `target = 2` a literal
    /// 100%-one-trade cascade scores a ratio of 0.5, which any sane band
    /// accepts. The one-trade *fraction* is the property the name claims, so
    /// it is asserted directly. It is zero from `target = 4` up, where
    /// `round(sqrt(target)) >= 2` puts the floor out of a single print's
    /// reach; at 2 and 3 the floor is 1 and 2, so short bars are the target
    /// rather than a collapse, and the bound is loosened to say so.
    #[test]
    fn small_targets_are_not_a_one_trade_cascade() {
        for target in [2_u64, 3, 4, 8, 10, 50, 79] {
            let sides = tape(target as usize * 400, 500);
            let mean = mean_len(target, &sides);
            let ratio = mean / f64::from(u32::try_from(target).unwrap());
            assert!(
                (0.5..=2.0).contains(&ratio),
                "target {target}: mean bar length {mean:.1} is {ratio:.2}x the target"
            );

            let mut builder = ImbalanceBarBuilder::new(target);
            let bars = run(&mut builder, &sides);
            let ones = bars.iter().filter(|b| b.trade_count == 1).count();
            let limit = if target < 4 { bars.len() / 2 } else { 0 };
            assert!(
                ones <= limit,
                "target {target}: {ones} of {} bars ran a single trade (limit {limit})",
                bars.len()
            );
        }
    }

    /// The cap is a backstop, not the closing rule.
    ///
    /// When bars routinely close at `3 * target` the threshold above has
    /// stopped working and the chart is a tick chart wearing an imbalance
    /// label. On the trader's sparse session that is exactly what happened:
    /// 47% of bars closed on the cap at target 1500 and 55% at target 2000,
    /// and on the dense one 39-59% in the morning hours.
    ///
    /// Not zero, and the bound says so honestly: the first-passage time to a
    /// fixed level has a tail, and `E[s]` is estimated from a finite window,
    /// so a minority of bars legitimately runs long. A quarter is the line
    /// between "a backstop fires sometimes" and "the backstop *is* the rule";
    /// measured here it sits at 9% (target 100) and 2% (target 2000).
    #[test]
    fn the_hard_cap_is_not_how_bars_normally_close() {
        for target in [100_u64, 2000] {
            let sides = tape(target as usize * 60, 500);
            let mut builder = ImbalanceBarBuilder::new(target);
            let bars = run(&mut builder, &sides);
            let cap = builder.hard_cap_trades();
            let capped = bars.iter().filter(|b| b.trade_count >= cap).count();
            assert!(
                capped * 4 < bars.len(),
                "target {target}: {capped} of {} bars closed on the cap",
                bars.len()
            );
        }
    }

    /// A weighted tape: sizes wander over an order of magnitude, the way a
    /// real one does, so the floor cannot be calibrated against a constant
    /// lot size it never sees.
    ///
    /// The **price** wanders too, and that is not decoration. The closing
    /// rule is scale-invariant in the weight (see
    /// `volume_unit_is_scale_invariant_in_quantity`), so at a constant price
    /// `DollarMeasure` is just `VolumeMeasure` times a constant and the two
    /// weighted units produce bit-identical bars — a Dollar arm over a
    /// constant-price tape re-tests Volume and would not notice price being
    /// wired out of the weight entirely.
    fn weighted_tape(len: usize, buy_per_mille: u64) -> Vec<Trade> {
        let mut lcg = Lcg::new(3);
        (0..len)
            .map(|i| {
                let side = if lcg.next() % 1000 < buy_per_mille {
                    Side::Buy
                } else {
                    Side::Sell
                };
                let qty = Decimal::from(1 + lcg.next() % 20);
                let price = Decimal::from(100 + lcg.next() % 400);
                Trade {
                    agg_id: i as u64,
                    timestamp_ms: 1000 + i as i64 * 100,
                    price,
                    quantity: qty,
                    side,
                }
            })
            .collect()
    }

    /// The guard the test above needs: over `weighted_tape`, Volume and
    /// Dollar must actually disagree. If they ever coincide again — a
    /// constant price sneaking back in — every Dollar assertion in this
    /// module silently becomes a second Volume assertion.
    #[test]
    fn the_weighted_tape_separates_volume_from_dollar() {
        let trades = weighted_tape(20_000, 500);
        let cut = |unit| {
            let mut b = ImbalanceBarBuilder::with_unit(200, unit);
            trades
                .iter()
                .filter_map(|t| b.push(t))
                .map(|bar| bar.trade_count)
                .collect::<Vec<_>>()
        };
        assert_ne!(
            cut(ImbalanceUnit::Volume),
            cut(ImbalanceUnit::Dollar),
            "price varies, so notional weighting must cut the tape differently"
        );
    }

    /// The target means trades per bar in *every* unit — the module doc has
    /// always said so, and the weighted units have to honour it too.
    ///
    /// The floor is what carries balanced flow, and in a weighted unit
    /// `theta` is measured in lots or currency, not counts. A floor
    /// calibrated for the trades unit is off by the typical trade size here,
    /// which on WIN is a factor of several.
    #[test]
    fn weighted_units_also_target_the_configured_trade_count() {
        for unit in [ImbalanceUnit::Volume, ImbalanceUnit::Dollar] {
            for target in [100_u64, 2000] {
                let trades = weighted_tape(target as usize * 60, 500);
                let mut builder = ImbalanceBarBuilder::with_unit(target, unit);
                let bars: Vec<Bar> = trades.iter().filter_map(|t| builder.push(t)).collect();
                assert!(!bars.is_empty(), "{unit:?} target {target} closed no bar");
                let mean = trades.len() as f64 / bars.len() as f64;
                let ratio = mean / f64::from(u32::try_from(target).unwrap());
                assert!(
                    (0.5..=2.0).contains(&ratio),
                    "{unit:?} target {target}: mean bar length {mean:.0} is {ratio:.2}x the target"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "imbalance bar target_trades must be >= 1")]
    fn rejects_zero_target() {
        let _ = ImbalanceBarBuilder::new(0);
    }

    #[test]
    fn target_trades_reports_configured_value() {
        assert_eq!(ImbalanceBarBuilder::new(100).target_trades(), 100);
    }

    #[test]
    fn warmup_bar_closes_at_exactly_target_trades() {
        // First bar has no history to calibrate against, so it closes at the
        // target length even under maximal imbalance (all buys).
        let mut b = ImbalanceBarBuilder::new(5);
        let bars = run(&mut b, &[Side::Buy; 5]);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].trade_count, 5);
        assert!(b.partial().is_none(), "no carry into the next bar");
    }

    #[test]
    fn contrary_burst_closes_a_bar_much_faster_than_target() {
        // Warm-up: 10 sells prime E[b] strongly negative. Then a burst of
        // buys — information the expectations did not predict — must close
        // the second bar far short of the 10-trade target. With
        // alpha_b = 2/11 the threshold falls below theta on the 3rd buy.
        let mut b = ImbalanceBarBuilder::new(10);
        let warmup = run(&mut b, &[Side::Sell; 10]);
        assert_eq!(warmup.len(), 1);
        assert_eq!(warmup[0].trade_count, 10);

        let burst = run(&mut b, &[Side::Buy; 10]);
        let first = burst.first().expect("the buy burst closes a bar");
        assert_eq!(
            first.trade_count, 3,
            "3 contrary trades beat the adapted threshold, not 10"
        );
        assert!(
            first.trade_count < b.target_trades(),
            "informative flow samples faster than the target"
        );
    }

    #[test]
    fn perfectly_offsetting_flow_closes_at_the_hard_cap() {
        // Strictly alternating buy/sell keeps |theta| <= 1 while E[b]
        // converges to +0.25 from above (target 4 => alpha_b = 0.4), so the
        // threshold E[T] * |E[b]| stays strictly above 1 and the adaptive rule
        // never fires. The 3x-target cap must close the bar instead.
        let mut b = ImbalanceBarBuilder::new(4);
        let sides: Vec<Side> = (0..40)
            .map(|i| if i % 2 == 0 { Side::Buy } else { Side::Sell })
            .collect();
        let bars = run(&mut b, &sides);
        assert_eq!(bars[0].trade_count, 4, "warm-up closes at target");
        assert_eq!(
            bars[1].trade_count, 12,
            "balanced flow runs to the 3x-target cap, never past it"
        );
    }

    #[test]
    fn imbalance_and_count_do_not_carry_across_a_close() {
        // After the warm-up close, the very next trade starts a fresh bar
        // whose partial reflects only itself — theta and count were reset. The
        // follow-up trade goes *with* the primed expectation (another buy), so
        // it extends the bar; a contrary trade would be information and close
        // it immediately.
        let mut b = ImbalanceBarBuilder::new(3);
        assert_eq!(run(&mut b, &[Side::Buy; 3]).len(), 1);
        assert!(b.partial().is_none());

        assert!(b.push(&trade(10, Side::Buy)).is_none());
        let p = b.partial().expect("fresh bar forming");
        assert_eq!(p.trade_count, 1);
        assert_eq!(p.buy_volume, Decimal::from_str("1.0").unwrap());
    }

    #[test]
    fn sustained_one_sided_flow_keeps_sampling_without_degenerating() {
        // Once one-sided flow *becomes* the expectation (E[b] -> 1), bars
        // re-lengthen toward E[T] instead of collapsing to 1-trade bars:
        // adaptation, not a death spiral.
        let mut b = ImbalanceBarBuilder::new(10);
        let bars = run(&mut b, &[Side::Buy; 200]);
        assert!(bars.len() >= 3, "the stream keeps producing bars");
        let last = bars.last().unwrap();
        assert!(
            last.trade_count >= 2,
            "expected re-lengthening, got a degenerate {}-trade bar",
            last.trade_count
        );
        for bar in &bars {
            assert!(
                bar.trade_count <= 3 * b.target_trades(),
                "no bar may exceed the hard cap"
            );
        }
    }

    /// An adaptive rule has no fixed threshold to count toward, and reporting
    /// one would tell the reader the bar closes at a moment it will not.
    #[test]
    fn an_adaptive_rule_reports_no_countdown() {
        let mut b = ImbalanceBarBuilder::new(10);
        assert!(b.progress().is_none());
        run(&mut b, &[Side::Buy, Side::Buy, Side::Sell]);
        assert!(b.progress().is_none());
    }

    // ---- units: volume / dollar imbalance (VIB / DIB) ----

    /// A trade with an explicit quantity, at the fixture price of 100.
    fn sized(agg_id: u64, side: Side, qty: &str) -> Trade {
        Trade {
            quantity: Decimal::from_str(qty).unwrap(),
            ..trade(agg_id, side)
        }
    }

    /// A trade with an explicit quantity and price.
    fn priced(agg_id: u64, side: Side, qty: &str, price: &str) -> Trade {
        Trade {
            price: Decimal::from_str(price).unwrap(),
            ..sized(agg_id, side, qty)
        }
    }

    #[test]
    fn unit_accessor_reports_configured_unit() {
        assert_eq!(
            ImbalanceBarBuilder::new(10).unit(),
            ImbalanceUnit::Trades,
            "the one-argument constructor keeps the historical tick unit"
        );
        assert_eq!(
            ImbalanceBarBuilder::with_unit(10, ImbalanceUnit::Dollar).unit(),
            ImbalanceUnit::Dollar
        );
        assert_eq!(
            ImbalanceBarBuilder::new(10).hard_cap_trades(),
            30,
            "the cap accessor reports the same 3x rule should_close enforces"
        );
    }

    /// The token vocabulary is owned here so every consumer speaks it; the
    /// round trip pins that emitting and parsing cannot drift apart.
    #[test]
    fn unit_tokens_round_trip() {
        for unit in ImbalanceUnit::ALL {
            assert_eq!(ImbalanceUnit::parse_token(unit.as_str()), Some(unit));
        }
        assert_eq!(ImbalanceUnit::parse_token("notional"), None);
        assert_eq!(
            ImbalanceUnit::parse_token("Trades"),
            None,
            "tokens are exact"
        );
    }

    /// `new` must stay a pure delegation to `with_unit(_, Trades)`: if a
    /// refactor ever gives it a code path of its own, the two builders here
    /// diverge and this test catches it. It cannot certify the *historical*
    /// behavior — both sides run today's code — that guarantee belongs to the
    /// untouched golden fixture in `tests/golden_imbalance.rs`.
    #[test]
    fn with_unit_trades_is_bit_exact_with_new() {
        let tape: Vec<Trade> = (0..200)
            .map(|i| {
                let side = if (i * 7 + 3) % 11 < 5 {
                    Side::Buy
                } else {
                    Side::Sell
                };
                let qty = format!("{}.5", (i % 9) + 1);
                let price = format!("{}", 100 + (i % 13));
                priced(i as u64, side, &qty, &price)
            })
            .collect();
        let mut legacy = ImbalanceBarBuilder::new(7);
        let mut trades_unit = ImbalanceBarBuilder::with_unit(7, ImbalanceUnit::Trades);
        let a: Vec<Bar> = tape.iter().filter_map(|t| legacy.push(t)).collect();
        let b: Vec<Bar> = tape.iter().filter_map(|t| trades_unit.push(t)).collect();
        assert!(a.len() >= 3, "fixture must actually close bars");
        assert_eq!(a, b);
        assert_eq!(legacy.partial(), trades_unit.partial());
    }

    /// Volume imbalance weighs θ by traded size: after a one-sided warm-up
    /// (E[s] = -0.8704 per trade, E[T] = 4, threshold 4·0.8704 = 3.4816) a
    /// half-lot contrary print stays inside the bar, a ten-lot elephant closes
    /// it on the spot. Same rule, same tape shape — only the size differs.
    ///
    /// The elephant case also pins the evaluation order for weighted units:
    /// the trade is judged against the expectations formed *before* it. Folding
    /// the ten-lot into E[s] first would lift the threshold to
    /// 4·|0.4·10 + 0.6·(-0.8704)| = 13.911 and the bar would absurdly survive
    /// its own elephant.
    #[test]
    fn volume_unit_closes_on_size_not_on_count() {
        let warmup: Vec<Trade> = (0..4).map(|i| sized(i, Side::Sell, "1")).collect();

        let mut small = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Volume);
        for t in &warmup {
            small.push(t);
        }
        assert!(
            small.push(&sized(10, Side::Buy, "0.5")).is_none(),
            "a half-lot contrary print is not information; the bar stays open"
        );

        let mut elephant = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Volume);
        for t in &warmup {
            elephant.push(t);
        }
        let bar = elephant
            .push(&sized(10, Side::Buy, "10"))
            .expect("a ten-lot contrary elephant closes the bar immediately");
        assert_eq!(bar.trade_count, 1);
    }

    /// Dollar imbalance weighs θ by notional: with the warm-up at price 100
    /// (threshold 4·87.04 = 348.16), the same one-lot contrary print stays
    /// inside the bar at price 300 and closes it at price 400 — price is part
    /// of the weight, not just quantity.
    #[test]
    fn dollar_unit_closes_on_notional_not_on_count() {
        let warmup: Vec<Trade> = (0..4).map(|i| priced(i, Side::Sell, "1", "100")).collect();

        let mut cheap = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Dollar);
        for t in &warmup {
            cheap.push(t);
        }
        assert!(
            cheap.push(&priced(10, Side::Buy, "1", "300")).is_none(),
            "300 notional is under the 348.16 threshold; the bar stays open"
        );

        let mut rich = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Dollar);
        for t in &warmup {
            rich.push(t);
        }
        let bar = rich
            .push(&priced(10, Side::Buy, "1", "400"))
            .expect("400 notional beats the threshold and closes the bar");
        assert_eq!(bar.trade_count, 1);
    }

    /// The warm-up bar and the hard cap count *trades* in every unit: huge
    /// quantities neither shorten the warm-up nor dodge the cap.
    #[test]
    fn weighted_units_keep_warmup_and_cap_in_trade_counts() {
        // Warm-up: four 1000-lot buys close at exactly the 4-trade target.
        let mut b = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Volume);
        let mut bars = Vec::new();
        for i in 0..4 {
            bars.extend(b.push(&sized(i, Side::Buy, "1000")));
        }
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].trade_count, 4);

        // Cap: strictly alternating constant-size flow keeps |θ| ≤ one weight
        // (1000) while the floor holds the threshold at
        // round(sqrt(80))·E[w] = 9·1000 = 9000, so only the 3x-target trade
        // cap can close the bar.
        let mut b = ImbalanceBarBuilder::with_unit(80, ImbalanceUnit::Volume);
        let mut bars = Vec::new();
        for i in 0..320 {
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            bars.extend(b.push(&sized(i, side, "1000")));
        }
        assert_eq!(bars[0].trade_count, 80, "warm-up closes at target");
        assert_eq!(
            bars[1].trade_count, 240,
            "balanced heavy flow runs to the 3x-target cap, never past it"
        );
    }

    /// The closing rule is scale-free in the weight: the same side sequence at
    /// nine times the size closes bars at exactly the same trades. θ, E[s] and
    /// the floor (a multiple of the typical weight) all scale together — a
    /// floor left in per-trade units would break this.
    #[test]
    fn volume_unit_is_scale_invariant_in_quantity() {
        let sides: Vec<Side> = (0..300)
            .map(|i| {
                if (i * 7 + 3) % 11 < 5 {
                    Side::Buy
                } else {
                    Side::Sell
                }
            })
            .collect();
        let close_points = |qty: &str| -> Vec<u64> {
            let mut b = ImbalanceBarBuilder::with_unit(12, ImbalanceUnit::Volume);
            sides
                .iter()
                .enumerate()
                .filter_map(|(i, side)| b.push(&sized(i as u64, *side, qty)))
                .map(|bar| bar.trade_count)
                .collect()
        };
        let ones = close_points("1");
        assert!(ones.len() >= 3, "fixture must actually close bars");
        assert_eq!(ones, close_points("9"));
    }

    /// The feed-arithmetic policy holds in the weighted units: adversarial
    /// prints whose notional saturates `Decimal` must never panic the
    /// builder. `E[s]` rides toward `Decimal::MAX` on the prints alone —
    /// `E[T]` is a construction-time constant now and contributes nothing to
    /// the growth — and a plain `*` against it overflows within a few prints.
    /// The saturating threshold instead means "no imbalance close", and the
    /// trade cap keeps bounding every bar.
    #[test]
    fn adversarial_notional_never_panics_and_bars_stay_capped() {
        let mut b = ImbalanceBarBuilder::with_unit(100, ImbalanceUnit::Dollar);
        for i in 0..100 {
            b.push(&priced(i, Side::Sell, "1", "100"));
        }
        let giant = Decimal::MAX.to_string();
        let mut bars = Vec::new();
        for i in 0..300 {
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            bars.extend(b.push(&priced(100 + i, side, &giant, &giant)));
        }
        assert!(!bars.is_empty(), "the stream still produces bars");
        for bar in &bars {
            assert!(bar.trade_count <= 300, "no bar may exceed the hard cap");
        }
    }

    /// A size unit over a tape that prints no size (every weight zero) has
    /// no measure to read, so it must not cascade one-trade bars — the
    /// degeneration the floor exists to prevent. Only the trade cap closes
    /// such a bar.
    #[test]
    fn zero_weight_tape_closes_on_the_cap_not_in_cascade() {
        let mut b = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Volume);
        let mut bars = Vec::new();
        for i in 0..40 {
            let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
            bars.extend(b.push(&sized(i, side, "0")));
        }
        assert_eq!(bars[0].trade_count, 4, "warm-up still counts trades");
        assert!(bars.len() >= 3, "the cap keeps the stream producing bars");
        for bar in &bars[1..] {
            assert_eq!(
                bar.trade_count, 12,
                "a measureless bar closes only at the 3x-target cap"
            );
        }
    }

    /// A signed export (negative price or quantity) weighs by magnitude:
    /// direction comes from the aggressor side alone, so two taker buys
    /// always reinforce theta — one of them printed negative must not cancel
    /// the other out (nor drive the `E[w]` floor negative).
    #[test]
    fn signed_prints_weigh_as_magnitude_not_as_direction() {
        let mut b = ImbalanceBarBuilder::with_unit(4, ImbalanceUnit::Dollar);
        for i in 0..4 {
            b.push(&priced(i, Side::Sell, "1", "100"));
        }
        // Threshold after the all-sell warm-up: 4 * 87.04 = 348.16.
        assert!(
            b.push(&priced(10, Side::Buy, "1", "300")).is_none(),
            "one 300-notional buy is under the threshold"
        );
        let bar = b
            .push(&priced(11, Side::Buy, "1", "-300"))
            .expect("a second buy adds |-300| to theta; cancelling to zero would deny the close");
        assert_eq!(bar.trade_count, 2);
    }
}
