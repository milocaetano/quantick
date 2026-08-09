//! Per-bar bid×ask footprint ladders and the pure signals over them.
//!
//! A footprint answers "at which prices did the buyers and sellers act inside
//! this bar?" — the per-price-level split that [`Bar`](crate::Bar) deliberately
//! does not carry (bars are equality-compared in golden tests and copied by
//! value everywhere; the ladder is opt-in and lives beside the bar, never
//! inside it).
//!
//! The design mirrors the engine's determinism rules:
//!
//! - Levels are keyed by **zero-anchored integer buckets**
//!   (`floor(price / group)`), so two bars with the same grouping align row for
//!   row — the prerequisite for reading stacked imbalances across bars — and no
//!   float ever becomes a map key.
//! - The ladder is capped. A bar that would exceed
//!   [`level cap`](FootprintBuilder::new) doubles its grouping (an *integer*
//!   re-bucket, so the merge is exact) and reports itself
//!   [`aggregated`](BarFootprint::is_aggregated) — coarser data is labeled,
//!   never silently patched.
//! - Accumulation saturates instead of panicking: quantities come from an
//!   untrusted feed, the same rule [`Bar`](crate::Bar) follows.
//!
//! The *builder* knows nothing about bar boundaries: the caller that already
//! drives a [`BarBuilder`](crate::BarBuilder) pushes the same trades here and
//! calls [`close`](FootprintBuilder::close) whenever its bar closes. That keeps
//! one bucketing rule (the bar builder's) and one ladder fold (this one), and
//! lets chart, backtest and bot read identical footprints from identical
//! trades.
//!
//! Signal functions ([`poc`](BarFootprint::poc),
//! [`imbalances`](BarFootprint::imbalances),
//! [`stacked_zones`](BarFootprint::stacked_zones),
//! [`extreme_ratio`](BarFootprint::extreme_ratio)) are pure reads over a
//! finished ladder. They are context tools for a human or a strategy to weigh —
//! none of them claims standalone predictive edge, and the engine attaches no
//! thresholds of its own: every cutoff is a caller-supplied parameter.

use std::collections::BTreeMap;

use rust_decimal::{Decimal, RoundingStrategy};

use crate::{Side, Trade};

/// Decimal places an approximated ladder's per-row share is truncated to.
/// Deeper than any venue's quantity step, shallow enough that a truncated
/// share times a full ladder of rows never overdraws the bar's own total.
const APPROX_SHARE_DECIMALS: u32 = 12;

/// Default ladder size cap: beyond this many price levels in one bar the
/// grouping doubles. Generous for every realistic bar (a dense Binance tick
/// bar prints a few hundred distinct levels) while bounding memory against a
/// corrupt feed spraying prices across the whole `Decimal` range.
pub const DEFAULT_LEVEL_CAP: usize = 2000;

/// The buy/sell split of one price level of one bar.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FootprintLevel {
    /// Quantity traded by taker-buys in this level.
    pub buy: Decimal,
    /// Quantity traded by taker-sells in this level.
    pub sell: Decimal,
    /// Number of trades that printed in this level.
    pub trade_count: u64,
}

impl FootprintLevel {
    /// Total traded quantity at this level. Saturates on overflow.
    #[must_use]
    pub fn volume(&self) -> Decimal {
        self.buy.saturating_add(self.sell)
    }

    /// Order-flow delta at this level: `buy - sell`. Saturates on overflow.
    #[must_use]
    pub fn delta(&self) -> Decimal {
        self.buy.saturating_sub(self.sell)
    }
}

/// Which end of a bar's ladder [`extreme_ratio`](BarFootprint::extreme_ratio)
/// inspects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extreme {
    /// The lowest traded level of the bar.
    Low,
    /// The highest traded level of the bar.
    High,
}

/// One diagonal imbalance: at `bucket`, one side overwhelmed the other side's
/// diagonal neighbour. See [`BarFootprint::imbalances`] for the exact rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Imbalance {
    /// The bucket whose aggressors dominated.
    pub bucket: i64,
    /// The dominating side.
    pub side: Side,
}

