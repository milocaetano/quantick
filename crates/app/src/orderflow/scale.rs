//! The session-wide bubble size scale, maintained one print at a time.
//!
//! The automatic size references (P99 / max) answer one question: *what
//! quantity deserves a full-size bubble?* The answer must not depend on the
//! viewport — zoom decides what is on screen, never what a quantity means —
//! and it must not cost a pass over the whole retained tape every time the
//! settled projection rebuilds, which under a dense feed is every rebuild
//! cadence. So the scale is accumulated here, incrementally: every print
//! folds into it as it is recorded, and a query reads what is already known.
//!
//! Three deliberate properties, all in the name of a bubble meaning the same
//! thing all session:
//!
//! - **Clustered at capture resolution.** Prints are clustered exactly the
//!   way the tape clusters them at native price grouping — same side, same
//!   base bucket, within [`HeatmapConfig::bubble_cluster_ms`](
//!   super::config::HeatmapConfig::bubble_cluster_ms) of the cluster's first
//!   print — so the scale speaks the same unit as the marks it sizes.
//!   Display grouping of any kind (a coarser multiple, the adaptive mode) is
//!   a *viewport* decision and contributes nothing: a cluster the display
//!   merges past this scale simply saturates at the maximum radius, the
//!   honest reading of "more than anything the scale measures". Resync
//!   generations are ignored for the same reason — a burst of aggression
//!   does not become two bursts because the book resynchronized under it.
//! - **The scale never forgets.** Retention trims what the chart *draws*;
//!   it does not change what a quantity *means*. A sweep at 10:00 still
//!   anchors the scale at 15:00, so the same quantity reads the same size
//!   all session instead of slowly inflating as big prints age out. The
//!   scale resets only when the session itself does (a symbol switch, a
//!   price-grouping reset) or when the clustering window changes — and a
//!   rebuild then replays only what is still retained, which is everything
//!   the chart can still show.
//! - **Deterministic.** Integer time math, `Decimal` quantities, `BTreeMap`
//!   everywhere: the same prints in the same order produce the same scale.

use std::collections::BTreeMap;

use quantick_engine::Side;
use rust_decimal::Decimal;

/// One open cluster: anchored at its first print, still accepting joins.
#[derive(Debug, Clone, Copy)]
struct OpenCluster {
    anchor_ms: i64,
    quantity: Decimal,
}

/// Accumulated distribution of session cluster quantities.
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
    /// Multiset of every cluster quantity the session produced; an open
    /// cluster is counted at its current sum and re-counted as it grows.
    counts: BTreeMap<Decimal, u64>,
    /// Total clusters counted — the `n` a percentile rank is taken over.
    clusters: u64,
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
            counts: BTreeMap::new(),
            clusters: 0,
        }
    }

    /// Fold one print into the scale.
    pub fn record(&mut self, timestamp_ms: i64, price: Decimal, quantity: Decimal, side: Side) {
        if quantity <= Decimal::ZERO {
            return;
        }
        let bucket = if self.bucket_width > Decimal::ZERO {
            (price / self.bucket_width).floor() * self.bucket_width
        } else {
            price
        };
        let key = (side_key(side), bucket);
        if self.cluster_ms > 0
            && let Some(open) = self.open.get_mut(&key)
            && timestamp_ms.saturating_sub(open.anchor_ms) <= self.cluster_ms
        {
            let previous = open.quantity;
            open.quantity += quantity;
            let grown = open.quantity;
            remove_count(&mut self.counts, previous);
            *self.counts.entry(grown).or_insert(0) += 1;
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
        *self.counts.entry(quantity).or_insert(0) += 1;
        self.clusters += 1;
    }

    /// Largest cluster quantity the session produced.
    #[must_use]
    pub fn max(&self) -> Decimal {
        self.counts
            .last_key_value()
            .map_or(Decimal::ZERO, |(quantity, _)| *quantity)
    }

    /// 99th-percentile cluster quantity, by the same rank the projection's
    /// visible percentile always used: over `n` quantities sorted ascending,
    /// the one at rank `ceil(99·n / 100)`.
    #[must_use]
    pub fn p99(&self) -> Decimal {
        if self.clusters == 0 {
            return Decimal::ZERO;
        }
        let rank = (99 * self.clusters).div_ceil(100);
        // Walking from the top touches ~1% of the distinct quantities: the
        // ascending rank `rank` is the descending rank `n − rank + 1`.
        let mut remaining = self.clusters - rank + 1;
        for (quantity, count) in self.counts.iter().rev() {
            if *count >= remaining {
                return *quantity;
            }
            remaining -= count;
        }
        Decimal::ZERO
    }
}

fn side_key(side: Side) -> u8 {
    match side {
        Side::Buy => 0,
        Side::Sell => 1,
    }
}

fn remove_count(counts: &mut BTreeMap<Decimal, u64>, quantity: Decimal) {
    if let Some(count) = counts.get_mut(&quantity) {
        *count -= 1;
        if *count == 0 {
            counts.remove(&quantity);
        }
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
        assert_eq!(scale.clusters, 3);
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
        assert_eq!(scale.clusters, 1);
    }

    #[test]
    fn dust_never_moves_the_scale() {
        let mut scale = SessionScale::new(Decimal::ONE, 0);
        scale.record(0, dec("100"), Decimal::ZERO, Side::Buy);
        scale.record(0, dec("100"), dec("-1"), Side::Buy);
        assert_eq!(scale.max(), Decimal::ZERO);
        assert_eq!(scale.p99(), Decimal::ZERO);
    }
}
