//! Volume-by-price profiles over a range of bars, and the value area over them.
//!
//! A [`VolumeProfile`] is the exact fold of N per-bar
//! [`BarFootprint`](crate::BarFootprint) ladders into one ladder: at which
//! prices did the whole *range* trade, not one bar. Because footprint buckets
//! are zero-anchored (`floor(price / group)`), ladders that share a grouping
//! align row for row and the merge is a plain `BTreeMap` sum — no resampling,
//! no interpolation, no float keys.
//!
//! The same determinism and honesty rules as the footprint apply:
//!
//! - Ladders whose cap forced different doublings merge at the **coarsest**
//!   grouping among them; finer ladders fold down by exact integer halving
//!   (the identity behind [`coarsen`]: `floor(floor(p/g)/2) == floor(p/2g)`).
//!   Ladders with different *base* groups never aligned at all and the merge
//!   refuses (`None`) instead of inventing rows.
//! - The merged ladder is capped like a bar's: past `level_cap` rows the
//!   grouping doubles and the profile reports itself
//!   [`aggregated`](VolumeProfile::is_aggregated) — as it also does when any
//!   input ladder already was.
//! - Accumulation saturates instead of panicking; quantities come from an
//!   untrusted feed.
//!
//! [`value_area`](VolumeProfile::value_area) is the classic profile read: the
//! narrowest band of rows around the [`poc`](VolumeProfile::poc) holding a
//! caller-supplied fraction of the range's volume (the engine attaches no
//! thresholds of its own — 70% is the caller's convention, not this module's).
//!
//! [`coarsen`]: VolumeProfile::coarsen

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::{BarFootprint, FootprintLevel};

/// The value area of a [`VolumeProfile`]: the contiguous row band around the
/// point of control holding the requested volume fraction. All three fields
/// are buckets under the profile's [`group`](VolumeProfile::group), inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueArea {
    /// Point of control: the highest-volume bucket of the profile.
    pub poc: i64,
    /// Value area high: the highest bucket inside the area.
    pub vah: i64,
    /// Value area low: the lowest bucket inside the area.
    pub val: i64,
}

/// The volume-by-price ladder of a range of bars. Obtained from
/// [`VolumeProfile::merge`]; pure data, like the footprint it is folded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeProfile {
    levels: BTreeMap<i64, FootprintLevel>,
    group: Decimal,
    aggregated: bool,
}

