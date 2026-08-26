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
//!   (the identity coarsening rests on: `floor(floor(p/g)/2) == floor(p/2g)`).
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

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::{BarFootprint, FootprintLevel};

/// How many rows one value-area expansion step weighs, and takes, per side.
///
/// The Sierra Chart / CQG convention expands the area by **two** rows at a
/// time, and three separate parts of the expansion have to agree on that
/// number: the pair the two sides are compared on, the price window a step is
/// not allowed to walk past, and the rows the step actually takes. Kept apart
/// they drift, and the area ends up weighed on one count and grown by another.
const VALUE_AREA_STEP_ROWS: usize = 2;

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
    /// This is the whole-range spelling of the same fold
    /// [`ProfileFold`](crate::ProfileFold) runs a piece at a time, and it is
    /// written on top of it rather than beside it: one accumulator, one set of
    /// coarsening rules, no second answer to keep in step. Callers that cannot
    /// afford the whole range in one call reach for the fold directly.
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
        let mut ladders = ladders.into_iter();
        let first = ladders.next()?;
        // The first ladder's own grouping becomes the fold's, so it can never
        // be the one refused; and a ladder's base group is positive by
        // `FootprintBuilder`'s construction, so the fold's assertion on that
        // cannot fire from here.
        let mut fold = crate::ProfileFold::new(first.base_group(), level_cap);
        fold.push_ladder(first);
        for ladder in ladders {
            // One ladder on another base grouping and there is no honest
            // profile over the set — answered the moment it is seen, rather
            // than after folding the rest of a range nobody will read.
            if !fold.push_ladder(ladder) {
                return None;
            }
        }
        fold.profile()
    }

    /// The profile these rows, this grouping and this honesty flag describe.
    ///
    /// Crate-only, and deliberately: a `VolumeProfile` is a *fold's* result,
    /// and there is exactly one fold — [`ProfileFold`](crate::ProfileFold),
    /// which [`merge`](Self::merge) itself is written on top of. Handing this
    /// to consumers would let a second fold exist, which is the drift the
    /// one-engine rule forbids.
    pub(crate) fn from_parts(
        levels: BTreeMap<i64, FootprintLevel>,
        group: Decimal,
        aggregated: bool,
    ) -> Self {
        Self {
            levels,
            group,
            aggregated,
        }
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
    /// POC, compare the combined volume of the next **two** price-adjacent
    /// buckets above against the next two below, and expand into the larger
    /// pair. Buckets where nothing traded contribute zero volume, so a pair
    /// facing a price gap competes with what actually sits behind it — an
    /// isolated cluster far above the POC no longer outbids a printed row
    /// directly below it just because the gap between them is free to cross.
    /// An exact tie expands **downward**, consistent with the POC's own
    /// tie-toward-lowest rule. Within the chosen pair, buckets are taken one
    /// at a time and expansion stops as soon as the fraction is captured, so
    /// the area is minimal.
    ///
    /// A side whose pair is entirely inside a gap loses to a side with any
    /// printed volume in its pair. When **both** pairs are gap the compared
    /// window is silent on both sides and cannot decide, so the choice falls
    /// to what actually sits behind each gap: the next **two printed rows**
    /// on each side — the same two the step will take, and the same two a
    /// contiguous ladder would have found inside the window. Ties there still
    /// expand downward. Expansion then jumps straight to those rows, so a
    /// `fraction ≥ 1` still covers every printed row and the loop stays
    /// bounded by printed rows, not by price distance.
    ///
    /// Because the silent case falls back on the very rows the window would
    /// have held, the area is a function of the rows that **printed**, not of
    /// the grouping they are read at: the same tape profiled at the tick and
    /// at a hundredth of it names the same POC, VAH and VAL. That both-gap
    /// case is not exotic — any grouping finer than the instrument's tick
    /// makes most of the ladder gap, and deciding those steps on the silent
    /// pairs alone would hand nearly every one of them to the tie-break and
    /// ratchet one edge of the area to the end of the profile.
    ///
    /// [`vah`](ValueArea::vah) and [`val`](ValueArea::val) are always printed
    /// buckets — empty buckets crossed on the way are never reported as edges.
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

        // Volume printed at exactly `bucket`, zero when nothing traded there.
        let vol_at = |bucket: i64| -> Decimal {
            rows.binary_search_by_key(&bucket, |&(b, _)| b)
                .map(|idx| rows[idx].1)
                .unwrap_or(Decimal::ZERO)
        };

        let target = self.total_volume().saturating_mul(fraction);
        let mut lo = poc_idx;
        let mut hi = poc_idx;
        let mut captured = rows[poc_idx].1;

        while captured < target && (lo > 0 || hi + 1 < rows.len()) {
            // The step's reach in buckets, on either side of an edge.
            let reach = VALUE_AREA_STEP_ROWS as i64;
            // Volume printed in the price window a step may take from, gaps
            // counting zero — the pair the convention compares.
            let window_volume = |edge: i64, direction: i64| -> Decimal {
                (1..=reach).fold(Decimal::ZERO, |sum, step| {
                    sum.saturating_add(vol_at(edge + direction * step))
                })
            };
            let above_exhausted = hi + 1 >= rows.len();
            let pair_above = if above_exhausted {
                Decimal::ZERO
            } else {
                window_volume(rows[hi].0, 1)
            };
            let pair_below = if lo == 0 {
                Decimal::ZERO
            } else {
                window_volume(rows[lo].0, -1)
            };
            // The next two *printed* rows on one side — what that side's step
            // would take. When the price-adjacent window holds them both this
            // is the number the pair already carries; when that window is
            // silent it is what the pair failed to see. Only the silent case
            // reads it, and this runs per frame, so it stays a closure.
            let printed_below = || {
                rows[..lo]
                    .iter()
                    .rev()
                    .take(VALUE_AREA_STEP_ROWS)
                    .fold(Decimal::ZERO, |sum, &(_, v)| sum.saturating_add(v))
            };
            let printed_above = || {
                rows[hi + 1..]
                    .iter()
                    .take(VALUE_AREA_STEP_ROWS)
                    .fold(Decimal::ZERO, |sum, &(_, v)| sum.saturating_add(v))
            };

            let take_below = if lo == 0 {
                false
            } else if above_exhausted {
                true
            } else if pair_below > Decimal::ZERO || pair_above > Decimal::ZERO {
                pair_below >= pair_above
            } else {
                // Both pairs are entirely inside a gap: the two-bucket window
                // the convention compares is silent on both sides, so it
                // cannot decide. Weigh what actually sits behind each gap
                // instead. Deciding on the silent pairs alone would hand every
                // such step to the tie-break, and a sparse ladder is *mostly*
                // such steps: the area would ratchet one way row after row and
                // pin that edge to the end of the profile.
                printed_below() >= printed_above()
            };
            // One step takes up to the two rows its side was weighed on, one
            // at a time, stopping the moment the fraction is captured — the
            // same two, whether the pair reached them across printed buckets
            // or the jump reached them across a gap. That is what keeps the
            // area a function of the rows that printed rather than of the
            // grouping they are read at.
            if take_below {
                // The window belongs to the step, not to the row last taken:
                // it is fixed before the first of the two, so a step can never
                // walk further than the pair it was weighed on. A pair that is
                // all gap has no window to stay inside — that is the jump.
                let window_floor = rows[lo].0 - reach;
                let bounded = pair_below > Decimal::ZERO;
                for _ in 0..VALUE_AREA_STEP_ROWS {
                    if lo == 0 || captured >= target {
                        break;
                    }
                    // Inside the window an unprinted bucket adds nothing and
                    // is not an edge; below it the step is over.
                    if bounded && rows[lo - 1].0 < window_floor {
                        break;
                    }
                    lo -= 1;
                    captured = captured.saturating_add(rows[lo].1);
                }
            } else {
                let window_ceiling = rows[hi].0 + reach;
                let bounded = pair_above > Decimal::ZERO;
                for _ in 0..VALUE_AREA_STEP_ROWS {
                    if hi + 1 >= rows.len() || captured >= target {
                        break;
                    }
                    if bounded && rows[hi + 1].0 > window_ceiling {
                        break;
                    }
                    hi += 1;
                    captured = captured.saturating_add(rows[hi].1);
                }
            }
        }

        Some(ValueArea {
            poc,
            vah: rows[hi].0,
            val: rows[lo].0,
        })
    }
}

