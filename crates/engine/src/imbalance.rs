//! Imbalance bars — close a bar when aggressor imbalance exceeds a dynamic
//! threshold.
//!
//! Tick imbalance bars (López de Prado, *Advances in Financial Machine
//! Learning*, ch. 2) sample by **information arrival** rather than by raw
//! activity: each trade contributes a signed tick `b = +1` (taker buy) or
//! `b = -1` (taker sell), and the bar closes when the running imbalance
//! `theta = sum(b)` becomes unusually large relative to what recent history
//! says is normal. Balanced two-way flow produces long bars; a one-sided burst
//! of aggression — new information hitting the market — closes a bar almost
//! immediately, so the sampling rate itself tracks information flow.
//!
//! # Closing rule
//!
//! The reference rule closes a bar when `|theta| >= E[T] * |E[b]|`, where both
//! expectations adapt to the observed stream:
//!
//! - `E[T]` — expected trades per bar: an EWMA (weight [`ALPHA_T`]) over the
//!   trade counts of closed bars, seeded with the `target_trades` parameter.
//! - `E[b]` — expected signed tick: a per-trade EWMA of `b` whose span is
//!   `target_trades` (weight `2 / (target_trades + 1)`), so the imbalance
//!   estimate looks back roughly one expected bar.
//!
//! `|2P[b=1] - 1|` in the book is exactly `|E[b]|`; the EWMA estimates it
//! directly.
//!
//! # Structural guards (and why they are honest)
//!
//! The textbook rule is known to degenerate: in near-balanced flow
//! `|E[b]| -> 0` collapses the threshold (a cascade of one-trade bars), and a
//! feedback loop between shrinking bars and shrinking `E[T]` can pin it there.
//! Rather than patch the stream, the closing rule itself is bounded by three
//! fixed, documented guards — every one deterministic and part of the rule,
//! not silent data repair:
//!
//! - the effective `|E[b]|` never drops below [`FLOOR_B`], so the threshold
//!   stays meaningfully positive in balanced flow;
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

use crate::threshold::{extend_bar, open_bar};
use crate::{Bar, BarBuilder, Side, Trade};

/// EWMA weight for the expected-trades-per-bar update, applied once per
/// closed bar. `0.25` spans roughly the last seven bars.
const ALPHA_T: Decimal = Decimal::from_parts(25, 0, 0, false, 2);

/// Lower bound on the effective `|E[b]|` in the threshold, so near-balanced
/// flow cannot collapse the threshold to zero.
const FLOOR_B: Decimal = Decimal::from_parts(5, 0, 0, false, 2);

/// A bar always closes after `CAP_MULT * target_trades` trades, whatever the
/// imbalance says.
const CAP_MULT: u64 = 3;

/// Builds tick imbalance bars: a bar closes when `|theta|` — the running sum
/// of signed ticks — reaches the adaptive threshold `E[T] * |E[b]|`.
///
/// See the [module docs](self) for the closing rule, the warm-up bar and the
/// structural guards. Feed trades in order with [`push`](BarBuilder::push);
/// the in-progress bar is available via [`partial`](BarBuilder::partial).
///
/// Like every builder, whole trades only: the trade that crosses the
/// threshold closes the bar it belongs to, and neither the imbalance nor the
/// trade count carries into the next bar.
#[derive(Debug, Clone)]
pub struct ImbalanceBarBuilder {
    target_trades: u64,
    /// Per-trade EWMA weight for `E[b]`: `2 / (target_trades + 1)`.
    alpha_b: Decimal,
    /// Expected trades per bar, seeded with `target_trades`.
    e_t: Decimal,
    /// Expected signed tick, primed from zero as trades arrive.
    e_b: Decimal,
    /// Signed tick imbalance of the in-progress bar.
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
        assert!(
            target_trades >= 1,
            "imbalance bar target_trades must be >= 1, got {target_trades}"
        );
        Self {
            target_trades,
            alpha_b: Decimal::from(2) / Decimal::from(target_trades.saturating_add(1)),
            e_t: Decimal::from(target_trades),
            e_b: Decimal::ZERO,
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

    /// Does the in-progress bar close on the trade just folded in?
    fn should_close(&self) -> bool {
        if !self.warmed_up {
            return self.count >= self.target_trades;
        }
        if self.count >= CAP_MULT.saturating_mul(self.target_trades) {
            return true;
        }
        let threshold = self.e_t * self.e_b.abs().max(FLOOR_B);
        self.theta.abs() >= threshold
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
            None => self.current = Some(open_bar(trade)),
            Some(bar) => extend_bar(bar, trade),
        }
        self.count += 1;
        let b = match trade.side {
            Side::Buy => Decimal::ONE,
            Side::Sell => -Decimal::ONE,
        };
        self.theta = self.theta.saturating_add(b);
        self.e_b = self.alpha_b * b + (Decimal::ONE - self.alpha_b) * self.e_b;

        if self.should_close() {
            self.close_bar()
        } else {
            None
        }
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
}
