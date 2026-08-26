//! Folding a *range* of bars into one [`VolumeProfile`], a piece at a time.
//!
//! [`VolumeProfile::merge`] is the whole fold in one call: hand it every
//! ladder, get the profile. That is the right shape when the range is a
//! screenful of tape. It is the wrong shape when the range is a time chart's
//! backfilled history, for two reasons this module exists to fix:
//!
//! - **Venue candles have no tape.** They join as
//!   [approximated](crate::BarFootprint::approximated) ladders — the candle's
//!   volume spread evenly over its high–low. Written out, one candle is up to
//!   `level_cap` map entries; twenty-five thousand of them is tens of millions
//!   of entries built only to be summed and thrown away. Read as an
//!   `ApproxSpread` instead, one candle is a *range add*: six numbers, three
//!   map touches, whatever the candle's width. The profile is identical
//!   because the spread is the same data — `BarFootprint::approximated` is
//!   written on top of the same `ApproxSpread::of`.
//! - **The caller may not have a frame to spare.** [`ProfileFold`] takes its
//!   inputs one at a time and can be read at any point, so a consumer with a
//!   deadline folds what it can afford, paints what it has, and comes back —
//!   instead of blocking until the whole range is done.
//!
//! Memory is bounded by construction: accumulated rows never exceed the level
//! cap, and pending spreads never exceed [`ProfileFold::SPREAD_BATCH`] before
//! they are folded in. A fold of a million candles holds no more than a fold
//! of ten.
//!
//! Determinism is the same contract as the merge: exact integer bucket
//! arithmetic, `BTreeMap` order, no clock, no randomness. `profile()` over a
//! set of inputs returns exactly what `merge` returns over the same set —
//! guarded by tests in this module and by
//! `crates/engine/tests/profile_fold_parity.rs`.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::footprint::{ApproxSpread, halved};
use crate::profile::fold_bucket;
use crate::{Bar, BarFootprint, FootprintLevel, VolumeProfile};

/// A signed running delta over one bucket. The fold builds a *difference map*
/// (a delta at each boundary, values recovered by running sum), which is what
/// turns a candle's uniform spread from `rows` map entries into two.
#[derive(Debug, Clone, Default)]
struct Delta {
    buy: Decimal,
    sell: Decimal,
    /// Signed because a range's closing delta subtracts; trade counts
    /// themselves are never negative.
    count: i128,
    /// How many spreads cover this bucket. A row is *printed* where coverage
    /// is positive — including rows whose quantities round to zero, which the
    /// materialised ladder would also have printed.
    cover: i64,
}

/// A resumable, memory-bounded fold of ladders and venue candles into one
/// [`VolumeProfile`]. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct ProfileFold {
    base_group: Decimal,
    level_cap: usize,
    /// Doublings the accumulated `levels` are keyed under.
    doublings: u32,
    levels: BTreeMap<i64, FootprintLevel>,
    /// Spreads not yet folded into `levels`; never more than [`Self::SPREAD_BATCH`].
    pending: Vec<ApproxSpread>,
    /// Number of inputs that contributed — an empty fold has no profile, the
    /// same answer `merge` gives an empty iterator.
    inputs: usize,
}

impl ProfileFold {
    /// How many pending spreads a fold holds before folding them into its
    /// rows.
    ///
    /// The batch is what keeps a long range's memory flat: spreads accumulate,
    /// then collapse into an accumulator that is itself capped at `level_cap`
    /// rows. Larger batches amortise the collapse over more candles; smaller
    /// ones hold less. A thousand spreads is a few tens of kilobytes at peak
    /// and one collapse per thousand candles — either way far under what a
    /// single capped ladder occupies. Public because it is half of the memory
    /// bound a caller can assert against ([`rows_held`](Self::rows_held)).
    pub const SPREAD_BATCH: usize = 1_024;

