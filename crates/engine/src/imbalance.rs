//! Imbalance bars (López de Prado): close when `|theta| = |sum(s)|`, `s = ±w` by aggressor, reaches
//! `max(E[T] * |E[s]|, floor)`; `E[T]` is the fixed `target_trades`, the floor `round(sqrt(target))` weights.
//! Units ([`ImbalanceUnit`]) change only `w`; counts (warm-up, cap `3x`) stay in trades. Decimal only.

use rust_decimal::Decimal;

use crate::{
    Bar, BarBuilder, DollarMeasure, Measure as _, Side, TickMeasure, Trade, VolumeMeasure,
};

/// `sqrt(n)` rounded to the nearest integer, in exact integer arithmetic.
/// Subtraction form so a target near `u64::MAX` cannot overflow.
const fn rounded_isqrt(n: u64) -> u64 {
    let k = n.isqrt();
    if n - k * k > k { k + 1 } else { k }
}

/// A bar always closes after `CAP_MULT * target_trades` trades; a backstop, not the rule.
const CAP_MULT: u64 = 3;

/// What `theta` accumulates; it changes the weight `w`, never what `target_trades` counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImbalanceUnit {
    /// `w = 1`: tick imbalance bars.
    Trades,
    /// `w = quantity`: volume imbalance bars.
    Volume,
    /// `w = price * quantity`: dollar imbalance bars.
    Dollar,
}

impl ImbalanceUnit {
    /// Every unit, in the order the UI offers them.
    pub const ALL: [Self; 3] = [Self::Trades, Self::Volume, Self::Dollar];

    /// The spec token this unit is written as (`imbalance:volume:500`), owned here so the chart and
    /// the backtest cannot drift apart.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "trades",
            Self::Volume => "volume",
            Self::Dollar => "dollar",
        }
    }

    /// Parse a spec token back into a unit, the inverse of [`as_str`](Self::as_str); `None` otherwise.
    #[must_use]
    pub fn parse_token(token: &str) -> Option<Self> {
        match token {
            "trades" => Some(Self::Trades),
            "volume" => Some(Self::Volume),
            "dollar" => Some(Self::Dollar),
            _ => None,
        }
    }

    /// The unsigned weight a trade adds to `theta`; direction comes from the aggressor side alone.
    /// Saturating ([`Trade::notional`]): out-of-range notionals clamp.
    fn weight(self, trade: &Trade) -> Decimal {
        match self {
            Self::Trades => TickMeasure.of(trade),
            Self::Volume => VolumeMeasure.of(trade).abs(),
            Self::Dollar => DollarMeasure.of(trade).abs(),
        }
    }
}

/// Builds imbalance bars: a bar closes when `|theta|` reaches `E[T] * |E[s]|` (module docs).
/// Whole trades only: nothing carries into the next bar.
#[derive(Debug, Clone)]
pub struct ImbalanceBarBuilder {
    target_trades: u64,
    /// What `theta` accumulates: signed ticks, volume or notional.
    unit: ImbalanceUnit,
    /// Per-trade EWMA weight for `E[s]` and `E[w]`: `2 / (target_trades + 1)`.
    alpha_b: Decimal,
    /// `1 - alpha_b`, precomputed: redoing the subtraction per trade rescales `ONE` to scale 28.
    keep_b: Decimal,
    /// `E[T]`: expected trades per bar, the configured target, deliberately not adaptive.
    e_t: Decimal,
    /// `round(sqrt(target_trades))` typical trades' worth of weight in the floor; never zero.
    floor_trades: Decimal,
    /// Expected signed weight per trade, primed from zero as trades arrive.
    e_s: Decimal,
    /// Typical unsigned weight per trade: an EWMA primed with the first trade's weight. The trades unit
    /// never sets it (weight is 1, the floor is `floor_trades` alone).
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
    /// Create a builder targeting about `target_trades` trades per bar. Panics if it is zero.
    #[must_use]
    pub fn new(target_trades: u64) -> Self {
        Self::with_unit(target_trades, ImbalanceUnit::Trades)
    }

    /// Create a builder whose `theta` accumulates in `unit`. Panics if `target_trades` is zero.
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
            // `target_trades >= 1` is asserted above and `rounded_isqrt(1)` is 1, so the floor is never below
            // one typical trade: a bar cannot close on its opening trade.
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

