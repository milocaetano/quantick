//! The session-wide bubble size scales, maintained one print at a time.
//!
//! The automatic size references (P99 / max) answer one question: *what
//! quantity deserves a full-size mark?* The answer must not depend on the
//! viewport — zoom decides what is on screen, never what a quantity means —
//! and it must not cost a pass over the whole retained tape every time the
//! settled projection rebuilds, which under a dense feed is every rebuild
//! cadence. So both scales are accumulated here, incrementally: every print
//! folds into them as it is recorded, and a query reads what is already
//! known.
//!
//! There are two, because the chart draws two kinds of mark an order of
//! magnitude apart. [`SessionScale`] is the scale of a *print*: clusters at
//! capture resolution, what the tape and an unsummarized slot draw.
//! [`SummaryScale`] is the scale of a *pie*: a closed bar's whole flow at one
//! price range, sized against the busiest minute a price level saw this
//! session. One shared reference would peg every pie at the largest radius
//! while flattening the tape into dots — the reason the tiers keep separate
//! scales — but both scales obey the same law:
//!
//! - **Viewport-free.** A print clusters at native price grouping — same
//!   side, same base bucket, within [`HeatmapConfig::bubble_cluster_ms`](
//!   super::config::HeatmapConfig::bubble_cluster_ms) of the cluster's first
//!   print. A summary total accumulates per native bucket over a fixed
//!   [`SUMMARY_WINDOW_MS`] of tape time, deliberately not per bar: bars are a
//!   display choice the way display grouping is, so "the busiest minute this
//!   level saw" keeps its meaning across a bar-spec switch exactly as it
//!   does across a zoom. Display grouping contributes nothing to either
//!   scale: a mark the viewport merges past its scale simply saturates at
//!   the maximum radius, the honest reading of "more than anything the scale
//!   measures". Resync generations are ignored for the same reason — a
//!   burst of aggression does not become two bursts because the book
//!   resynchronized under it.
//! - **The scale never forgets.** Retention trims what the chart *draws*;
//!   it does not change what a quantity *means*. A sweep at 10:00 still
//!   anchors the scale at 15:00, so the same quantity reads the same size
//!   all session instead of slowly inflating as big prints age out. A scale
//!   resets only when the session itself does (a symbol switch, a
//!   price-grouping reset) or when its own clustering input changes — and a
//!   rebuild then replays only what is still retained, which is everything
//!   the chart can still show.
//! - **Deterministic.** Integer time math, `Decimal` quantities, `BTreeMap`
//!   everywhere: the same prints in the same order produce the same scales.

use std::collections::BTreeMap;
use std::mem::size_of;

use quantick_engine::Side;
use rust_decimal::Decimal;

/// Tape time behind one summary-scale total, per price bucket.
///
/// Fixed, and deliberately not the bar duration: the full-size pie has to
/// mean the same quantity whatever bars the chart is cut into, or switching
/// from tick to time bars would silently rescale history. One minute is the
/// same order of magnitude as the bars a summary is read on, so a typical
/// pie lands mid-scale and only a violent bar saturates.
pub const SUMMARY_WINDOW_MS: i64 = 60_000;

/// One open cluster: anchored at its first print, still accepting joins.
#[derive(Debug, Clone, Copy)]
struct OpenCluster {
    anchor_ms: i64,
    quantity: Decimal,
}

/// One open per-bucket summary window: totals prints until the minute turns.
#[derive(Debug, Clone, Copy)]
struct OpenWindow {
    window_index: i64,
    quantity: Decimal,
}

/// Accumulated multiset of quantities with rank queries.
///
/// The shared core of both scales: entries only ever grow in count (the
/// ratchet), an *open* aggregate is counted at its current sum and re-counted
/// as it grows, and P99/max read the distribution without a sort.
#[derive(Debug, Clone, Default)]
struct Distribution {
    counts: BTreeMap<Decimal, u64>,
    total: u64,
}

impl Distribution {
    /// Count a fresh aggregate.
    fn insert(&mut self, quantity: Decimal) {
        *self.counts.entry(quantity).or_insert(0) += 1;
        self.total += 1;
    }

    /// Re-count an open aggregate that grew from `previous` to `grown`.
    fn replace(&mut self, previous: Decimal, grown: Decimal) {
        if let Some(count) = self.counts.get_mut(&previous) {
            *count -= 1;
            if *count == 0 {
                self.counts.remove(&previous);
            }
        }
        *self.counts.entry(grown).or_insert(0) += 1;
    }

    /// Largest quantity counted.
    fn max(&self) -> Decimal {
        self.counts
            .last_key_value()
            .map_or(Decimal::ZERO, |(quantity, _)| *quantity)
    }