    /// An empty fold over ladders grouped at `base_group`, capped at
    /// `level_cap` rows.
    ///
    /// # Panics
    ///
    /// Panics if `level_cap` is zero or `base_group` is not positive — a
    /// configuration error, the same contract as
    /// [`FootprintBuilder::new`](crate::FootprintBuilder::new).
    #[must_use]
    pub fn new(base_group: Decimal, level_cap: usize) -> Self {
        assert!(
            base_group > Decimal::ZERO,
            "profile base group must be positive"
        );
        assert!(level_cap > 0, "profile level cap must be positive");
        Self {
            base_group,
            level_cap,
            doublings: 0,
            levels: BTreeMap::new(),
            pending: Vec::new(),
            inputs: 0,
        }
    }

    /// Add one bar's real ladder. Costs one map touch per row it holds.
    ///
    /// Returns `false`, having added **nothing**, when `ladder` was built on a
    /// different base grouping: its buckets never aligned with this fold's,
    /// and folding them would invent rows rather than sum them.
    ///
    /// The refusal is reported, not remembered. What a mismatch means is the
    /// caller's to decide — [`VolumeProfile::merge`] has no honest answer over
    /// a set that disagrees and returns `None`, while a chart holding one
    /// stale snapshot beside a refolded group would rather fold the bars that
    /// do align and say how many it left out. A fold that poisoned itself
    /// would take the second caller's whole range down with one bar.
    pub fn push_ladder(&mut self, ladder: &BarFootprint) -> bool {
        if ladder.base_group() != self.base_group {
            return false;
        }
        self.raise_to(ladder.doublings());
        let shift = self.doublings - ladder.doublings();
        for (&bucket, level) in ladder.levels() {
            let target = self.levels.entry(fold_bucket(bucket, shift)).or_default();
            target.buy = target.buy.saturating_add(level.buy);
            target.sell = target.sell.saturating_add(level.sell);
            target.trade_count = target.trade_count.saturating_add(level.trade_count);
        }
        self.inputs += 1;
        self.cap_rows();
        true
    }

    /// Add one venue candle as an approximated spread — `O(1)` in the candle's
    /// width, where a materialised ladder is `O(rows)`.
    ///
    /// Returns `false` when the candle traded nothing: there is no
    /// distribution to approximate, exactly as
    /// [`BarFootprint::approximated`] returns `None`.
    pub fn push_candle(&mut self, bar: &Bar) -> bool {
        let Some(spread) = ApproxSpread::of(bar, self.base_group, self.level_cap) else {
            return false;
        };
        self.pending.push(spread);
        self.inputs += 1;
        if self.pending.len() >= Self::SPREAD_BATCH {
            self.collapse_pending();
        }
        true
    }

    /// How many inputs have been folded in.
    #[must_use]
    pub fn inputs(&self) -> usize {
        self.inputs
    }

    /// Rows currently held, pending spreads included as the rows they will
    /// print. The number a memory bound is asserted against: it answers to the
    /// level cap and the batch size, never to how many bars were folded.
    #[must_use]
    pub fn rows_held(&self) -> usize {
        self.levels.len() + self.pending.len()
    }

    /// The profile of everything folded so far, or `None` when nothing
    /// contributed. Costs a fold of the pending
    /// spreads and a clone of the accumulated rows; it does not consume or
    /// mutate the fold, so a consumer may read a partial answer as often as it
    /// likes and keep folding.
    #[must_use]
    pub fn profile(&self) -> Option<VolumeProfile> {
        if self.inputs == 0 {
            return None;
        }
        let mut doublings = self.doublings;
        let mut levels = self.levels.clone();
        if !self.pending.is_empty() {
            let (spread_rows, spread_doublings) =
                fold_spreads(&self.pending, doublings, self.level_cap);
            if spread_doublings > doublings {
                for _ in doublings..spread_doublings {
                    levels = halved(levels);
                }
                doublings = spread_doublings;
            }
            merge_into(&mut levels, spread_rows);
        }
        while levels.len() > self.level_cap {
            levels = halved(levels);
            doublings += 1;
        }
        let mut group = self.base_group;
        for _ in 0..doublings {
            group = group.saturating_mul(Decimal::TWO);
        }
        Some(VolumeProfile::from_parts(levels, group, doublings > 0))
    }