/// A run of at least N consecutive same-side [`Imbalance`]s — the "stacked
/// imbalance" zone flow traders treat as a level with memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackedZone {
    /// Lowest bucket of the run (inclusive).
    pub low_bucket: i64,
    /// Highest bucket of the run (inclusive).
    pub high_bucket: i64,
    /// The side every imbalance in the run shares.
    pub side: Side,
}

/// The finished (or in-progress) footprint ladder of one bar.
///
/// Obtained from [`FootprintBuilder`]; whether it is closed is known from
/// *where* you obtained it, exactly like [`Bar`](crate::Bar).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarFootprint {
    levels: BTreeMap<i64, FootprintLevel>,
    base_group: Decimal,
    /// Power-of-two doublings applied on top of `base_group`.
    doublings: u32,
}

impl BarFootprint {
    fn new(base_group: Decimal) -> Self {
        Self {
            levels: BTreeMap::new(),
            base_group,
            doublings: 0,
        }
    }

    /// An **approximated** ladder built from a bar's summary alone — for
    /// bars with no tape behind them (venue history candles), where the real
    /// per-price distribution is unknowable.
    ///
    /// The bar's volume is spread uniformly over the buckets its low–high
    /// range crosses, each side separately (a venue that reports the
    /// taker-buy split — Binance klines do — keeps its real side totals);
    /// division remainders land in the close's bucket, so every total is
    /// conserved *exactly*: folding approximated ladders into a
    /// [`VolumeProfile`](crate::VolumeProfile) sums to precisely the bars'
    /// own volumes. A range too wide for `level_cap` doubles the grouping
    /// first and reports itself [`aggregated`](Self::is_aggregated).
    ///
    /// The shape is honest about *totals* and approximate about *placement*
    /// — the consumer must label it so (data honesty); the engine cannot,
    /// because an approximated ladder reads identically to a real one on
    /// purpose: one fold, one profile, one code path.
    ///
    /// `None` when the bar traded nothing. # Panics — on a non-positive
    /// `base_group` or a zero `level_cap`, the same configuration contract
    /// as [`FootprintBuilder::new`].
    #[must_use]
    pub fn approximated(bar: &crate::Bar, base_group: Decimal, level_cap: usize) -> Option<Self> {
        assert!(
            base_group > Decimal::ZERO,
            "footprint base group must be positive"
        );
        assert!(level_cap > 0, "footprint level cap must be positive");
        let volume = bar.buy_volume.saturating_add(bar.sell_volume);
        if volume <= Decimal::ZERO {
            return None;
        }
        // Widen the grouping until the bar's price span fits the cap — the
        // same bound the trade fold enforces, decided up front because the
        // span is known before any bucket exists.
        let mut ladder = Self::new(base_group);
        let (lo, hi) = loop {
            let group = ladder.group();
            let lo = bucket_of(bar.low.min(bar.high), group);
            let hi = bucket_of(bar.high.max(bar.low), group);
            let span = hi.saturating_sub(lo).saturating_add(1);
            if span >= 0 && (span as u128) <= level_cap as u128 {
                break (lo, hi);
            }
            ladder.doublings += 1;
        };
        let close_bucket = bucket_of(bar.close, ladder.group()).clamp(lo, hi);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rows = (hi - lo + 1) as u64;
        let rows_dec = Decimal::from(rows);
        // Per-row shares rounded *down*, remainders to the close's row:
        // totals conserve exactly, whatever the divisions truncated — a
        // share rounded up would overdraw the total by an epsilon and break
        // the conservation the fold is trusted for.
        let share = |total: Decimal| {
            total
                .checked_div(rows_dec)
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(APPROX_SHARE_DECIMALS, RoundingStrategy::ToZero)
        };
        let buy_share = share(bar.buy_volume);
        let sell_share = share(bar.sell_volume);
        let count_share = bar.trade_count / rows;
        for bucket in lo..=hi {
            ladder.levels.insert(
                bucket,
                FootprintLevel {
                    buy: buy_share,
                    sell: sell_share,
                    trade_count: count_share,
                },
            );
        }
        let close_level = ladder
            .levels
            .get_mut(&close_bucket)
            .expect("close bucket is clamped into the spread range");
        close_level.buy = bar
            .buy_volume
            .saturating_sub(buy_share.saturating_mul(Decimal::from(rows - 1)));
        close_level.sell = bar
            .sell_volume
            .saturating_sub(sell_share.saturating_mul(Decimal::from(rows - 1)));
        close_level.trade_count = bar.trade_count - count_share * (rows - 1);
        Some(ladder)
    }