    /// The hard cap: a bar always closes at this many trades, so a consumer reporting why bars closed
    /// reads the rule instead of guessing it.
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
        // All-zero weights (a size unit over a size-less tape): theta and threshold are both zero, and
        // closing on `0 >= 0` would cascade one-trade bars. Such a bar closes only at the cap.
        threshold > Decimal::ZERO && self.theta.abs() >= threshold
    }

    /// The imbalance a bar must reach: `E[T] * |E[s]|`, floored at [`noise_floor`](Self::noise_floor).
    /// Saturating: a saturated threshold means no imbalance close; the cap still bounds the bar.
    fn threshold(&self) -> Decimal {
        self.e_t
            .saturating_mul(self.e_s.abs())
            .max(self.noise_floor())
    }

    /// The imbalance balanced flow reaches in `target_trades` trades: `round(sqrt(target))` typical weights.
    /// The trades unit skips the multiply (weight 1); the zero fallback keeps this total.
    fn noise_floor(&self) -> Decimal {
        match self.unit {
            ImbalanceUnit::Trades => self.floor_trades,
            _ => self
                .floor_trades
                .saturating_mul(self.e_w.unwrap_or(Decimal::ZERO)),
        }
    }

    /// Fold one trade's signed weight into the running expectations; the unsigned weight is derived
    /// as `s.abs()` so the two cannot be passed inconsistently.
    fn absorb(&mut self, s: Decimal) {
        self.e_s = self
            .alpha_b
            .saturating_mul(s)
            .saturating_add(self.keep_b.saturating_mul(self.e_s));
        // The trades unit skips the typical-weight EWMA: its weight is 1, so the floor is a constant.
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

    /// Close the in-progress bar: reset the per-bar accumulators (no carry) and hand the bar out.
    /// `E[T]` stays untouched: feeding the length back into it is the loop the fixed target removed.
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

        // Per-unit evaluation order (module docs): trades folds the trade in first, bit-exact with the
        // shipped behavior; weighted units judge it against the expectations formed before it.
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

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize {
        closed.extend(self.push(trade));
        0
    }

    fn partial(&self) -> Option<&Bar> {
        self.current.as_ref()
    }

    /// `theta` itself: zero right after a close, since the close resets it.
    fn running_imbalance(&self) -> Option<Decimal> {
        Some(self.theta)
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
    fn the_running_imbalance_is_signed_and_starts_over_at_a_close() {
        let mut builder = ImbalanceBarBuilder::new(4);
        assert_eq!(builder.running_imbalance(), Some(Decimal::ZERO));
        let closed = run(&mut builder, &[Side::Buy, Side::Buy, Side::Sell]);
        assert!(closed.is_empty(), "warm-up closes at the fourth trade");
        assert_eq!(builder.running_imbalance(), Some(Decimal::ONE));
        let _ = builder.push(&trade(3, Side::Sell));
        assert!(
            builder.partial().is_none(),
            "the fourth trade closed the bar"
        );
        assert_eq!(builder.running_imbalance(), Some(Decimal::ZERO));
        let _ = builder.push(&trade(4, Side::Sell));
        assert_eq!(builder.running_imbalance(), Some(Decimal::NEGATIVE_ONE));
    }

    #[test]
    fn an_adaptive_rule_still_reports_no_fixed_progress() {
        let mut builder = ImbalanceBarBuilder::new(4);
        let _ = run(&mut builder, &[Side::Buy]);
        assert_eq!(builder.progress(), None);
        assert_eq!(builder.closes_at_ms(), None);
    }

    /// A fixed-seed LCG: it builds a fixture tape, identical on every machine and run, with no clock or
    /// entropy. The determinism rule governs the builders, not this.
    struct Lcg(u64);

    impl Lcg {
        /// `seed` distinguishes independent tapes; one seed gives prefixes of one another.
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

    /// A seeded random-walk tape with a chosen buy share (per-mille); alternation would test only the cap.
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

    /// Mean trades per *closed* bar over `sides`, in the trades unit. Not `sides.len() / bars.len()`:
    /// trades in the unclosed partial bar belong to no closed bar and would inflate the ratio.
    fn mean_len(target: u64, sides: &[Side]) -> f64 {
        let mut builder = ImbalanceBarBuilder::new(target);
        let bars = run(&mut builder, sides);
        assert!(!bars.is_empty(), "target {target} closed no bar at all");
        let closed: u64 = bars.iter().map(|b| b.trade_count).sum();
        closed as f64 / bars.len() as f64
    }

    /// The knob's promise: in balanced flow a bar is about `target_trades` long.
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
    #[test]
    fn the_bar_length_does_not_depend_on_the_regime_before_it() {
        let target = 2000;
        // The window is stream 1 and the lead stream 2, so the arms differ only in the preceding regime
        // (one stream would make the lead a prefix of the window).
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

    /// `sqrt` rounded, not truncated: a truncated `sqrt(3)` is 1, and a floor of one closes a bar on
    /// the trade after its opener. The toolbar's minimum is 2, inside the settable range.
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

    /// Targets 2 and up must not cascade one-trade bars; asserted as a one-trade fraction (zero from 4).
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

    /// The cap is a backstop: fewer than a quarter of bars may close on it (measured 2-9%).
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

    /// A weighted tape; price wanders too, or Dollar would equal Volume times a constant.
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

    /// Guard for the test above: over `weighted_tape` Volume and Dollar must disagree, or every
    /// Dollar assertion silently becomes a Volume one.
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

    /// The target means trades per bar in *every* unit, so the floor must scale to the weighted
    /// `theta` (lots or currency), not the trades unit's counts.
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
        // The first bar has no history, so it closes at the target length even under maximal imbalance.
        let mut b = ImbalanceBarBuilder::new(5);
        let bars = run(&mut b, &[Side::Buy; 5]);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].trade_count, 5);
        assert!(b.partial().is_none(), "no carry into the next bar");
    }

    #[test]
    fn contrary_burst_closes_a_bar_much_faster_than_target() {
        // Warm-up: 10 sells prime E[b] negative; a contrary burst must close the second bar far short of
        // the target (with alpha_b = 2/11 the threshold falls below theta on the 3rd buy).
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
        // Strictly alternating flow keeps |theta| <= 1 while E[b] converges to +0.25 (target 4, alpha_b
        // 0.4), so the threshold stays above 1 and only the 3x-target cap closes the bar.
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
        // After the warm-up close, the next trade starts a fresh bar: theta and count were reset. A trade
        // with the primed expectation extends the bar; a contrary one would close it.
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
        // Once one-sided flow becomes the expectation (E[b] -> 1), bars re-lengthen toward E[T] instead
        // of collapsing to 1-trade bars.
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

    /// An adaptive rule has no fixed threshold to count toward; a countdown would promise a close it
    /// will not make.
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

    /// The token vocabulary is owned here; the round trip pins emitting and parsing together.
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

    /// `new` must stay a pure delegation to `with_unit(_, Trades)`. This cannot certify the historical
    /// behavior (both sides run today's code); the golden fixture in `tests/golden_imbalance.rs` does.
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

    /// Volume imbalance weighs θ by size: a half-lot contrary print stays inside, a ten-lot closes.
    /// Weighted units judge a trade against the expectations from before it.
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

    /// Dollar imbalance weighs θ by notional: with threshold 348.16 after the price-100 warm-up, a
    /// one-lot contrary print stays inside at price 300 and closes at 400.
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

    /// The warm-up bar and the hard cap count *trades* in every unit.
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

        // Cap: alternating constant-size flow keeps |θ| <= 1000 while the floor holds the threshold at
        // round(sqrt(80))·E[w] = 9000, so only the 3x-target cap closes the bar.
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

    /// The closing rule is scale-free in the weight: nine times the size closes bars at the same
    /// trades, since θ, E[s] and the floor (a multiple of the typical weight) scale together.
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

    /// The feed-arithmetic policy holds in weighted units: prints whose notional saturates `Decimal`
    /// must never panic the builder. `E[s]` rides toward `Decimal::MAX`; the saturating threshold means
    /// "no imbalance close" and the cap still bounds every bar.
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

    /// A size unit over a tape with no size (all weights zero) must not cascade one-trade bars; only
    /// the trade cap closes such a bar.
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

    /// A signed export weighs by magnitude: direction comes from the aggressor side, so a negative
    /// print must not cancel a buy or drive the `E[w]` floor negative.
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