impl VolumeProfile {
    /// Fold `ladders` into one profile, capped at `level_cap` rows.
    ///
    /// The result's grouping is the coarsest among the inputs (finer ladders
    /// fold down exactly); if the merged ladder still exceeds `level_cap` the
    /// grouping doubles until it fits, and the profile reports itself
    /// [`aggregated`](Self::is_aggregated) — as it also does when any input
    /// already was.
    ///
    /// `None` when `ladders` is empty, and when the inputs disagree on their
    /// *base* grouping — such ladders never shared bucket boundaries, and a
    /// profile over them would be an invention, not a fold.
    ///
    /// # Panics
    ///
    /// Panics if `level_cap` is zero — a configuration error, not feed input,
    /// the same contract as [`FootprintBuilder::new`](crate::FootprintBuilder::new).
    #[must_use]
    pub fn merge<'a>(
        ladders: impl IntoIterator<Item = &'a BarFootprint>,
        level_cap: usize,
    ) -> Option<Self> {
        assert!(level_cap > 0, "profile level cap must be positive");
        let ladders: Vec<&BarFootprint> = ladders.into_iter().collect();
        let first = *ladders.first()?;
        let base_group = first.base_group();
        if ladders.iter().any(|l| l.base_group() != base_group) {
            return None;
        }

        let doublings = ladders.iter().map(|l| l.doublings()).max().unwrap_or(0);
        let mut profile = Self {
            levels: BTreeMap::new(),
            group: first.group(),
            aggregated: doublings > 0,
        };
        // The coarsest input's group, recomputed the way the footprint does
        // (repeated doubling), so the two stay bit-identical.
        profile.group = base_group;
        for _ in 0..doublings {
            profile.group = profile.group.saturating_mul(Decimal::TWO);
        }

        for ladder in ladders {
            let shift = doublings - ladder.doublings();
            for (&bucket, level) in ladder.levels() {
                let target = profile
                    .levels
                    .entry(fold_bucket(bucket, shift))
                    .or_default();
                target.buy = target.buy.saturating_add(level.buy);
                target.sell = target.sell.saturating_add(level.sell);
                target.trade_count += level.trade_count;
            }
        }

        while profile.levels.len() > level_cap {
            profile.coarsen();
        }
        Some(profile)
    }

    /// The ladder, lowest bucket first. Keys are `floor(price / group())`.
    #[must_use]
    pub fn levels(&self) -> &BTreeMap<i64, FootprintLevel> {
        &self.levels
    }

    /// The price width of one row of the profile.
    #[must_use]
    pub fn group(&self) -> Decimal {
        self.group
    }

    /// The lower price bound of `bucket` under the profile's grouping.
    #[must_use]
    pub fn bucket_price(&self, bucket: i64) -> Decimal {
        self.group.saturating_mul(Decimal::from(bucket))
    }

    /// True when this profile is coarser than the bars' base grouping —
    /// because an input ladder was capped, or because the merge itself was.
    /// Consumers must surface this; coarser data is labeled, never silently
    /// patched.
    #[must_use]
    pub fn is_aggregated(&self) -> bool {
        self.aggregated
    }

    /// Total traded quantity across the profile. Saturates on overflow.
    #[must_use]
    pub fn total_volume(&self) -> Decimal {
        self.levels
            .values()
            .fold(Decimal::ZERO, |acc, l| acc.saturating_add(l.volume()))
    }

    /// The highest single-row volume — the histogram's full-width row.
    #[must_use]
    pub fn max_level_volume(&self) -> Decimal {
        self.levels
            .values()
            .map(FootprintLevel::volume)
            .max()
            .unwrap_or(Decimal::ZERO)
    }

    /// Order-flow delta of the whole range: `buy - sell`. Saturates.
    #[must_use]
    pub fn total_delta(&self) -> Decimal {
        self.levels.values().fold(Decimal::ZERO, |acc, l| {
            acc.saturating_add(l.buy).saturating_sub(l.sell)
        })
    }

    /// Point of control: the bucket with the highest total volume. Ties break
    /// toward the **lowest** bucket, the same rule as
    /// [`BarFootprint::poc`](crate::BarFootprint::poc).
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

    /// The narrowest printed-row band around the POC holding at least
    /// `fraction` of [`total_volume`](Self::total_volume).
    ///
    /// Expansion follows the Sierra Chart / CQG convention: starting at the
    /// POC, compare the combined volume of the next **two** printed rows above
    /// against the next two below (a side with fewer rows left contributes
    /// what it has; an exhausted side contributes zero and the other side is
    /// taken), and expand into the larger pair. An exact tie expands
    /// **downward**, consistent with the POC's own tie-toward-lowest rule.
    /// Within the chosen pair rows are added one at a time and expansion stops
    /// as soon as the fraction is captured, so the area is minimal.
    ///
    /// Rows are the profile's *printed* buckets — price gaps where nothing
    /// traded are not filled in and cost nothing to cross.
    ///
    /// `None` on an empty profile. `fraction` is the caller's convention
    /// (0.70 is the classic); a fraction ≥ 1 covers every printed row.
    #[must_use]
    pub fn value_area(&self, fraction: Decimal) -> Option<ValueArea> {
        let poc = self.poc()?;
        let rows: Vec<(i64, Decimal)> = self
            .levels
            .iter()
            .map(|(&bucket, level)| (bucket, level.volume()))
            .collect();
        // The POC came from these same rows; position lookup cannot fail.
        let poc_idx = rows.iter().position(|&(bucket, _)| bucket == poc)?;

        let target = self.total_volume().saturating_mul(fraction);
        let mut lo = poc_idx;
        let mut hi = poc_idx;
        let mut captured = rows[poc_idx].1;

        while captured < target && (lo > 0 || hi + 1 < rows.len()) {
            let pair_above = row_volume(&rows, hi + 1).saturating_add(row_volume(&rows, hi + 2));
            let pair_below = if lo == 0 {
                Decimal::ZERO
            } else {
                row_volume(&rows, lo - 1).saturating_add(if lo >= 2 {
                    rows[lo - 2].1
                } else {
                    Decimal::ZERO
                })
            };
            let take_below = if lo == 0 {
                false
            } else if hi + 1 >= rows.len() {
                true
            } else {
                pair_below >= pair_above
            };

            if take_below {
                for _ in 0..lo.min(2) {
                    lo -= 1;
                    captured = captured.saturating_add(rows[lo].1);
                    if captured >= target {
                        break;
                    }
                }
            } else {
                for _ in 0..(rows.len() - 1 - hi).min(2) {
                    hi += 1;
                    captured = captured.saturating_add(rows[hi].1);
                    if captured >= target {
                        break;
                    }
                }
            }
        }

        Some(ValueArea {
            poc,
            vah: rows[hi].0,
            val: rows[lo].0,
        })
    }

    /// Double the grouping: merge bucket pairs by integer halving. The same
    /// exact identity as the footprint's coarsen.
    fn coarsen(&mut self) {
        let mut merged: BTreeMap<i64, FootprintLevel> = BTreeMap::new();
        for (bucket, level) in std::mem::take(&mut self.levels) {
            let target = merged.entry(bucket.div_euclid(2)).or_default();
            target.buy = target.buy.saturating_add(level.buy);
            target.sell = target.sell.saturating_add(level.sell);
            target.trade_count += level.trade_count;
        }
        self.levels = merged;
        self.group = self.group.saturating_mul(Decimal::TWO);
        self.aggregated = true;
    }
}