    /// The ladder, lowest bucket first. Keys are `floor(price / group())`.
    #[must_use]
    pub fn levels(&self) -> &BTreeMap<i64, FootprintLevel> {
        &self.levels
    }

    /// The price width of one bucket: the builder's base grouping times every
    /// doubling the level cap forced.
    #[must_use]
    pub fn group(&self) -> Decimal {
        let mut group = self.base_group;
        for _ in 0..self.doublings {
            group = group.saturating_mul(Decimal::TWO);
        }
        group
    }

    /// The lower price bound of `bucket` under the current grouping.
    #[must_use]
    pub fn bucket_price(&self, bucket: i64) -> Decimal {
        self.group().saturating_mul(Decimal::from(bucket))
    }

    /// True when the level cap forced a coarser grouping than the builder's
    /// base. Consumers must surface this — a coarsened ladder reads the same
    /// as a native one and the difference is never patched silently.
    #[must_use]
    pub fn is_aggregated(&self) -> bool {
        self.doublings > 0
    }

    /// The builder's base grouping, before any cap-forced doubling. Crate-only:
    /// [`VolumeProfile::merge`](crate::VolumeProfile::merge) needs it to refuse
    /// ladders whose buckets never aligned in the first place.
    pub(crate) fn base_group(&self) -> Decimal {
        self.base_group
    }

    /// Power-of-two doublings applied on top of the base group. Crate-only:
    /// the profile merge folds finer ladders down by exactly this difference.
    pub(crate) fn doublings(&self) -> u32 {
        self.doublings
    }

    /// Point of control: the bucket with the highest total volume.
    ///
    /// Ties break toward the **lowest** bucket, deterministically: iteration
    /// is ascending and only a strictly greater volume replaces the candidate.
    #[must_use]
    pub fn poc(&self) -> Option<i64> {
        let mut best: Option<(i64, Decimal)> = None;
        for (&bucket, level) in &self.levels {
            let volume = level.volume();
            match best {
                Some((_, best_volume)) if volume <= best_volume => {}
                _ => best = Some((bucket, volume)),
            }
        }
        best.map(|(bucket, _)| bucket)
    }

    /// Diagonal imbalances under a caller-supplied rule.
    ///
    /// The comparison is diagonal because the two sides of one price did not
    /// compete — they crossed. A taker-buy printing at level `p` competed with
    /// the taker-sells one level *below*, so:
    ///
    /// - **buy** imbalance at `p`: `buy(p) >= ratio * sell(p - 1)` **and**
    ///   `buy(p) - sell(p - 1) >= min_qty`;
    /// - **sell** imbalance at `p`: `sell(p) >= ratio * buy(p + 1)` **and**
    ///   `sell(p) - buy(p + 1) >= min_qty`.
    ///
    /// A missing neighbour counts as zero, which makes the ratio test vacuous
    /// there — `min_qty` is what separates signal from a 4-vs-1-lot artefact,
    /// on empty neighbours and thin ones alike. A bucket may satisfy both
    /// sides; both entries are returned, ascending by bucket, buy before sell
    /// within one bucket.
    #[must_use]
    pub fn imbalances(&self, ratio: Decimal, min_qty: Decimal) -> Vec<Imbalance> {
        let side_qty = |bucket: i64, side: Side| -> Decimal {
            self.levels
                .get(&bucket)
                .map(|level| match side {
                    Side::Buy => level.buy,
                    Side::Sell => level.sell,
                })
                .unwrap_or(Decimal::ZERO)
        };
        let dominates = |qty: Decimal, other: Decimal| -> bool {
            qty >= ratio.saturating_mul(other) && qty.saturating_sub(other) >= min_qty
        };

        let mut found = Vec::new();
        for (&bucket, level) in &self.levels {
            if dominates(level.buy, side_qty(bucket - 1, Side::Sell)) {
                found.push(Imbalance {
                    bucket,
                    side: Side::Buy,
                });
            }
            if dominates(level.sell, side_qty(bucket + 1, Side::Buy)) {
                found.push(Imbalance {
                    bucket,
                    side: Side::Sell,
                });
            }
        }
        found
    }