    /// Fold every pending spread in, so later reads cost a clone of the rows
    /// and nothing else.
    ///
    /// [`profile`](Self::profile) takes `&self`, so it folds the pending
    /// spreads on *every* call — right for a fold still running, waste for one
    /// that is finished and read again and again (a chart re-reads a completed
    /// fold whenever its forming bar moves). A consumer that knows it has
    /// pushed its last input calls this once. Nothing else changes:
    /// collapsing early and collapsing late reach the same rows.
    pub fn seal(&mut self) {
        self.collapse_pending();
    }

    /// Fold the pending spreads into the accumulated rows.
    fn collapse_pending(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let (rows, doublings) = fold_spreads(&self.pending, self.doublings, self.level_cap);
        self.pending.clear();
        self.raise_to(doublings);
        merge_into(&mut self.levels, rows);
        self.cap_rows();
    }

    /// Bring the accumulator up to `doublings` if it is finer. Coarsening is
    /// the exact integer halving the merge uses, so folding early and folding
    /// late reach the same rows.
    fn raise_to(&mut self, doublings: u32) {
        while self.doublings < doublings {
            self.levels = halved(std::mem::take(&mut self.levels));
            self.doublings += 1;
        }
    }

    /// Hold the accumulator to the level cap. Row count only ever grows as
    /// inputs arrive, so a fold that coarsens here is one the whole-range
    /// merge would also have coarsened.
    fn cap_rows(&mut self) {
        while self.levels.len() > self.level_cap {
            self.levels = halved(std::mem::take(&mut self.levels));
            self.doublings += 1;
        }
    }
}

/// Sum `rows` into `levels`.
fn merge_into(levels: &mut BTreeMap<i64, FootprintLevel>, rows: BTreeMap<i64, FootprintLevel>) {
    for (bucket, level) in rows {
        let target = levels.entry(bucket).or_default();
        target.buy = target.buy.saturating_add(level.buy);
        target.sell = target.sell.saturating_add(level.sell);
        target.trade_count = target.trade_count.saturating_add(level.trade_count);
    }
}

/// Print `spreads` as rows, at `floor` doublings or coarser.
///
/// The spreads go into a difference map — two touches for a run of rows, one
/// for the remainder the close is owed — which is then run-summed into the
/// rows themselves. Coarser: the printed rows are counted *before* any of them
/// exist, and a count over the cap doubles the grouping and starts again, so a
/// range spanning a million buckets never materialises a million rows.
fn fold_spreads(
    spreads: &[ApproxSpread],
    floor: u32,
    level_cap: usize,
) -> (BTreeMap<i64, FootprintLevel>, u32) {
    let mut doublings = floor.max(
        spreads
            .iter()
            .map(|spread| spread.doublings)
            .max()
            .unwrap_or(0),
    );
    loop {
        let diff = difference_map(spreads, doublings);
        if printed_rows(&diff) <= level_cap {
            return (print_rows(&diff), doublings);
        }
        doublings += 1;
    }
}

/// The difference map of every spread at `doublings`: value at a bucket is the
/// running sum of the deltas at and before it.
fn difference_map(spreads: &[ApproxSpread], doublings: u32) -> BTreeMap<i64, Delta> {
    let mut diff: BTreeMap<i64, Delta> = BTreeMap::new();
    for spread in spreads {
        let raw_shift = doublings - spread.doublings;
        // Past 63 every bucket folds onto 0 (or -1), which is what the block
        // arithmetic below reproduces when the shift is held there.
        let shift = raw_shift.min(63);
        let lo = fold_bucket(spread.lo, raw_shift);
        let hi = fold_bucket(spread.hi, raw_shift);
        // Source rows of `target` that the spread actually covers: a full
        // block in the interior, a partial one at each end.
        let covered = |target: i64| -> i128 {
            let start = (i128::from(target) << shift).max(i128::from(spread.lo));
            let end =
                ((i128::from(target) << shift) + (1i128 << shift) - 1).min(i128::from(spread.hi));
            (end - start + 1).max(0)
        };

        if lo == hi {
            add_at(&mut diff, lo, spread, covered(lo));
        } else {
            add_at(&mut diff, lo, spread, covered(lo));
            add_at(&mut diff, hi, spread, covered(hi));
            if hi - lo > 1 {
                // The interior blocks are full and identical — one range add
                // covers all of them, however many there are.
                let full = 1i128 << shift;
                let run = row_delta(spread, full);
                delta_at(&mut diff, lo + 1).add(&run);
                delta_at(&mut diff, hi).sub(&run);
            }
        }
        // Remainders the shares rounded away, on the close's row.
        let close = fold_bucket(spread.close, raw_shift);
        let point = delta_at(&mut diff, close);
        point.buy = point.buy.saturating_add(spread.buy_extra);
        point.sell = point.sell.saturating_add(spread.sell_extra);
        point.count += i128::from(spread.count_extra);
        delta_at(&mut diff, close.saturating_add(1)).sub(&Delta {
            buy: spread.buy_extra,
            sell: spread.sell_extra,
            count: i128::from(spread.count_extra),
            cover: 0,
        });
        // Coverage: every bucket the spread reaches prints a row.
        delta_at(&mut diff, lo).cover += 1;
        delta_at(&mut diff, hi.saturating_add(1)).cover -= 1;
    }
    diff
}