/// `bucket / 2^shift`, floored — the exact fold that lands a finer ladder's
/// bucket in the coarser grouping. A shift past the integer width collapses
/// every bucket to row 0 (or -1 below zero), which is where `floor` sends it.
fn fold_bucket(bucket: i64, shift: u32) -> i64 {
    if shift >= i64::BITS - 1 {
        if bucket < 0 { -1 } else { 0 }
    } else {
        bucket.div_euclid(1i64 << shift)
    }
}

fn row_volume(rows: &[(i64, Decimal)], idx: usize) -> Decimal {
    rows.get(idx).map(|&(_, v)| v).unwrap_or(Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DEFAULT_LEVEL_CAP, FootprintBuilder, Side, Trade};
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

    fn ladder_capped(trades: &[Trade], group: &str, cap: usize) -> BarFootprint {
        let mut builder = FootprintBuilder::new(dec(group), cap);
        for t in trades {
            builder.push(t);
        }
        builder.close().expect("fixture pushed at least one trade")
    }

    fn ladder(trades: &[Trade], group: &str) -> BarFootprint {
        ladder_capped(trades, group, DEFAULT_LEVEL_CAP)
    }

    /// Volumes laid out as `(price, buy, sell)` rows, folded through one bar.
    fn profile_of(rows: &[(&str, &str, &str)]) -> VolumeProfile {
        let mut trades = Vec::new();
        for (i, &(price, buy, sell)) in rows.iter().enumerate() {
            if dec(buy) > Decimal::ZERO {
                trades.push(trade(2 * i as u64, price, buy, Side::Buy));
            }
            if dec(sell) > Decimal::ZERO {
                trades.push(trade(2 * i as u64 + 1, price, sell, Side::Sell));
            }
        }
        VolumeProfile::merge([&ladder(&trades, "1")], DEFAULT_LEVEL_CAP).unwrap()
    }

    #[test]
    fn merge_is_the_exact_per_bucket_sum() {
        let a = ladder(
            &[
                trade(0, "100", "1", Side::Buy),
                trade(1, "101", "2", Side::Sell),
            ],
            "1",
        );
        let b = ladder(
            &[
                trade(2, "101", "3", Side::Buy),
                trade(3, "102", "4", Side::Sell),
            ],
            "1",
        );
        let profile = VolumeProfile::merge([&a, &b], DEFAULT_LEVEL_CAP).unwrap();

        let buckets: Vec<i64> = profile.levels().keys().copied().collect();
        assert_eq!(buckets, vec![100, 101, 102]);
        assert_eq!(profile.levels()[&100].buy, dec("1"));
        assert_eq!(profile.levels()[&101].buy, dec("3"));
        assert_eq!(profile.levels()[&101].sell, dec("2"));
        assert_eq!(profile.levels()[&101].trade_count, 2);
        assert_eq!(profile.levels()[&102].sell, dec("4"));
        assert_eq!(profile.group(), dec("1"));
        assert!(!profile.is_aggregated());
        assert_eq!(profile.total_volume(), dec("10"));
        assert_eq!(profile.total_delta(), dec("-2"));
        assert_eq!(profile.max_level_volume(), dec("5"));
    }

    #[test]
    fn merge_equals_one_builder_fed_all_trades() {
        // Exactness proof: folding trades through two bars and merging equals
        // folding them through one bar — the fold is associative.
        let trades: Vec<Trade> = (0..20u64)
            .map(|i| {
                trade(
                    i,
                    &format!("{}.5", 100 + (i % 7)),
                    "1.25",
                    if i % 3 == 0 { Side::Sell } else { Side::Buy },
                )
            })
            .collect();
        let (first, second) = trades.split_at(11);

        let split = VolumeProfile::merge(
            [&ladder(first, "0.5"), &ladder(second, "0.5")],
            DEFAULT_LEVEL_CAP,
        )
        .unwrap();
        let whole = VolumeProfile::merge([&ladder(&trades, "0.5")], DEFAULT_LEVEL_CAP).unwrap();

        assert_eq!(split.levels(), whole.levels());
        assert_eq!(split.group(), whole.group());
    }

    #[test]
    fn merge_with_divergent_doublings_lands_on_the_coarsest_group() {
        // `capped` doubled once (cap 4, six levels); `fine` did not.
        let capped_trades: Vec<Trade> = (0..6u64)
            .map(|i| trade(i, &format!("{}", 100 + i), "1", Side::Buy))
            .collect();
        let capped = ladder_capped(&capped_trades, "1", 4);
        assert!(capped.is_aggregated());

        let fine = ladder(
            &[
                trade(10, "101", "2", Side::Sell),
                trade(11, "103", "3", Side::Sell),
            ],
            "1",
        );

        let profile = VolumeProfile::merge([&capped, &fine], DEFAULT_LEVEL_CAP).unwrap();
        assert_eq!(profile.group(), dec("2"));
        assert!(profile.is_aggregated());
        // Totals are conserved under the fold.
        assert_eq!(profile.total_volume(), dec("11"));
        // 101 -> bucket 50, 103 -> bucket 51 under group 2.
        assert_eq!(profile.levels()[&50].sell, dec("2"));
        assert_eq!(profile.levels()[&51].sell, dec("3"));
    }

    #[test]
    fn merge_refuses_empty_and_incompatible_base_groups() {
        assert!(VolumeProfile::merge([], DEFAULT_LEVEL_CAP).is_none());

        let a = ladder(&[trade(0, "100", "1", Side::Buy)], "1");
        let b = ladder(&[trade(1, "100", "1", Side::Buy)], "0.5");
        assert!(VolumeProfile::merge([&a, &b], DEFAULT_LEVEL_CAP).is_none());
    }

    #[test]
    fn merge_past_the_cap_coarsens_and_says_so() {
        let a = ladder(
            &(0..4u64)
                .map(|i| trade(i, &format!("{}", 100 + i), "1", Side::Buy))
                .collect::<Vec<_>>(),
            "1",
        );
        let b = ladder(
            &(0..4u64)
                .map(|i| trade(10 + i, &format!("{}", 104 + i), "1", Side::Sell))
                .collect::<Vec<_>>(),
            "1",
        );
        // Eight distinct rows over a cap of 4 force one doubling.
        let profile = VolumeProfile::merge([&a, &b], 4).unwrap();
        assert!(profile.is_aggregated());
        assert_eq!(profile.group(), dec("2"));
        assert_eq!(profile.levels().len(), 4);
        assert_eq!(profile.total_volume(), dec("8"));
    }

    #[test]
    fn poc_ties_break_toward_the_lowest_bucket() {
        let profile = profile_of(&[("10", "3", "0"), ("11", "0", "2"), ("12", "0", "3")]);
        assert_eq!(profile.poc(), Some(10));
    }

    #[test]
    fn value_area_on_a_symmetric_ladder() {
        // 1 / 4 / 10 / 4 / 1: POC row is 10 of 20 total; 70% needs exactly
        // 14. The pairs tie (4+1 both sides), the tie expands downward, and
        // the first row below (4) completes the fraction — the area is
        // minimal, so expansion stops mid-pair right there.
        let profile = profile_of(&[
            ("10", "1", "0"),
            ("11", "4", "0"),
            ("12", "10", "0"),
            ("13", "4", "0"),
            ("14", "1", "0"),
        ]);
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 12,
                vah: 12,
                val: 11
            }
        );
    }

    #[test]
    fn value_area_expands_one_sided_when_the_poc_sits_at_the_top() {
        let profile = profile_of(&[
            ("10", "1", "0"),
            ("11", "2", "0"),
            ("12", "3", "0"),
            ("13", "10", "0"),
        ]);
        // 70% of 16 is 11.2: POC (10) then the pair below adds 3 -> 13.
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 13,
                vah: 13,
                val: 12
            }
        );
    }

    #[test]
    fn value_area_of_a_single_row_is_that_row() {
        let profile = profile_of(&[("10", "5", "5")]);
        assert_eq!(
            profile.value_area(dec("0.70")).unwrap(),
            ValueArea {
                poc: 10,
                vah: 10,
                val: 10
            }
        );
    }

    #[test]
    fn value_area_tie_between_pairs_expands_downward() {
        // Pairs around the POC are exactly equal (2+3 both sides); the tie
        // must go to the pair below, mirroring the POC's own tie rule.
        let profile = profile_of(&[
            ("10", "2", "0"),
            ("11", "3", "0"),
            ("12", "10", "0"),
            ("13", "3", "0"),
            ("14", "2", "0"),
        ]);
        // 70% of 20 is 14: POC (10) + one row below (3) reaches 13, the next
        // (2) reaches 15 — all below, nothing above.
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 12,
                vah: 12,
                val: 10
            }
        );
    }

    #[test]
    fn value_area_at_full_fraction_covers_every_printed_row() {
        let profile = profile_of(&[
            ("10", "1", "0"),
            ("12", "5", "0"),
            ("20", "1", "0"), // far gap: printed rows only, gaps cost nothing
        ]);
        let area = profile.value_area(dec("1")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 12,
                vah: 20,
                val: 10
            }
        );
    }

    #[test]
    fn value_area_none_only_on_an_empty_profile() {
        let profile = profile_of(&[("10", "1", "0")]);
        assert!(profile.value_area(dec("0.70")).is_some());
        // An empty profile cannot be built through merge (empty input is
        // `None`), so `value_area` has no empty case to answer — asserted
        // here through the merge contract.
        assert!(VolumeProfile::merge([], DEFAULT_LEVEL_CAP).is_none());
    }

    #[test]
    fn absurd_quantities_saturate_instead_of_panicking() {
        let mut builder = FootprintBuilder::new(dec("1"), DEFAULT_LEVEL_CAP);
        builder.push(&Trade {
            agg_id: 0,
            timestamp_ms: 0,
            price: dec("100"),
            quantity: Decimal::MAX,
            side: Side::Buy,
        });
        let a = builder.close().unwrap();
        let b = ladder(&[trade(1, "100", "1", Side::Buy)], "1");

        let profile = VolumeProfile::merge([&a, &b], DEFAULT_LEVEL_CAP).unwrap();
        assert_eq!(profile.levels()[&100].buy, Decimal::MAX);
        assert_eq!(profile.total_volume(), Decimal::MAX);
        // The value area over a saturated total still answers.
        assert!(profile.value_area(dec("0.70")).is_some());
    }
}