/// `bucket / 2^shift`, floored — the exact fold that lands a finer ladder's
/// bucket in the coarser grouping. A shift past the integer width collapses
/// every bucket to row 0 (or -1 below zero), which is where `floor` sends it.
pub(crate) fn fold_bucket(bucket: i64, shift: u32) -> i64 {
    if shift >= i64::BITS - 1 {
        if bucket < 0 { -1 } else { 0 }
    } else {
        bucket.div_euclid(1i64 << shift)
    }
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
    fn value_area_does_not_annex_clusters_across_price_gaps() {
        // Issue #156's reproduction. 21 units total, 70% needs 14.7. The old
        // printed-row pairing saw the isolated cluster at 500 as "the next row
        // above 102" and dragged the VAH 400 buckets up — while leaving out
        // the row at 100 holding 4x the volume of the row it included.
        let profile = profile_of(&[
            ("100", "4", "0"),
            ("101", "10", "0"),
            ("102", "1", "0"),
            ("500", "6", "0"),
        ]);
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 101,
                vah: 102,
                val: 100
            }
        );
    }

    #[test]
    fn value_area_gap_side_loses_to_a_printed_neighbor() {
        // The pair above the POC is pure gap; the row directly below wins the
        // comparison even though the far cluster holds more volume than it.
        let profile = profile_of(&[("100", "4", "0"), ("101", "10", "0"), ("500", "6", "0")]);
        // 70% of 20 is 14: POC (10) + the row at 100 (4) completes it.
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 101,
                vah: 101,
                val: 100
            }
        );
    }

    #[test]
    fn value_area_pair_scans_past_a_hole_to_its_printed_second_bucket() {
        // Below the POC the pair is (99: gap, 98: printed) — the pair's
        // volume sits in its second bucket and must still be reachable.
        let profile = profile_of(&[("98", "6", "0"), ("100", "10", "0"), ("101", "2", "0")]);
        // 70% of 18 is 12.6: POC (10), pair below (6) beats pair above (2),
        // 99 is skipped as a gap and 98 completes the fraction.
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 100,
                vah: 100,
                val: 98
            }
        );
    }

    #[test]
    fn value_area_terminates_across_an_astronomical_gap() {
        // The loop must be bounded by printed rows, not by price distance:
        // covering everything requires crossing a billion-bucket gap, and the
        // jump must land on the printed row in one step.
        let profile = profile_of(&[("0", "10", "0"), ("1000000000", "3", "0")]);
        let area = profile.value_area(dec("1")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 0,
                vah: 1_000_000_000,
                val: 0
            }
        );
    }

    #[test]
    fn value_area_with_gaps_on_both_sides_weighs_what_sits_behind_them() {
        // A sparse ladder — a fine grouping over a wide range, the normal
        // shape of a live order-flow profile — puts *both* two-bucket pairs
        // inside a gap on nearly every step. Deciding on the pairs alone reads
        // 0 vs 0 there, so the tie-break picks a side without looking at the
        // tape: the area then ratchets that way row after row and pins its
        // edge to the end of the profile. The side that wins must be the one
        // whose next printed row actually holds more volume.
        //
        // 30 units total, 70% needs 21. Above the POC sit 9 and 9; below sit
        // 1 and 1. Value has to grow upward.
        let profile = profile_of(&[
            ("100", "1", "0"),
            ("110", "1", "0"),
            ("120", "10", "0"),
            ("130", "9", "0"),
            ("140", "9", "0"),
        ]);
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 120,
                vah: 140,
                val: 120
            }
        );
    }

    #[test]
    fn value_area_across_two_gaps_still_ties_downward() {
        // Both pairs are gap and what sits behind each gap is equal: the
        // POC's own tie-toward-lowest rule decides, as it does for printed
        // neighbours. 12 units total, 70% needs 8.4.
        let profile = profile_of(&[("100", "3", "0"), ("110", "6", "0"), ("120", "3", "0")]);
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 110,
                vah: 110,
                val: 100
            }
        );
    }

    #[test]
    fn value_area_is_the_same_prices_at_every_grouping_the_tape_prints_on() {
        // The instrument trades on a 5-wide tick, so a ladder grouped at 5 is
        // contiguous and one grouped at 1 is the same rows with four empty
        // buckets between each - same prints, same volumes, two resolutions.
        // The value area is a read of where volume traded, so it must name the
        // same prices either way; a profile drawn on a grouping finer than the
        // tick is otherwise a different answer to the same question.
        let trades: Vec<Trade> = [
            ("95", "1"),
            ("100", "2"),
            ("105", "8"),
            ("110", "9"),
            ("115", "6"),
            ("120", "2"),
            ("125", "1"),
        ]
        .iter()
        .enumerate()
        .map(|(i, &(price, qty))| trade(i as u64, price, qty, Side::Buy))
        .collect();

        let coarse = VolumeProfile::merge([&ladder(&trades, "5")], DEFAULT_LEVEL_CAP).unwrap();
        let fine = VolumeProfile::merge([&ladder(&trades, "1")], DEFAULT_LEVEL_CAP).unwrap();

        let coarse_area = coarse.value_area(dec("0.70")).unwrap();
        let fine_area = fine.value_area(dec("0.70")).unwrap();

        assert_eq!(
            (
                coarse.bucket_price(coarse_area.poc),
                coarse.bucket_price(coarse_area.val),
                coarse.bucket_price(coarse_area.vah),
            ),
            (
                fine.bucket_price(fine_area.poc),
                fine.bucket_price(fine_area.val),
                fine.bucket_price(fine_area.vah),
            ),
        );
        // And the area is a two-sided band around the POC, reaching neither
        // end of the profile: 29 units total, 70% needs 20.3.
        assert_eq!(coarse.bucket_price(coarse_area.poc), dec("110"));
        assert_eq!(coarse.bucket_price(coarse_area.val), dec("100"));
        assert_eq!(coarse.bucket_price(coarse_area.vah), dec("115"));
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
