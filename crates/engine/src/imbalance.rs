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
//! The reference rule closes a bar when `|theta| >= E[T] * |E[s]|`, where both
//! expectations adapt to the observed stream:
//!
//! - `E[T]` — expected trades per bar: an EWMA (weight [`ALPHA_T`]) over the
//!   trade counts of closed bars, seeded with the `target_trades` parameter.
//! - `E[s]` — expected signed weight per trade: a per-trade EWMA of `s` whose
//!   span is `target_trades` (weight `2 / (target_trades + 1)`), so the
//!   imbalance estimate looks back roughly one expected bar.
//!
//! In the trades unit `|E[s]|` is exactly the book's `|2P[b=1] - 1|`; in the
//! weighted units it estimates `|2v+ - E[v]|` the same way.
//!
//! **Evaluation order.** The trades unit folds the arriving trade into `E[s]`
//! *before* testing the threshold — the behavior this bar type shipped with,
//! kept bit-for-bit so existing charts and backtests never move. For `|s| = 1`
//! the difference is second-order. For the weighted units it is first-order: a
//! giant print folded into `E[s]` first can raise the threshold by more than
//! its own contribution to `theta`, and the bar would survive exactly the
//! elephant it exists to flag. The weighted units therefore judge each trade
//! against the expectations formed *before* it (the book's `E_0[.]`), and fold
//! it in afterwards.
//!
//! # Structural guards (and why they are honest)
//!
//! The textbook rule is known to degenerate: in near-balanced flow
//! `|E[s]| -> 0` collapses the threshold (a cascade of one-trade bars), and a
//! feedback loop between shrinking bars and shrinking `E[T]` can pin it there.
//! Rather than patch the stream, the closing rule itself is bounded by three
//! fixed, documented guards — every one deterministic and part of the rule,
//! not silent data repair:
//!
//! - the effective `|E[s]|` never drops below [`FLOOR_B`] times `E[w]` — an
//!   EWMA of the unsigned weight, primed with the first trade's weight, so the
//!   floor means "5% of a typical trade" in every unit (in the trades unit
//!   `E[w]` is exactly 1 and the floor is the historical `0.05`) and the
//!   threshold stays meaningfully positive in balanced flow;
//! - a bar always closes after `3 * target_trades` trades ([`CAP_MULT`]), so
//!   perfectly offsetting flow cannot grow a bar without bound;
//! - `E[T]` is clamped to `[target_trades / 4, 3 * target_trades]`, so a
//!   transient regime cannot drag the expectation somewhere it takes forever
//!   to recover from.
//!
//! The **first** bar is a warm-up: with no history there is no meaningful
//! expectation, so it closes at exactly `target_trades` trades (like a tick
//! bar) while the EWMAs prime. Labeling the first bar's rule explicitly beats
//! pretending an uninformed threshold was informed.
//!
//! Everything is `Decimal` arithmetic and integer counts — no wall clock, no
//! randomness — so the builder stays deterministic per the engine's rules.

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, Side, Trade};

/// EWMA weight for the expected-trades-per-bar update, applied once per
/// closed bar. `0.25` spans roughly the last seven bars.
const ALPHA_T: Decimal = Decimal::from_parts(25, 0, 0, false, 2);

/// Lower bound on the effective `|E[s]|` in the threshold, as a fraction of
/// `E[w]` (the typical per-trade weight), so near-balanced flow cannot
/// collapse the threshold to zero. In the trades unit `E[w]` is exactly 1 and
/// this is the historical absolute floor of `0.05`.
const FLOOR_B: Decimal = Decimal::from_parts(5, 0, 0, false, 2);

/// A bar always closes after `CAP_MULT * target_trades` trades, whatever the
/// imbalance says.
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
    /// The unsigned weight this trade contributes to `theta`.
    ///
    /// Saturating: a notional beyond `Decimal`'s range clamps instead of
    /// panicking, matching the engine-wide feed-arithmetic policy.
    fn weight(self, trade: &Trade) -> Decimal {
        match self {
            Self::Trades => Decimal::ONE,
            Self::Volume => trade.quantity,
            Self::Dollar => trade.price.saturating_mul(trade.quantity),
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
    /// Expected trades per bar, seeded with `target_trades`.
    e_t: Decimal,
    /// Expected signed weight per trade, primed from zero as trades arrive.
    e_s: Decimal,
    /// Typical unsigned weight per trade: an EWMA primed with the first
    /// trade's weight (`None` until then). The trades unit never sets it —
    /// its weight is identically 1 — and the floor falls back to the
    /// historical constant `0.05`.
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
        Self {
            target_trades,
            unit,
            alpha_b: Decimal::from(2) / Decimal::from(target_trades.saturating_add(1)),
            e_t: Decimal::from(target_trades),
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

    /// Does the in-progress bar close, given the current expectations?
    fn should_close(&self) -> bool {
        if !self.warmed_up {
            return self.count >= self.target_trades;
        }
        if self.count >= CAP_MULT.saturating_mul(self.target_trades) {
            return true;
        }
        // `e_w` is `None` exactly in the trades unit, whose typical weight is
        // identically 1 — the floor multiplication would buy nothing on the
        // per-trade hot path.
        let floor = match self.e_w {
            None => FLOOR_B,
            Some(typical) => FLOOR_B.saturating_mul(typical),
        };
        let threshold = self.e_t * self.e_s.abs().max(floor);
        self.theta.abs() >= threshold
    }

    /// Fold one trade's signed weight into the running expectations.
    fn absorb(&mut self, s: Decimal, w: Decimal) {
        let keep = Decimal::ONE - self.alpha_b;
        self.e_s = self
            .alpha_b
            .saturating_mul(s)
            .saturating_add(keep.saturating_mul(self.e_s));
        // The trades unit skips the typical-weight EWMA: `w` is identically 1,
        // so the update is two Decimal multiplications per trade spent
        // computing 1 — measurably slower on the hot path (bench: 2x) for a
        // floor `should_close` can use as a constant instead.
        if self.unit != ImbalanceUnit::Trades {
            self.e_w = Some(match self.e_w {
                None => w,
                Some(prev) => self
                    .alpha_b
                    .saturating_mul(w)
                    .saturating_add(keep.saturating_mul(prev)),
            });
        }
    }

    /// Close the in-progress bar: fold its length into `E[T]`, reset the
    /// per-bar accumulators (no carry), and hand the bar out.
    fn close_bar(&mut self) -> Option<Bar> {
        let closed_len = Decimal::from(self.count);
        let updated = ALPHA_T * closed_len + (Decimal::ONE - ALPHA_T) * self.e_t;
        let min = Decimal::from(self.target_trades) / Decimal::from(4);
        // Saturating to match `should_close`'s hard cap and stay panic-free for
        // any configured `target_trades`.
        let max = Decimal::from(CAP_MULT.saturating_mul(self.target_trades));
        self.e_t = updated.clamp(min, max);
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
            self.absorb(s, w);
            self.should_close()
        } else {
            let close = self.should_close();
            self.absorb(s, w);
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
    }

    /// `with_unit(_, Trades)` is the same builder `new` always was: same tape
    /// in, bit-identical bars out — existing charts and backtests must not
    /// move by a single trade.
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
        // while the floor holds the threshold at E[T]·0.05·1000 ≥ 4000, so
        // only the 3x-target trade cap can close the bar.
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
    /// the floor (a fraction of the typical weight) all scale together — a
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
}