    /// Maximal runs of at least `min_run` *consecutive* same-side imbalances.
    ///
    /// Consecutive means adjacent buckets — a level that printed nothing
    /// breaks the stack, as does one that merely failed the imbalance rule.
    /// Zones are returned ascending by `low_bucket`, buy zones before sell
    /// zones at equal starts.
    #[must_use]
    pub fn stacked_zones(
        &self,
        ratio: Decimal,
        min_qty: Decimal,
        min_run: usize,
    ) -> Vec<StackedZone> {
        let imbalances = self.imbalances(ratio, min_qty);
        let mut zones = Vec::new();
        for side in [Side::Buy, Side::Sell] {
            let buckets: Vec<i64> = imbalances
                .iter()
                .filter(|i| i.side == side)
                .map(|i| i.bucket)
                .collect();
            let mut run_start = 0usize;
            for i in 0..buckets.len() {
                let run_breaks = i + 1 == buckets.len() || buckets[i + 1] != buckets[i] + 1;
                if run_breaks {
                    if i + 1 - run_start >= min_run.max(1) {
                        zones.push(StackedZone {
                            low_bucket: buckets[run_start],
                            high_bucket: buckets[i],
                            side,
                        });
                    }
                    run_start = i + 1;
                }
            }
        }
        zones.sort_by_key(|zone| (zone.low_bucket, zone.side == Side::Sell));
        zones
    }

    /// The aggression ratio at one extreme of the bar: dominant side over the
    /// other side at the lowest (or highest) traded level — the number a
    /// footprint chart badges beside a candle's extreme as an exhaustion or
    /// absorption cue.
    ///
    /// Returns `None` for an empty ladder **and** when the smaller side is
    /// zero: a one-sided extreme has no finite ratio, and inventing a large
    /// stand-in would be a silent lie — the caller can read the raw
    /// [`levels`](BarFootprint::levels) and say "one-sided" honestly.
    #[must_use]
    pub fn extreme_ratio(&self, extreme: Extreme) -> Option<Decimal> {
        let (_, level) = match extreme {
            Extreme::Low => self.levels.iter().next()?,
            Extreme::High => self.levels.iter().next_back()?,
        };
        let (dominant, other) = if level.buy >= level.sell {
            (level.buy, level.sell)
        } else {
            (level.sell, level.buy)
        };
        if other.is_zero() {
            return None;
        }
        // `checked_div` for the same reason the fold saturates: quantities
        // come from an untrusted feed, and a ratio too large for `Decimal`
        // must read as "no finite ratio", never as a panic.
        dominant.checked_div(other)
    }

    fn fold(&mut self, trade: &Trade, level_cap: usize) {
        let bucket = bucket_of(trade.price, self.group());
        let level = self.levels.entry(bucket).or_default();
        match trade.side {
            Side::Buy => level.buy = level.buy.saturating_add(trade.quantity),
            Side::Sell => level.sell = level.sell.saturating_add(trade.quantity),
        }
        level.trade_count += 1;

        while self.levels.len() > level_cap {
            self.coarsen();
        }
    }

    /// Double the grouping: merge bucket pairs by integer halving. Exact —
    /// `floor(floor(p / g) / 2) == floor(p / 2g)` — so the coarser ladder is
    /// the one a builder with the doubled base group would have produced.
    fn coarsen(&mut self) {
        let mut merged: BTreeMap<i64, FootprintLevel> = BTreeMap::new();
        for (bucket, level) in std::mem::take(&mut self.levels) {
            let target = merged.entry(bucket.div_euclid(2)).or_default();
            target.buy = target.buy.saturating_add(level.buy);
            target.sell = target.sell.saturating_add(level.sell);
            target.trade_count += level.trade_count;
        }
        self.levels = merged;
        self.doublings += 1;
    }
}