    /// 99th-percentile quantity, by the same rank the projection's visible
    /// percentile always used: over `n` quantities sorted ascending, the one
    /// at rank `ceil(99·n / 100)`.
    fn p99(&self) -> Decimal {
        if self.total == 0 {
            return Decimal::ZERO;
        }
        let rank = (99 * self.total).div_ceil(100);
        // Walking from the top touches ~1% of the distinct quantities: the
        // ascending rank `rank` is the descending rank `n − rank + 1`.
        let mut remaining = self.total - rank + 1;
        for (quantity, count) in self.counts.iter().rev() {
            if *count >= remaining {
                return *quantity;
            }
            remaining -= count;
        }
        Decimal::ZERO
    }

    /// Approximate bytes held — one entry per *distinct* quantity, the one
    /// part of a scale a long session grows without bound.
    fn approximate_bytes(&self) -> usize {
        self.counts.len() * size_of::<(Decimal, u64)>()
    }
}

/// Accumulated distribution of session cluster quantities — the print scale.
///
/// Owned by [`LiquidityHistory`](super::history::LiquidityHistory), fed from
/// [`record_aggression`](super::history::LiquidityHistory::record_aggression),
/// read by the projection as the bubble size reference.
#[derive(Debug, Clone)]
pub struct SessionScale {
    bucket_width: Decimal,
    cluster_ms: i64,
    /// The still-growing cluster per (aggressor side, native bucket).
    open: BTreeMap<(u8, Decimal), OpenCluster>,
    distribution: Distribution,
}

impl SessionScale {
    /// An empty scale clustering at `bucket_width` and `cluster_ms`.
    ///
    /// A non-positive `bucket_width` degenerates to per-price clustering,
    /// mirroring how the grouping sweep treats it.
    #[must_use]
    pub fn new(bucket_width: Decimal, cluster_ms: i64) -> Self {
        Self {
            bucket_width,
            cluster_ms: cluster_ms.max(0),
            open: BTreeMap::new(),
            distribution: Distribution::default(),
        }
    }

    /// Fold one print into the scale.
    pub fn record(&mut self, timestamp_ms: i64, price: Decimal, quantity: Decimal, side: Side) {
        if quantity <= Decimal::ZERO {
            return;
        }
        let bucket = bucket_for(price, self.bucket_width);
        let key = (side_key(side), bucket);
        if self.cluster_ms > 0
            && let Some(open) = self.open.get_mut(&key)
            && timestamp_ms.saturating_sub(open.anchor_ms) <= self.cluster_ms
        {
            let previous = open.quantity;
            open.quantity += quantity;
            let grown = open.quantity;
            self.distribution.replace(previous, grown);
            return;
        }
        // The previous cluster on this key (if any) is closed by displacement:
        // its quantity simply stays counted.
        self.open.insert(
            key,
            OpenCluster {
                anchor_ms: timestamp_ms,
                quantity,
            },
        );
        self.distribution.insert(quantity);
    }

    /// Largest cluster quantity the session produced.
    #[must_use]
    pub fn max(&self) -> Decimal {
        self.distribution.max()
    }

    /// 99th-percentile cluster quantity.
    #[must_use]
    pub fn p99(&self) -> Decimal {
        self.distribution.p99()
    }

    /// Approximate bytes held by the accumulated distribution, counted so the
    /// history's memory estimate never hides it.
    #[must_use]
    pub fn approximate_bytes(&self) -> usize {
        self.distribution.approximate_bytes()
            + self.open.len() * size_of::<((u8, Decimal), OpenCluster)>()
    }

    #[cfg(test)]
    fn clusters(&self) -> u64 {
        self.distribution.total
    }
}

/// Accumulated distribution of per-minute, per-level totals — the pie scale.
///
/// Both sides sum into one total, because a pie is the one mark that carries
/// both sides of a price at once.
#[derive(Debug, Clone)]
pub struct SummaryScale {
    bucket_width: Decimal,
    /// The still-filling minute per native bucket.
    open: BTreeMap<Decimal, OpenWindow>,
    distribution: Distribution,
}

impl SummaryScale {
    /// An empty scale totalling per `bucket_width` level.
    #[must_use]
    pub fn new(bucket_width: Decimal) -> Self {
        Self {
            bucket_width,
            open: BTreeMap::new(),
            distribution: Distribution::default(),
        }
    }

    /// Fold one print into the scale.
    pub fn record(&mut self, timestamp_ms: i64, price: Decimal, quantity: Decimal) {
        if quantity <= Decimal::ZERO {
            return;
        }
        let bucket = bucket_for(price, self.bucket_width);
        let window_index = timestamp_ms.div_euclid(SUMMARY_WINDOW_MS);
        if let Some(open) = self.open.get_mut(&bucket)
            && open.window_index == window_index
        {
            let previous = open.quantity;
            open.quantity += quantity;
            let grown = open.quantity;
            self.distribution.replace(previous, grown);
            return;
        }
        // The minute turned (or the level is new): the finished total stays
        // counted and a fresh one opens.
        self.open.insert(
            bucket,
            OpenWindow {
                window_index,
                quantity,
            },
        );
        self.distribution.insert(quantity);
    }

    /// Largest per-minute level total the session produced.
    #[must_use]
    pub fn max(&self) -> Decimal {
        self.distribution.max()
    }