/// One target bucket's share of a spread: `rows` source rows of it.
fn row_delta(spread: &ApproxSpread, rows: i128) -> Delta {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let factor = Decimal::from(rows.clamp(0, i128::from(u64::MAX)) as u64);
    Delta {
        buy: spread.buy_share.saturating_mul(factor),
        sell: spread.sell_share.saturating_mul(factor),
        count: i128::from(spread.count_share) * rows,
        cover: 0,
    }
}

/// Add one target bucket's share as a single-bucket range.
fn add_at(diff: &mut BTreeMap<i64, Delta>, bucket: i64, spread: &ApproxSpread, rows: i128) {
    let delta = row_delta(spread, rows);
    delta_at(diff, bucket).add(&delta);
    delta_at(diff, bucket.saturating_add(1)).sub(&delta);
}

fn delta_at(diff: &mut BTreeMap<i64, Delta>, bucket: i64) -> &mut Delta {
    diff.entry(bucket).or_default()
}

impl Delta {
    /// Every field, coverage included. It carried three of the four once and
    /// the callers made up the difference by hand — a method named `add` that
    /// quietly skips a field prints the wrong rows the first time someone
    /// trusts its name, and prints them without failing anything.
    fn add(&mut self, other: &Delta) {
        self.buy = self.buy.saturating_add(other.buy);
        self.sell = self.sell.saturating_add(other.sell);
        self.count += other.count;
        self.cover += other.cover;
    }

    fn sub(&mut self, other: &Delta) {
        self.buy = self.buy.saturating_sub(other.buy);
        self.sell = self.sell.saturating_sub(other.sell);
        self.count -= other.count;
        self.cover -= other.cover;
    }
}

/// How many rows the difference map would print — counted from the runs
/// between its boundaries, without printing any of them.
fn printed_rows(diff: &BTreeMap<i64, Delta>) -> usize {
    let mut rows: u128 = 0;
    let mut cover = 0i64;
    let mut previous: Option<i64> = None;
    for (&bucket, delta) in diff {
        if let Some(start) = previous
            && cover > 0
        {
            rows += u128::try_from(i128::from(bucket) - i128::from(start)).unwrap_or(0);
        }
        cover += delta.cover;
        previous = Some(bucket);
    }
    usize::try_from(rows).unwrap_or(usize::MAX)
}

/// Run-sum the difference map into the rows it prints.
fn print_rows(diff: &BTreeMap<i64, Delta>) -> BTreeMap<i64, FootprintLevel> {
    let mut rows: BTreeMap<i64, FootprintLevel> = BTreeMap::new();
    let mut running = Delta::default();
    let mut previous: Option<i64> = None;
    for (&bucket, delta) in diff {
        if let Some(start) = previous
            && running.cover > 0
        {
            let level = FootprintLevel {
                buy: running.buy,
                sell: running.sell,
                trade_count: u64::try_from(running.count.max(0)).unwrap_or(u64::MAX),
            };
            for row in start..bucket {
                rows.insert(row, level.clone());
            }
        }
        running.add(delta);
        previous = Some(bucket);
    }
    rows
}