/// Accumulates one [`BarFootprint`] at a time from the trade stream the bar
/// builders already consume. See the [module docs](self) for the contract.
#[derive(Debug, Clone)]
pub struct FootprintBuilder {
    base_group: Decimal,
    level_cap: usize,
    partial: Option<BarFootprint>,
}

impl FootprintBuilder {
    /// A builder bucketing prices into rows `base_group` wide (normally the
    /// instrument's tick size), holding at most `level_cap` rows per bar
    /// before the grouping doubles.
    ///
    /// # Panics
    ///
    /// Panics if `base_group` is not positive or `level_cap` is zero — both
    /// are configuration errors, not feed input.
    #[must_use]
    pub fn new(base_group: Decimal, level_cap: usize) -> Self {
        assert!(
            base_group > Decimal::ZERO,
            "footprint base group must be positive"
        );
        assert!(level_cap > 0, "footprint level cap must be positive");
        Self {
            base_group,
            level_cap,
            partial: None,
        }
    }

    /// Fold `trade` into the current bar's ladder.
    pub fn push(&mut self, trade: &Trade) {
        let partial = self
            .partial
            .get_or_insert_with(|| BarFootprint::new(self.base_group));
        partial.fold(trade, self.level_cap);
    }

    /// Close the current bar: hand out its ladder and start fresh. Call this
    /// whenever the bar builder driven by the same trades closes a bar (the
    /// closing trade included). `None` when no trade arrived since the last
    /// close.
    pub fn close(&mut self) -> Option<BarFootprint> {
        self.partial.take()
    }

    /// The in-progress ladder, if any trade arrived since the last close.
    #[must_use]
    pub fn partial(&self) -> Option<&BarFootprint> {
        self.partial.as_ref()
    }
}