    /// 99th-percentile per-minute level total.
    #[must_use]
    pub fn p99(&self) -> Decimal {
        self.distribution.p99()
    }

    /// Approximate bytes held by the accumulated distribution.
    #[must_use]
    pub fn approximate_bytes(&self) -> usize {
        self.distribution.approximate_bytes() + self.open.len() * size_of::<(Decimal, OpenWindow)>()
    }
}

/// The native-bucket lower edge containing `price`; non-positive widths
/// degenerate to per-price, mirroring the grouping sweep.
fn bucket_for(price: Decimal, bucket_width: Decimal) -> Decimal {
    if bucket_width > Decimal::ZERO {
        (price / bucket_width).floor() * bucket_width
    } else {
        price
    }
}

fn side_key(side: Side) -> u8 {
    match side {
        Side::Buy => 0,
        Side::Sell => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    #[test]
    fn prints_on_one_level_within_the_window_are_one_cluster() {
        let mut scale = SessionScale::new(Decimal::ONE, 1_000);
        scale.record(0, dec("100"), dec("3"), Side::Buy);
        scale.record(500, dec("100.4"), dec("2"), Side::Buy);
        // Same native bucket, same side, inside the window: one cluster of 5.
        assert_eq!(scale.max(), dec("5"));
        assert_eq!(scale.p99(), dec("5"));
        // Outside the window: a new cluster, and the old sum stays counted.
        scale.record(2_000, dec("100"), dec("7"), Side::Buy);
        assert_eq!(scale.max(), dec("7"));
        scale.record(2_100, dec("100"), dec("1"), Side::Sell);
        // The opposite side never joins: a sell opens its own cluster of 1
        // instead of growing the buy cluster to 8.
        assert_eq!(scale.max(), dec("7"));
        assert_eq!(scale.clusters(), 3);
    }

    #[test]
    fn raw_mode_keeps_every_print_its_own_cluster() {
        let mut scale = SessionScale::new(Decimal::ONE, 0);
        scale.record(0, dec("100"), dec("3"), Side::Buy);
        scale.record(1, dec("100"), dec("2"), Side::Buy);
        assert_eq!(scale.max(), dec("3"));
    }

    #[test]
    fn p99_matches_the_projections_rank_rule() {
        // 100 distinct clusters, quantities 1..=100: rank ceil(9900/100) = 99.
        let mut scale = SessionScale::new(Decimal::ONE, 0);
        for value in 1..=100u32 {
            scale.record(
                i64::from(value),
                Decimal::from(value),
                Decimal::from(value),
                Side::Buy,
            );
        }
        assert_eq!(scale.p99(), dec("99"));
        assert_eq!(scale.max(), dec("100"));
        // Small sets degenerate to the maximum, exactly as the rank does.
        let mut small = SessionScale::new(Decimal::ONE, 0);
        for value in [dec("2"), dec("50")] {
            small.record(0, dec("100"), value, Side::Sell);
        }
        assert_eq!(small.p99(), dec("50"));
    }

    #[test]
    fn a_growing_open_cluster_is_recounted_not_double_counted() {
        let mut scale = SessionScale::new(Decimal::ONE, 1_000);
        scale.record(0, dec("100"), dec("3"), Side::Buy);
        scale.record(100, dec("100"), dec("3"), Side::Buy);
        // One cluster of 6 — not a 3 and a 6.
        assert_eq!(scale.max(), dec("6"));
        assert_eq!(scale.p99(), dec("6"));
        assert_eq!(scale.clusters(), 1);
    }

    #[test]
    fn dust_never_moves_the_scale() {
        let mut scale = SessionScale::new(Decimal::ONE, 0);
        scale.record(0, dec("100"), Decimal::ZERO, Side::Buy);
        scale.record(0, dec("100"), dec("-1"), Side::Buy);
        assert_eq!(scale.max(), Decimal::ZERO);
        assert_eq!(scale.p99(), Decimal::ZERO);
    }

    #[test]
    fn a_minute_totals_both_sides_of_one_level() {
        let mut scale = SummaryScale::new(Decimal::ONE);
        scale.record(1_000, dec("100"), dec("3"));
        scale.record(59_000, dec("100.7"), dec("2"));
        // Same bucket, same minute, sides irrelevant: one total of 5.
        assert_eq!(scale.max(), dec("5"));
        // The next minute opens a new total; the old one stays counted.
        scale.record(61_000, dec("100"), dec("1"));
        assert_eq!(scale.max(), dec("5"));
        // A different level totals apart even inside the same minute.
        scale.record(61_500, dec("105"), dec("9"));
        assert_eq!(scale.max(), dec("9"));
    }

    #[test]
    fn the_busiest_minute_is_a_ratchet() {
        let mut scale = SummaryScale::new(Decimal::ONE);
        scale.record(0, dec("100"), dec("40"));
        // Hours later the tape is quiet; the anchor from the loud minute
        // still stands.
        scale.record(7_200_000, dec("100"), dec("1"));
        assert_eq!(scale.max(), dec("40"));
    }
}
