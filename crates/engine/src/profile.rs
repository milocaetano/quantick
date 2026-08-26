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
//! band of rows the expansion grows around the [`poc`](VolumeProfile::poc)
//! until it holds a caller-supplied fraction of the range's volume (the engine
//! attaches no thresholds of its own — 70% is the caller's convention, not
//! this module's). That method's doc comment owns the rule.
//!

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::{BarFootprint, FootprintLevel};

/// The most rows one value-area expansion step weighs, and takes, per side.
///
/// The Sierra Chart / CQG convention expands the area by **two** rows at a
/// time. Two parts of the expansion read this number and must agree on it: how
/// far a step's window reaches, and how many rows a side may offer inside it.
/// A step then weighs and takes exactly the rows that window admitted, so all
/// three stay in step by construction rather than by three literals matching.
/// A side may offer fewer than this many — that is a price gap costing
/// something — but never more.
///
/// It counts *rows*, never buckets: the window is this many rows' worth of the
/// step's own gap, which is what lets one ladder read at two groupings answer
/// the same. See [`VolumeProfile::value_area`], which owns the rule.
const VALUE_AREA_STEP_ROWS: usize = 2;

/// Which way a value-area step is walking. The two directions are mirror
/// images, and the rule they share — how far a step may reach and what it
/// weighs when it gets there — is only one rule if it is only written once.
#[derive(Clone, Copy)]
enum Side {
    Below,
    Above,
}

use Side::{Above, Below};