/// `floor(price / group)`, saturated to `i64` so a corrupt feed price cannot
/// panic the fold. Zero-anchored: bars sharing a grouping share bucket
/// boundaries, whatever their own price ranges.
fn bucket_of(price: Decimal, group: Decimal) -> i64 {
    // `checked_div`: a price near `Decimal::MAX` over a sub-unit group
    // overflows the division, and a corrupt print must land in a saturated
    // bucket, not panic the fold (same rule as the accumulation).
    let Some(quotient) = price.checked_div(group).map(|q| q.floor()) else {
        return if price.is_sign_negative() {
            i64::MIN
        } else {
            i64::MAX
        };
    };
    quotient
        .try_into()
        .unwrap_or(if quotient.is_sign_negative() {
            i64::MIN
        } else {
            i64::MAX
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn trade(agg_id: u64, price: &str, quantity: &str, side: Side) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1_700_000_000_000 + agg_id as i64 * 10,
            price: dec(price),
            quantity: dec(quantity),
            side,
        }
    }

    fn ladder(trades: &[Trade], group: &str) -> BarFootprint {
        let mut builder = FootprintBuilder::new(dec(group), DEFAULT_LEVEL_CAP);
        for t in trades {
            builder.push(t);
        }
        builder.close().expect("fixture pushed at least one trade")
    }

    #[test]
    fn buckets_are_zero_anchored_floors_of_price_over_group() {
        // Group 0.5: 100.0 -> 200, 100.4 -> 200, 100.5 -> 201, 99.9 -> 199.
        let fp = ladder(
            &[
                trade(0, "100.0", "1", Side::Buy),
                trade(1, "100.4", "2", Side::Sell),
                trade(2, "100.5", "3", Side::Buy),
                trade(3, "99.9", "4", Side::Sell),
            ],
            "0.5",
        );
        let buckets: Vec<i64> = fp.levels().keys().copied().collect();
        assert_eq!(buckets, vec![199, 200, 201]);
        assert_eq!(fp.levels()[&200].buy, dec("1"));
        assert_eq!(fp.levels()[&200].sell, dec("2"));
        assert_eq!(fp.levels()[&200].trade_count, 2);
        assert_eq!(fp.levels()[&201].buy, dec("3"));
        assert_eq!(fp.levels()[&199].sell, dec("4"));
        assert_eq!(fp.bucket_price(200), dec("100.0"));
        assert!(!fp.is_aggregated());
    }

    #[test]
    fn same_trades_produce_equal_footprints() {
        let trades = [
            trade(0, "36000.1", "1.5", Side::Buy),
            trade(1, "36000.2", "0.5", Side::Sell),
            trade(2, "35999.9", "2.25", Side::Buy),
        ];
        assert_eq!(ladder(&trades, "0.1"), ladder(&trades, "0.1"));
    }

    #[test]
    fn close_resets_and_partial_tracks_the_open_bar() {
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        assert!(builder.partial().is_none());
        assert!(builder.close().is_none());

        builder.push(&trade(0, "10", "1", Side::Buy));
        assert_eq!(builder.partial().unwrap().levels().len(), 1);
        let closed = builder.close().unwrap();
        assert_eq!(closed.levels()[&10].buy, dec("1"));

        assert!(builder.partial().is_none());
        builder.push(&trade(1, "11", "2", Side::Sell));
        let next = builder.close().unwrap();
        assert!(!next.levels().contains_key(&10), "close must start fresh");
        assert_eq!(next.levels()[&11].sell, dec("2"));
    }

    #[test]
    fn exceeding_the_level_cap_doubles_the_group_and_says_so() {
        // Cap 4, group 1: six one-unit levels force two levels per bucket.
        let mut builder = FootprintBuilder::new(dec("1"), 4);
        for i in 0..6u64 {
            builder.push(&trade(i, &format!("{}", 100 + i), "1", Side::Buy));
        }
        let fp = builder.close().unwrap();
        assert!(fp.is_aggregated());
        assert_eq!(fp.group(), dec("2"));
        // Nothing was lost in the merge: total volume and trade count hold.
        let total: Decimal = fp.levels().values().map(|l| l.buy).sum();
        assert_eq!(total, dec("6"));
        let trades_total: u64 = fp.levels().values().map(|l| l.trade_count).sum();
        assert_eq!(trades_total, 6);
        // 100..=105 under group 2 is buckets 50, 51, 52 — pairs merged exactly.
        let buckets: Vec<i64> = fp.levels().keys().copied().collect();
        assert_eq!(buckets, vec![50, 51, 52]);
        assert_eq!(fp.levels()[&50].buy, dec("2"));
    }

    #[test]
    fn poc_takes_the_highest_volume_and_ties_break_low() {
        let fp = ladder(
            &[
                trade(0, "10", "3", Side::Buy),
                trade(1, "11", "2", Side::Sell),
                trade(2, "12", "3", Side::Sell),
            ],
            "1",
        );
        // 10 and 12 tie at volume 3; the lower bucket wins, deterministically.
        assert_eq!(fp.poc(), Some(10));
    }

    #[test]
    fn diagonal_imbalance_requires_both_ratio_and_min_qty() {
        // Level 10: buy 30. Level 9: sell 5. Diagonal 30 vs 5: ratio 6 >= 3
        // and difference 25 >= 20 -> buy imbalance at 10.
        // Level 9 sell 5 vs level 10 buy 30: no sell imbalance anywhere.
        let trades = [
            trade(0, "10", "30", Side::Buy),
            trade(1, "9", "5", Side::Sell),
        ];
        let fp = ladder(&trades, "1");
        assert_eq!(
            fp.imbalances(dec("3"), dec("20")),
            vec![Imbalance {
                bucket: 10,
                side: Side::Buy
            }]
        );
        // Raise min_qty past the difference and the same ladder goes quiet.
        assert!(fp.imbalances(dec("3"), dec("26")).is_empty());
    }

    #[test]
    fn empty_diagonal_neighbour_is_gated_by_min_qty_alone() {
        // A lone 25-lot buy print with nothing below: ratio test is vacuous
        // against zero, min_qty 20 lets it through, min_qty 30 does not.
        let trades = [trade(0, "10", "25", Side::Buy)];
        let fp = ladder(&trades, "1");
        assert_eq!(
            fp.imbalances(dec("3"), dec("20")),
            vec![Imbalance {
                bucket: 10,
                side: Side::Buy
            }]
        );
        assert!(fp.imbalances(dec("3"), dec("30")).is_empty());
    }

    #[test]
    fn stacked_zones_need_consecutive_same_side_runs() {
        // Buy imbalances at 10, 11, 12 (each buy dwarfs the sell below);
        // an isolated one at 15 must not form a zone of its own.
        let trades = [
            trade(0, "9", "1", Side::Sell),
            trade(1, "10", "30", Side::Buy),
            trade(2, "11", "30", Side::Buy),
            trade(3, "12", "30", Side::Buy),
            trade(4, "15", "30", Side::Buy),
        ];
        let fp = ladder(&trades, "1");
        assert_eq!(
            fp.stacked_zones(dec("3"), dec("20"), 3),
            vec![StackedZone {
                low_bucket: 10,
                high_bucket: 12,
                side: Side::Buy
            }]
        );
        // A run of two is not a stack of three.
        assert!(
            ladder(
                &[
                    trade(0, "10", "30", Side::Buy),
                    trade(1, "11", "30", Side::Buy),
                ],
                "1"
            )
            .stacked_zones(dec("3"), dec("20"), 3)
            .is_empty()
        );
    }

    #[test]
    fn extreme_ratio_reads_the_ladder_ends_and_refuses_one_sided_levels() {
        // Low level: sell 49.1 vs buy 5 -> 9.82, the reference badge value.
        let fp = ladder(
            &[
                trade(0, "100", "49.1", Side::Sell),
                trade(1, "100", "5", Side::Buy),
                trade(2, "101", "1", Side::Buy),
            ],
            "1",
        );
        assert_eq!(fp.extreme_ratio(Extreme::Low), Some(dec("9.82")));
        // High level is buy-only: no finite ratio, and no invented stand-in.
        assert_eq!(fp.extreme_ratio(Extreme::High), None);
    }

    /// A corrupt print near `Decimal::MAX` over a sub-unit group overflows
    /// the bucket division; it must land in a saturated bucket, and a
    /// one-sided extreme whose ratio overflows must read as "no finite
    /// ratio" — never a panic, on either path (feed input is untrusted).
    #[test]
    fn absurd_feed_values_saturate_instead_of_panicking() {
        let mut builder = FootprintBuilder::new(dec("0.01"), DEFAULT_LEVEL_CAP);
        builder.push(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: Decimal::MAX,
            quantity: dec("1"),
            side: Side::Buy,
        });
        builder.push(&Trade {
            agg_id: 1,
            timestamp_ms: 1,
            price: Decimal::MIN,
            quantity: dec("1"),
            side: Side::Sell,
        });
        let fp = builder.close().unwrap();
        let buckets: Vec<i64> = fp.levels().keys().copied().collect();
        assert_eq!(buckets, vec![i64::MIN, i64::MAX]);

        // Ratio overflow: MAX over a tiny opposite side has no Decimal home.
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        builder.push(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: dec("100"),
            quantity: Decimal::MAX,
            side: Side::Sell,
        });
        builder.push(&Trade {
            agg_id: 1,
            timestamp_ms: 1,
            price: dec("100"),
            quantity: dec("0.0000000000000000000000000001"),
            side: Side::Buy,
        });
        assert_eq!(builder.close().unwrap().extreme_ratio(Extreme::Low), None);
    }

    fn venue_bar(
        low: &str,
        high: &str,
        close: &str,
        buy: &str,
        sell: &str,
        trades: u64,
    ) -> crate::Bar {
        crate::Bar {
            open_time: 1_700_000_000_000,
            close_time: 1_700_000_060_000,
            open: dec(low),
            high: dec(high),
            low: dec(low),
            close: dec(close),
            buy_volume: dec(buy),
            sell_volume: dec(sell),
            trade_count: trades,
        }
    }

    /// The approximated ladder conserves every total exactly: shares are
    /// rounded down and the remainders land in the close's row, so folding
    /// approximated bars sums to precisely the bars' own volumes — the
    /// property the range profile trusts.
    #[test]
    fn an_approximated_ladder_conserves_volume_sides_and_count_exactly() {
        // 1.0 buy over three $1 rows: 0.333… truncated, remainder to close.
        let bar = venue_bar("100", "102.9", "100.5", "1", "0.2", 7);
        let ladder = BarFootprint::approximated(&bar, dec("1"), DEFAULT_LEVEL_CAP).unwrap();

        let buckets: Vec<i64> = ladder.levels().keys().copied().collect();
        assert_eq!(buckets, vec![100, 101, 102]);
        let total_buy: Decimal = ladder.levels().values().map(|l| l.buy).sum();
        let total_sell: Decimal = ladder.levels().values().map(|l| l.sell).sum();
        let total_count: u64 = ladder.levels().values().map(|l| l.trade_count).sum();
        assert_eq!(total_buy, dec("1"), "buy conserved exactly");
        assert_eq!(total_sell, dec("0.2"), "sell conserved exactly");
        assert_eq!(total_count, 7, "trade count conserved exactly");
        // The close's row (bucket 100) carries the remainders, so it is the
        // heaviest — every other row holds the truncated share.
        assert!(ladder.levels()[&100].buy > ladder.levels()[&101].buy);
        assert_eq!(ladder.levels()[&101], ladder.levels()[&102]);
        assert!(!ladder.is_aggregated());
    }

    #[test]
    fn a_one_row_candle_puts_everything_in_that_row() {
        let bar = venue_bar("100.2", "100.7", "100.4", "3", "2", 5);
        let ladder = BarFootprint::approximated(&bar, dec("1"), DEFAULT_LEVEL_CAP).unwrap();
        assert_eq!(ladder.levels().len(), 1);
        assert_eq!(ladder.levels()[&100].buy, dec("3"));
        assert_eq!(ladder.levels()[&100].sell, dec("2"));
        assert_eq!(ladder.levels()[&100].trade_count, 5);
    }

    #[test]
    fn a_bar_that_traded_nothing_yields_no_ladder() {
        assert!(
            BarFootprint::approximated(
                &venue_bar("100", "101", "100", "0", "0", 0),
                dec("1"),
                DEFAULT_LEVEL_CAP
            )
            .is_none()
        );
    }

    /// A span wider than the cap coarsens the grouping up front and says so
    /// — same contract as the trade fold's cap, same honesty flag.
    #[test]
    fn an_oversized_span_coarsens_and_says_so() {
        let bar = venue_bar("100", "163", "120", "8", "0", 8);
        let ladder = BarFootprint::approximated(&bar, dec("1"), 32).unwrap();
        assert!(ladder.is_aggregated());
        assert_eq!(ladder.group(), dec("2"));
        let total: Decimal = ladder.levels().values().map(|l| l.buy).sum();
        assert_eq!(total, dec("8"));
        // An approximated ladder folds into a profile beside real ones.
        let profile = crate::VolumeProfile::merge([&ladder], DEFAULT_LEVEL_CAP).unwrap();
        assert_eq!(profile.total_volume(), dec("8"));
    }

    /// The doubled ladder equals the ladder a builder with the doubled base
    /// group would have produced — the merge is exact, not approximate.
    #[test]
    fn coarsening_matches_a_natively_coarser_builder() {
        let trades: Vec<Trade> = (0..8u64)
            .map(|i| {
                trade(
                    i,
                    &format!("{}.5", 200 + i),
                    "1.25",
                    if i % 2 == 0 { Side::Buy } else { Side::Sell },
                )
            })
            .collect();

        let mut capped = FootprintBuilder::new(dec("1"), 4);
        for t in &trades {
            capped.push(t);
        }
        let coarse = capped.close().unwrap();
        let native = ladder(&trades, "2");

        assert!(coarse.is_aggregated());
        assert_eq!(coarse.group(), native.group());
        assert_eq!(coarse.levels(), native.levels());
    }
}