/// The rows one side offers a step: the next up to [`VALUE_AREA_STEP_ROWS`]
/// printed rows from `edge`, stopping at the first one further away than
/// `window`, with their volumes in outward order.
///
/// Reach is decided by distance alone — a row that traded nothing is still a
/// row, and reading it as absent would let a step walk past the window it was
/// weighed on. Distances are unsigned throughout: buckets saturate on ingest,
/// so a corrupt feed price can put rows a whole bucket space apart, and a gap
/// that does not fit in an `i64` must not read as no gap at all.
fn reachable(
    rows: &[(i64, Decimal)],
    edge: usize,
    window: u64,
    side: Side,
) -> ([Decimal; VALUE_AREA_STEP_ROWS], usize) {
    let mut volumes = [Decimal::ZERO; VALUE_AREA_STEP_ROWS];
    let mut reach = 0;
    for offset in 1..=VALUE_AREA_STEP_ROWS {
        let index = match side {
            Below => edge.checked_sub(offset),
            Above => edge.checked_add(offset),
        };
        let Some(&(bucket, volume)) = index.and_then(|index| rows.get(index)) else {
            break;
        };
        if rows[edge].0.abs_diff(bucket) > window {
            break;
        }
        volumes[offset - 1] = volume;
        reach = offset;
    }
    (volumes, reach)
}

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

    /// The band of rows the Sierra Chart / CQG expansion grows around the POC
    /// until it holds at least `fraction` of
    /// [`total_volume`](Self::total_volume).
    ///
    /// The expansion is greedy and stops the moment the fraction is captured,
    /// so the band is minimal *along the path it took* — never grown past the
    /// point that answers the question. It is not the narrowest band that
    /// could hold the fraction: a different pair of edges may hold as much in
    /// less price, and the convention does not look for it.
    ///
    /// Expansion follows the Sierra Chart / CQG convention: starting at the
    /// POC, compare the volume of the next **two** rows above against the next
    /// two below and expand into the heavier side, taking its rows one at a
    /// time and stopping the moment the fraction is captured, so the area
    /// never grows past the point that answers the question. An exact tie
    /// expands **downward**, consistent with the POC's own tie-toward-lowest
    /// rule.
    ///
    /// A row only counts for its side if it lies within the step's **window**,
    /// and that is where a price gap costs something: a side whose next rows
    /// all sit beyond the window weighs nothing and loses to a side with any
    /// row inside it, however little that row holds. So an isolated cluster
    /// cannot annex the area just by being bigger than the row next to the
    /// POC — it has to be close enough to compete at all.
    ///
    /// **Close enough is measured in rows, not in price.** The window is two
    /// rows' worth of the step's own gap: how far the nearest unclaimed
    /// neighbour actually is, on whichever side that is. On a ladder printing
    /// every tick the window is two ticks, the convention's own reading. On
    /// the same ladder read a hundred times finer — the same rows at the same
    /// prices, with empty buckets between them — the gap is a hundred times
    /// wider in buckets and so is the window, and both readings expand the
    /// same way. A window counted in buckets instead would find nothing on
    /// either side at nearly every step of the finer ladder, hand every step
    /// to the tie-break, and ratchet the area one way until an edge reached
    /// the end of the profile.
    ///
    /// Two things follow, and the second is the limit of the first:
    ///
    /// - The area is a function of the rows that **printed**, not of the
    ///   grouping they are read at, whenever that grouping divides the price
    ///   grid the tape prints on — which is the case that matters, since a
    ///   ladder is grouped at the instrument's tick or at a fraction of it. A
    ///   grouping that cuts across the grid rather than refining it lands the
    ///   prints on unevenly spaced buckets and is a different ladder, so it may
    ///   well name a different area. [`merge`](Self::merge) can *produce* such
    ///   a grouping on its own: past its row cap it doubles, and a doubled
    ///   fraction of a tick need not divide that tick. A capped profile says so
    ///   through [`is_aggregated`](Self::is_aggregated), and that flag is the
    ///   only warning there is.
    /// - The gap that sets the window is the *step's*, not the profile's, so a
    ///   print far outside the area cannot change **which side** a step
    ///   expands into. That matters because the profile is re-read while a bar
    ///   forms: a band that swung because of a trade nowhere near it would be
    ///   noise on the chart. It does not make the band immune to distant
    ///   prints — `fraction` is a share of the whole range's volume, so a print
    ///   anywhere raises the target every side is expanding toward, and the
    ///   band grows. That is the caller's own definition of value doing its
    ///   work, not the window moving.
    ///
    /// Because the window is set by whichever neighbour is nearest, that side
    /// always has a row inside it — expansion is never stuck, and there is no
    /// case where neither side can be weighed.
    ///
    /// [`vah`](ValueArea::vah) and [`val`](ValueArea::val) are always printed
    /// buckets — empty buckets crossed on the way are never reported as edges.
    /// A bucket that printed a zero-quantity trade *is* a row, and is crossed
    /// like any other rather than read as a gap.
    ///
    /// `None` on an empty profile. `fraction` is the caller's convention
    /// (0.70 is the classic). A fraction of 1 asks for the whole range's
    /// volume, which every row that *traded* is needed for — but expansion
    /// stops the moment it has that volume, so rows holding nothing may sit
    /// outside the band even then. A zero-quantity print is a row like any
    /// other and is subject to this.
    #[must_use]
    pub fn value_area(&self, fraction: Decimal) -> Option<ValueArea> {
        let poc = self.poc()?;
        let rows: Vec<(i64, Decimal)> = self
            .levels
            .iter()
            .map(|(&bucket, level)| (bucket, level.volume()))
            .collect();
        // `rows` is sorted by bucket, and the POC came from these same rows,
        // so the lookup is a search rather than a scan and cannot fail.
        let poc_idx = rows
            .binary_search_by_key(&poc, |&(bucket, _)| bucket)
            .ok()?;

        // What the first `k` of a side's rows hold.
        let prefix = |volumes: &[Decimal; VALUE_AREA_STEP_ROWS], k: usize| {
            volumes[..k]
                .iter()
                .fold(Decimal::ZERO, |sum, &volume| sum.saturating_add(volume))
        };

        // Summed off `rows` rather than through `total_volume`, which would
        // walk the whole map a second time — this runs while a bar forms.
        let total = rows.iter().fold(Decimal::ZERO, |sum, &(_, volume)| {
            sum.saturating_add(volume)
        });
        let target = total.saturating_mul(fraction);
        let mut lo = poc_idx;
        let mut hi = poc_idx;
        let mut captured = rows[poc_idx].1;

        while captured < target && (lo > 0 || hi + 1 < rows.len()) {
            let has_below = lo > 0;
            let has_above = hi + 1 < rows.len();
            // Distances stay unsigned the whole way: buckets saturate on
            // ingest, so a corrupt feed price can put rows a full bucket space
            // apart, and a gap that does not fit in an `i64` must not read as
            // no gap at all.
            let below_gap = has_below.then(|| rows[lo].0.abs_diff(rows[lo - 1].0));
            let above_gap = has_above.then(|| rows[hi + 1].0.abs_diff(rows[hi].0));
            // The step's own idea of one row: how far its nearest unclaimed
            // neighbour actually is, on whichever side that is. Reading it per
            // step rather than once over the ladder is what keeps a print far
            // outside the area from moving the window — the profile is re-read
            // while a bar forms, and a band that shifted because of a trade
            // nowhere near it would be noise on the chart.
            let step_gap = match (below_gap, above_gap) {
                (Some(below), Some(above)) => below.min(above),
                (Some(only), None) | (None, Some(only)) => only,
                // The loop condition guarantees a neighbour on one side, so
                // this is unreachable; it fails loudly in a test build rather
                // than quietly returning a band that is short of its target.
                (None, None) => {
                    debug_assert!(false, "value area step with no neighbour on either side");
                    break;
                }
            };
            let window = step_gap.saturating_mul(VALUE_AREA_STEP_ROWS as u64);

            // The next rows each side offers, out to the window. Reach is
            // decided by distance alone — a row that traded nothing is still a
            // row, and reading it as absent would let a step walk past the
            // window it was weighed on.
            let (below_rows, below_reach) = reachable(&rows, lo, window, Below);
            let (above_rows, above_reach) = reachable(&rows, hi, window, Above);

            let (take_below, steps) = if !has_below {
                (false, above_reach)
            } else if !has_above {
                (true, below_reach)
            } else if below_reach == 0 || above_reach == 0 {
                // A side with nothing inside the window is across a gap, and
                // loses to a side with any row inside it however little that
                // row holds. This is what a price gap costs, and it is settled
                // before volume so a row that traded nothing cannot lose its
                // side the tie.
                let take_below = below_reach > 0;
                (
                    take_below,
                    if take_below { below_reach } else { above_reach },
                )
            } else {
                // Both sides have rows inside the window, so each is weighed
                // on everything it actually offers there.
                //
                // Weighing them instead on `min(reach)` rows — to make the
                // comparison "like for like" when one side reaches further —
                // is worse, and measurably: it throws away a row the window
                // already admitted. On 8(10), 9(1), 10(20), 12(5) at 70% it
                // drops the 10-unit row two buckets below and returns a band
                // that is wider in price and lighter in volume than the one it
                // rejected. The reach limit is the whole gap charge; a side
                // that reaches fewer rows is offering less, and that is the
                // answer, not an artefact to correct for.
                let take_below =
                    prefix(&below_rows, below_reach) >= prefix(&above_rows, above_reach);
                (
                    take_below,
                    if take_below { below_reach } else { above_reach },
                )
            };

            // A step takes the rows it was weighed on, one at a time, stopping
            // the moment the fraction is captured, so the area never grows past
            // the point that answers the question.
            //
            // `max(1)` is what makes every pass advance an edge, and so what
            // guarantees the loop ends. The side that set the window always has
            // a row inside it, so `steps` is normally at least one on its own
            // account; the guard is there so a future change to the window or
            // to the reach rule cannot turn a stalled pass into a spin, inside
            // a function the chart re-runs while a bar forms.
            if take_below {
                for _ in 0..steps.max(1) {
                    if lo == 0 || captured >= target {
                        break;
                    }
                    lo -= 1;
                    captured = captured.saturating_add(rows[lo].1);
                }
            } else {
                for _ in 0..steps.max(1) {
                    if hi + 1 >= rows.len() || captured >= target {
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
    ///
    /// A `"0"` here means *no trade on that side*, and the row is skipped —
    /// which is why [`profile_at`] exists rather than delegating to this.
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

    /// Buy-only `(price, quantity)` prints read at `group` — the shape the
    /// grouping fixtures want, where the side split is beside the point.
    ///
    /// Deliberately not built on [`profile_at_group`]: there `"0"` means *no
    /// trade on this side*, and the row is skipped. Here a zero quantity is a
    /// print that really happened for nothing, which the tape does send and
    /// which does create a row — the distinction one of these tests is about.
    fn profile_at(prints: &[(&str, &str)], group: &str) -> VolumeProfile {
        let trades: Vec<Trade> = prints
            .iter()
            .enumerate()
            .map(|(i, &(price, qty))| trade(i as u64, price, qty, Side::Buy))
            .collect();
        VolumeProfile::merge([&ladder(&trades, group)], DEFAULT_LEVEL_CAP).unwrap()
    }

    /// The area's prices, which is what a chart draws and the only form two
    /// groupings can be compared in.
    fn area_prices(profile: &VolumeProfile, fraction: &str) -> (Decimal, Decimal, Decimal) {
        let area = profile.value_area(dec(fraction)).unwrap();
        (
            profile.bucket_price(area.poc),
            profile.bucket_price(area.val),
            profile.bucket_price(area.vah),
        )
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
    fn value_area_on_a_sparse_ladder_grows_toward_the_volume() {
        // A ladder read on a grouping finer than the tick — the normal shape
        // of a live order-flow profile, and the app's own default. Every row
        // is ten buckets from the next, so a window counted in buckets finds
        // nothing on either side at every step and hands the whole expansion
        // to the tie-break; the area then ratchets one way until an edge hits
        // the end of the profile. Scaled to the ladder's own step, the window
        // sees the rows that are there and the heavier side wins.
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
    fn value_area_ties_downward_across_equally_wide_gaps() {
        // Both neighbours are the same distance out, so the window is that
        // distance and both sides are inside it holding the same volume. The
        // POC's own tie-toward-lowest rule decides, exactly as it does on a
        // contiguous ladder — the gaps being wide changes nothing.
        //
        // 20 units total, 70% needs 14: the POC holds 10 and the row across
        // the lower gap completes it, so the tie is the whole answer.
        let profile = profile_at(
            &[
                ("0", "1"),
                ("10", "4"),
                ("1000", "10"),
                ("1990", "4"),
                ("2000", "1"),
            ],
            "1",
        );
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 1000,
                vah: 1000,
                val: 10
            }
        );
    }

    #[test]
    fn value_area_is_the_same_prices_at_every_grouping_the_tape_prints_on() {
        // The instrument trades on a 5-wide tick, so a ladder grouped at 5 is
        // contiguous and one grouped finer is the same rows with empty buckets
        // between them — same prints, same volumes, two resolutions. The value
        // area is a read of where volume traded, so it names the same prices
        // either way.
        let prints = [
            ("95", "1"),
            ("100", "2"),
            ("105", "8"),
            ("110", "9"),
            ("115", "6"),
            ("120", "2"),
            ("125", "1"),
        ];
        let at_tick = area_prices(&profile_at(&prints, "5"), "0.70");
        // A two-sided band around the POC, reaching neither end of the
        // profile: 29 units total, 70% needs 20.3.
        assert_eq!(at_tick, (dec("110"), dec("100"), dec("115")));
        for finer in ["1", "0.5", "0.01"] {
            assert_eq!(
                area_prices(&profile_at(&prints, finer), "0.70"),
                at_tick,
                "grouping {finer} named a different area than the tick did",
            );
        }
    }

    #[test]
    fn value_area_names_the_same_prices_on_a_mixed_ladder_at_every_grouping() {
        // Dense below the POC, gapped above — the shape that breaks a window
        // measured in buckets rather than in the tape's own row spacing. Read
        // at the tick the rows below are price-adjacent; read a fifth of the
        // tick they are not, and a bucket-wide window goes silent on a side
        // that has volume sitting right there.
        let prints = [
            ("90", "1"),
            ("95", "1"),
            ("100", "10"),
            ("120", "9"),
            ("125", "9"),
        ];
        let at_tick = area_prices(&profile_at(&prints, "5"), "0.70");
        assert_eq!(at_tick, (dec("100"), dec("90"), dec("120")));
        for finer in ["1", "0.5", "0.01"] {
            assert_eq!(
                area_prices(&profile_at(&prints, finer), "0.70"),
                at_tick,
                "grouping {finer} named a different area than the tick did",
            );
        }
    }

    #[test]
    fn value_area_weighs_a_side_on_every_row_the_window_admits() {
        // The window is the gap charge, and a side that reaches fewer rows is
        // offering less — that is the answer, not an artefact to correct for.
        //
        // Here the step's gap is one bucket, so the window is two: the rows at
        // 9 and 8 are both inside it, and above only 12 is. Weighing the sides
        // on the same *count* of rows — one each, to make the comparison look
        // fair — drops the 10-unit row the window already admitted, and hands
        // back a band that is wider in price and lighter in volume than the
        // one it turned down.
        //
        // 36 units total, 70% needs 25.2. 8..10 holds 31 in three buckets;
        // 9..12 holds 26 in four.
        let profile = profile_at(&[("8", "10"), ("9", "1"), ("10", "20"), ("12", "5")], "1");
        assert_eq!(
            profile.value_area(dec("0.70")).unwrap(),
            ValueArea {
                poc: 10,
                vah: 10,
                val: 8
            }
        );
    }

    #[test]
    fn value_area_never_lets_a_remote_cluster_outbid_a_nearer_row() {
        // Issue #156's defect in the shape that survives a price-adjacent
        // window. The row below the POC is one step away; the cluster above it
        // is a hundred times further and holds more. Only the window keeps the
        // near row from losing, so this is the test that fails if the window
        // ever stops charging for a gap.
        //
        // 28 units total, 70% needs 19.6 — more than the two rows around the
        // POC hold, so the area does end up crossing both gaps. What it must
        // not do is cross the far one *first* and leave the near row outside.
        let profile = profile_at(
            &[("0", "8"), ("10", "10"), ("1000", "1"), ("100000", "9")],
            "1",
        );
        assert_eq!(
            profile.value_area(dec("0.70")).unwrap(),
            ValueArea {
                poc: 10,
                vah: 100_000,
                val: 0
            }
        );
        // And with a fraction the near rows alone can answer, nothing beyond
        // the window is annexed at all.
        assert_eq!(
            profile.value_area(dec("0.60")).unwrap(),
            ValueArea {
                poc: 10,
                vah: 10,
                val: 0
            }
        );
    }

    #[test]
    fn value_area_crosses_the_nearer_gap_first() {
        // The nearer neighbour sets the window, so the far side is outside it
        // and weighs nothing however much it holds: 990 buckets down against
        // 200 up. The old rule saw a gap on both sides, read 0 against 0, and
        // took the tie-break down every time — which is how the area ended up
        // pinned to the bottom of a gapped profile.
        //
        // 23 units total, 70% needs 16.1: the POC holds 10, the nearer gap
        // adds 6, and the far one completes it.
        let profile = profile_at(
            &[("0", "6"), ("10", "1"), ("1000", "10"), ("1200", "6")],
            "1",
        );
        let area = profile.value_area(dec("0.70")).unwrap();
        assert_eq!(
            area,
            ValueArea {
                poc: 1000,
                vah: 1200,
                val: 10
            }
        );
    }

    #[test]
    fn value_area_treats_a_printed_row_that_traded_nothing_as_a_row() {
        // A zero-quantity print still creates a level. Testing the window for
        // *volume* rather than for a printed row would read that level as a
        // gap and let the step jump clean past the window it was weighed on.
        let profile = profile_at(&[("50", "4"), ("100", "0"), ("101", "5")], "1");
        assert!(
            profile.levels().contains_key(&100),
            "the fixture needs the empty row"
        );
        let area = profile.value_area(dec("0.70")).unwrap();
        // 9 units, 70% needs 6.3: the POC alone holds 5, the empty row adds
        // nothing, and the row at 50 completes it. The empty row is crossed,
        // never treated as absent.
        assert_eq!(area.val, 50);
        assert_eq!(area.vah, 101);
    }

    #[test]
    fn value_area_reads_a_profile_at_the_edges_of_the_bucket_space() {
        // Buckets saturate on ingest so a corrupt feed price cannot panic the
        // fold; the window arithmetic over them has to hold up too. Adjacent
        // rows at the extremes only prove the arithmetic does not overflow.
        for (low, high) in [(i64::MAX - 1, i64::MAX), (i64::MIN, i64::MIN + 1)] {
            let trades = vec![
                trade(0, &low.to_string(), "5", Side::Buy),
                trade(1, &high.to_string(), "5", Side::Buy),
            ];
            let profile = VolumeProfile::merge([&ladder(&trades, "1")], DEFAULT_LEVEL_CAP).unwrap();
            let area = profile.value_area(dec("0.70")).unwrap();
            assert_eq!((area.val, area.vah), (low, high));
        }

        // The case that decides something: rows a whole bucket space apart, so
        // the gap does not fit in an `i64` at all. Measuring it as a signed
        // distance reads the wider gap as the nearer one and expands into the
        // lighter side — the heavier side must still win.
        //
        // 14 units total, 75% needs 10.5: the POC holds 10 and only the row
        // below can complete it without over-reaching.
        let trades = vec![
            trade(0, &i64::MIN.to_string(), "3", Side::Buy),
            trade(1, "0", "10", Side::Buy),
            trade(2, &i64::MAX.to_string(), "1", Side::Buy),
        ];
        let profile = VolumeProfile::merge([&ladder(&trades, "1")], DEFAULT_LEVEL_CAP).unwrap();
        assert_eq!(
            profile.value_area(dec("0.75")).unwrap(),
            ValueArea {
                poc: 0,
                vah: 0,
                val: i64::MIN
            }
        );
    }

    /// A deterministic pseudo-random source: fixed seed, no clock, no `rand`,
    /// so a sweep that fails fails the same way on every machine and the tape
    /// it failed on can be pasted straight into a fixture.
    struct Tapes(u64);

    impl Tapes {
        fn new() -> Self {
            Self(0x2545_F491_4F6C_DD1D)
        }

        fn next(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % n
        }

        /// A tape on a uniform price grid of `tick` — what an instrument with
        /// a tick size prints, with gaps of one to six ticks between rows.
        fn on_a_grid(&mut self, tick: i64) -> Vec<(String, String)> {
            let mut price = 1000 * tick;
            (0..3 + self.next(10))
                .map(|_| {
                    price += tick * (1 + self.next(6) as i64);
                    (price.to_string(), (1 + self.next(20)).to_string())
                })
                .collect()
        }

        /// A tape whose rows are spaced *irregularly* — clusters a bucket
        /// apart beside gaps thousands wide, and volumes that vary by orders
        /// of magnitude. Nothing a tick size explains, and the shape every
        /// regression in this module so far has hidden in.
        fn irregular(&mut self) -> Vec<(String, String)> {
            let mut price = 100_000i64;
            (0..3 + self.next(12))
                .map(|_| {
                    price += match self.next(4) {
                        0 => 1 + self.next(3) as i64,
                        1 => 4 + self.next(30) as i64,
                        2 => 50 + self.next(500) as i64,
                        _ => 1000 + self.next(20_000) as i64,
                    };
                    let quantity = match self.next(3) {
                        0 => self.next(3),
                        1 => 1 + self.next(50),
                        _ => 1 + self.next(5_000),
                    };
                    (price.to_string(), quantity.to_string())
                })
                .collect()
        }
    }

    fn borrowed(prints: &[(String, String)]) -> Vec<(&str, &str)> {
        prints
            .iter()
            .map(|(price, quantity)| (price.as_str(), quantity.as_str()))
            .collect()
    }

    #[test]
    fn value_area_is_grouping_invariant_across_a_sweep_of_generated_tapes() {
        // The property the window's step scaling buys, asserted over a spread
        // of shapes rather than one hand-picked ladder.
        let mut tapes = Tapes::new();
        for _ in 0..2000 {
            let tick = 1 + tapes.next(9) as i64;
            let prints = tapes.on_a_grid(tick);
            let prints = borrowed(&prints);

            let at_tick = profile_at(&prints, &tick.to_string());
            let finer = profile_at(&prints, "0.01");
            // These tapes are a dozen rows at most and the cap counts rows, so
            // neither ladder can coarsen; both really are the same tape read
            // twice, which is what makes the comparison mean anything.
            assert!(!at_tick.is_aggregated() && !finer.is_aggregated());
            assert_eq!(
                area_prices(&at_tick, "0.70"),
                area_prices(&finer, "0.70"),
                "tape {prints:?} named a different area at tick {tick} and at 0.01",
            );
        }
    }

    #[test]
    fn value_area_holds_its_contract_on_irregular_ladders() {
        // Every "always" this method's doc comment states, checked against
        // shapes chosen to break it rather than to illustrate it. Written this
        // way on purpose: the regressions this module has had were each
        // covered by a fixture that agreed with the rule it was meant to test,
        // because the fixture and the rule came from one idea of what a ladder
        // looks like. A generator that does not share that idea is the check.
        let mut tapes = Tapes::new();
        for _ in 0..3000 {
            let prints = tapes.irregular();
            let prints = borrowed(&prints);
            let profile = profile_at(&prints, "1");
            let levels = profile.levels();
            let lowest = *levels.keys().next().unwrap();
            let highest = *levels.keys().next_back().unwrap();

            for fraction in ["0.30", "0.70", "0.95", "1"] {
                let area = profile.value_area(dec(fraction)).unwrap();

                // The edges are printed rows, and the POC is inside its band.
                assert!(
                    levels.contains_key(&area.val),
                    "VAL {} is not a printed row on {prints:?}",
                    area.val,
                );
                assert!(
                    levels.contains_key(&area.vah),
                    "VAH {} is not a printed row on {prints:?}",
                    area.vah,
                );
                assert!(
                    area.val <= area.poc && area.poc <= area.vah,
                    "POC outside its own band on {prints:?}",
                );

                // The band holds the fraction, or there was no more tape to
                // take — the only two ways expansion is allowed to stop.
                let held: Decimal = levels
                    .iter()
                    .filter(|&(&bucket, _)| bucket >= area.val && bucket <= area.vah)
                    .map(|(_, level)| level.volume())
                    .sum();
                let target = profile.total_volume() * dec(fraction);
                assert!(
                    held >= target || (area.val == lowest && area.vah == highest),
                    "band {}..{} holds {held} of {target} on {prints:?}",
                    area.val,
                    area.vah,
                );
            }

            // Asking for more value can only add rows. The order rows are
            // taken in does not depend on the fraction — that only decides
            // where expansion stops — so a band that moved sideways means a
            // step decided differently for a reason other than the tape.
            let mut narrower = profile.value_area(dec("0.10")).unwrap();
            for fraction in ["0.25", "0.50", "0.75", "0.90", "1"] {
                let wider = profile.value_area(dec(fraction)).unwrap();
                assert!(
                    wider.val <= narrower.val && wider.vah >= narrower.vah,
                    "{fraction} of {prints:?} gave {wider:?}, not a superset of {narrower:?}",
                );
                narrower = wider;
            }
        }
    }

    #[test]
    fn value_area_ignores_a_print_that_traded_nothing_far_outside_it() {
        // The locality the doc claims for the window, as a property rather
        // than as a sentence. A zero-quantity print adds a row without adding
        // volume, so it moves neither the total nor the target. Landing it far
        // above everything the band reaches must therefore change nothing; a
        // band that shifts anyway is reading a gap that does not belong to the
        // step it is taking, which is what a whole-ladder window did.
        let mut tapes = Tapes::new();
        for _ in 0..3000 {
            let prints = tapes.irregular();
            let profile = profile_at(&borrowed(&prints), "1");
            let before = profile.value_area(dec("0.70")).unwrap();

            let mut intruded = prints.clone();
            intruded.push((
                before.vah.saturating_add(1_000_000).to_string(),
                "0".to_owned(),
            ));
            let after = profile_at(&borrowed(&intruded), "1")
                .value_area(dec("0.70"))
                .unwrap();

            assert_eq!(
                before, after,
                "a zero-volume print far above moved the band on {prints:?}",
            );
        }
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
