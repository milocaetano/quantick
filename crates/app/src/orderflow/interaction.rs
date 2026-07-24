//! Pure aggression clustering and non-causal liquidity interaction evidence.
//!
//! A compatible aggressive print can be aligned with a displayed-liquidity
//! reduction, but neither the depth stream nor aggregate trades identify
//! cancellation versus execution causally. The vocabulary here deliberately
//! remains factual: matched aggression evidence or depth-only reduction.

use std::collections::BTreeMap;

use quantick_engine::Side;
use quantick_orderbook::BookSide;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::grouping::{EffectiveGrouping, LiquidityTransition, bucket_for_price};
use super::history::{Aggression, AggressorSide, CoverageSegment, RestingSide};

/// Evidence available beside a displayed-liquidity reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidityEvidence {
    /// Compatible aggressive quantity exists near the factual depth reduction.
    AggressionAligned,
    /// Only the factual before/after depth observation is available.
    DepthOnly,
}

/// One deterministic cluster of compatible aggressive prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggressionCluster {
    /// Stable representative id (the first id in deterministic time order).
    pub agg_id: u64,
    /// Every aggregate-trade id represented by this cluster.
    pub agg_ids: Vec<u64>,
    /// Coverage generation derived from exchange timestamp, if available.
    pub generation: Option<u64>,
    /// Taker side.
    pub side: AggressorSide,
    /// Passive side compatible with the print.
    pub consumed_side: RestingSide,
    /// Inclusive lower edge of the visual price range.
    pub price_bucket: Decimal,
    /// Exact summed execution quantity.
    pub quantity: Decimal,
    /// Quantity-weighted execution price.
    pub price: Decimal,
    /// Deterministic visual timestamp centered in the cluster interval.
    pub timestamp_ms: i64,
    /// Earliest exchange trade timestamp represented.
    pub first_timestamp_ms: i64,
    /// Latest exchange trade timestamp represented.
    pub last_timestamp_ms: i64,
    /// Number of aggregate trades represented.
    pub trade_count: usize,
    /// Quantity conservatively allocated to compatible liquidity reductions.
    pub matched_quantity: Decimal,
    /// Event ids receiving at least part of this cluster's quantity.
    pub liquidity_event_ids: Vec<u64>,
}

impl AggressionCluster {
    /// Fraction of this bubble's exact quantity aligned with reductions.
    #[must_use]
    pub fn matched_fraction(&self) -> f32 {
        decimal_fraction(self.matched_quantity, self.quantity)
    }
}

/// One factual liquidity reduction before projection into chart coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct LiquidityEvent {
    /// Deterministic frame-local id.
    pub event_id: u64,
    /// Synchronization generation.
    pub generation: u64,
    /// Resting side.
    pub side: RestingSide,
    /// Inclusive lower edge of the visual price range.
    pub price_bucket: Decimal,
    /// Exchange observation timestamp.
    pub timestamp_ms: i64,
    /// Displayed quantity immediately before the reduction.
    pub before: Decimal,
    /// Displayed quantity immediately after the reduction.
    pub after: Decimal,
    /// Exact factual reduction (`before - after`).
    pub removed: Decimal,
    /// Reduction divided by `before`.
    pub fraction: f32,
    /// Whether the displayed visual range became empty.
    pub full_removal: bool,
    /// Exact compatible aggression quantity allocated to this event.
    pub matched_quantity: Decimal,
    /// Matched quantity divided by the factual reduction.
    pub matched_fraction: f32,
    /// Available evidence, without a causal execution/cancellation claim.
    pub evidence: LiquidityEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ClusterKey {
    generation: Option<u64>,
    side: u8,
    price_bucket: Decimal,
}

#[derive(Debug)]
struct ClusterBuilder {
    key: ClusterKey,
    side: AggressorSide,
    first_timestamp_ms: i64,
    last_timestamp_ms: i64,
    quantity: Decimal,
    price_quantity: Decimal,
    first_price: Decimal,
    agg_ids: Vec<u64>,
}

impl ClusterBuilder {
    fn new(trade: &Aggression, generation: Option<u64>, price_bucket: Decimal) -> ClusterBuilder {
        ClusterBuilder {
            key: ClusterKey {
                generation,
                side: aggressor_side_key(trade.side),
                price_bucket,
            },
            side: trade.side,
            first_timestamp_ms: trade.timestamp_ms,
            last_timestamp_ms: trade.timestamp_ms,
            quantity: trade.quantity,
            price_quantity: trade.price * trade.quantity,
            first_price: trade.price,
            agg_ids: vec![trade.agg_id],
        }
    }

    fn push(&mut self, trade: &Aggression) {
        self.last_timestamp_ms = self.last_timestamp_ms.max(trade.timestamp_ms);
        self.quantity += trade.quantity;
        self.price_quantity += trade.price * trade.quantity;
        self.agg_ids.push(trade.agg_id);
    }

    fn finish(self) -> AggressionCluster {
        let price = if self.quantity > Decimal::ZERO {
            self.price_quantity / self.quantity
        } else {
            self.first_price
        };
        let timestamp_ms = self.first_timestamp_ms.saturating_add(
            self.last_timestamp_ms
                .saturating_sub(self.first_timestamp_ms)
                / 2,
        );
        AggressionCluster {
            agg_id: self.agg_ids[0],
            agg_ids: self.agg_ids,
            generation: self.key.generation,
            side: self.side,
            consumed_side: consumed_side(self.side),
            price_bucket: self.key.price_bucket,
            quantity: self.quantity,
            price,
            timestamp_ms,
            first_timestamp_ms: self.first_timestamp_ms,
            last_timestamp_ms: self.last_timestamp_ms,
            trade_count: 0, // Filled from the preserved id vector below.
            matched_quantity: Decimal::ZERO,
            liquidity_event_ids: Vec::new(),
        }
        .with_trade_count()
    }
}

trait WithTradeCount {
    fn with_trade_count(self) -> Self;
}

impl WithTradeCount for AggressionCluster {
    fn with_trade_count(mut self) -> Self {
        self.trade_count = self.agg_ids.len();
        self
    }
}

/// Resolve the honest coverage generation for an exchange timestamp.
///
/// The stored generation on an asynchronously observed trade is intentionally
/// not trusted for correlation; out-of-order delivery can make it stale.
#[must_use]
pub fn generation_at(timestamp_ms: i64, coverage: &[CoverageSegment]) -> Option<u64> {
    coverage
        .iter()
        .find(|segment| {
            timestamp_ms >= segment.start_ms
                && segment.end_ms.is_none_or(|end_ms| timestamp_ms < end_ms)
        })
        .map(|segment| segment.generation)
}

/// Cluster aggressive prints by taker side, visual range, coverage and time.
///
/// `cluster_ms == 0` is raw mode. Otherwise a cluster is anchored at its first
/// trade and never spans more than the requested window, avoiding order- or
/// chain-dependent grouping.
#[must_use]
pub fn cluster_aggressions<'a>(
    aggressions: impl IntoIterator<Item = &'a Aggression>,
    coverage: &[CoverageSegment],
    grouping: EffectiveGrouping,
    cluster_ms: i64,
) -> Vec<AggressionCluster> {
    let cluster_ms = cluster_ms.max(0);
    let mut trades: Vec<(&Aggression, ClusterKey)> = aggressions
        .into_iter()
        .map(|trade| {
            let key = ClusterKey {
                generation: generation_at(trade.timestamp_ms, coverage),
                side: aggressor_side_key(trade.side),
                price_bucket: bucket_for_price(trade.price, grouping),
            };
            (trade, key)
        })
        .collect();
    trades.sort_by(|(a, a_key), (b, b_key)| {
        a_key
            .cmp(b_key)
            .then_with(|| a.timestamp_ms.cmp(&b.timestamp_ms))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });

    let mut clusters = Vec::new();
    let mut current: Option<ClusterBuilder> = None;
    for (trade, key) in trades {
        let joins = current.as_ref().is_some_and(|cluster| {
            cluster_ms > 0
                && cluster.key == key
                && trade
                    .timestamp_ms
                    .saturating_sub(cluster.first_timestamp_ms)
                    <= cluster_ms
        });
        if joins {
            current
                .as_mut()
                .expect("join decision requires a current cluster")
                .push(trade);
            continue;
        }
        if let Some(cluster) = current.take() {
            clusters.push(cluster.finish());
        }
        current = Some(ClusterBuilder::new(trade, key.generation, key.price_bucket));
    }
    if let Some(cluster) = current {
        clusters.push(cluster.finish());
    }

    clusters.sort_by(|a, b| {
        a.first_timestamp_ms
            .cmp(&b.first_timestamp_ms)
            .then_with(|| a.last_timestamp_ms.cmp(&b.last_timestamp_ms))
            .then_with(|| aggressor_side_key(a.side).cmp(&aggressor_side_key(b.side)))
            .then_with(|| a.price_bucket.cmp(&b.price_bucket))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });
    clusters
}

/// Convert factual grouped transitions into reduction events.
#[must_use]
pub fn liquidity_events(transitions: &[LiquidityTransition]) -> Vec<LiquidityEvent> {
    let mut reductions: Vec<&LiquidityTransition> = transitions
        .iter()
        .filter(|transition| {
            transition.before > Decimal::ZERO && transition.after < transition.before
        })
        .collect();
    reductions.sort_by(|a, b| {
        a.timestamp_ms
            .cmp(&b.timestamp_ms)
            .then_with(|| a.generation.cmp(&b.generation))
            .then_with(|| resting_side_key(a.side).cmp(&resting_side_key(b.side)))
            .then_with(|| a.price_bucket.cmp(&b.price_bucket))
    });
    reductions
        .into_iter()
        .enumerate()
        .map(|(index, transition)| {
            let removed = transition.before - transition.after;
            LiquidityEvent {
                event_id: u64::try_from(index)
                    .unwrap_or(u64::MAX - 1)
                    .saturating_add(1),
                generation: transition.generation,
                side: transition.side,
                price_bucket: transition.price_bucket,
                timestamp_ms: transition.timestamp_ms,
                before: transition.before,
                after: transition.after,
                removed,
                fraction: decimal_fraction(removed, transition.before),
                full_removal: transition.after <= Decimal::ZERO,
                matched_quantity: Decimal::ZERO,
                matched_fraction: 0.0,
                evidence: LiquidityEvidence::DepthOnly,
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MatchKey {
    generation: u64,
    passive_side: u8,
    price_bucket: Decimal,
}

/// Conservatively allocate compatible aggressive quantity to reductions.
///
/// Each unit of aggressive quantity and each unit of removed liquidity can be
/// allocated at most once. Compatibility requires the passive side, exact
/// visual range, coverage generation derived by timestamp, and temporal
/// proximity. This is evidence alignment only; it does not label cancellation
/// or execution as a cause.
pub fn correlate_liquidity(
    events: &mut [LiquidityEvent],
    clusters: &mut [AggressionCluster],
    correlation_ms: i64,
) {
    let correlation_ms = correlation_ms.max(0);
    for event in events.iter_mut() {
        event.matched_quantity = Decimal::ZERO;
        event.matched_fraction = 0.0;
        event.evidence = LiquidityEvidence::DepthOnly;
    }
    for cluster in clusters.iter_mut() {
        cluster.matched_quantity = Decimal::ZERO;
        cluster.liquidity_event_ids.clear();
    }

    let mut compatible: BTreeMap<MatchKey, Vec<usize>> = BTreeMap::new();
    for (index, cluster) in clusters.iter().enumerate() {
        let Some(generation) = cluster.generation else {
            continue;
        };
        compatible
            .entry(MatchKey {
                generation,
                passive_side: resting_side_key(cluster.consumed_side),
                price_bucket: cluster.price_bucket,
            })
            .or_default()
            .push(index);
    }

    let mut event_order: Vec<usize> = (0..events.len()).collect();
    event_order.sort_by(|&a, &b| {
        events[a]
            .timestamp_ms
            .cmp(&events[b].timestamp_ms)
            .then_with(|| events[a].event_id.cmp(&events[b].event_id))
    });

    for event_index in event_order {
        let key = MatchKey {
            generation: events[event_index].generation,
            passive_side: resting_side_key(events[event_index].side),
            price_bucket: events[event_index].price_bucket,
        };
        let Some(candidates) = compatible.get(&key) else {
            continue;
        };
        let mut event_remaining = events[event_index].removed;
        while event_remaining > Decimal::ZERO {
            let best = candidates
                .iter()
                .copied()
                .filter(|&cluster_index| {
                    clusters[cluster_index].quantity > clusters[cluster_index].matched_quantity
                        && cluster_distance_ms(
                            events[event_index].timestamp_ms,
                            &clusters[cluster_index],
                        ) <= correlation_ms
                })
                .min_by(|&a, &b| {
                    cluster_distance_ms(events[event_index].timestamp_ms, &clusters[a])
                        .cmp(&cluster_distance_ms(
                            events[event_index].timestamp_ms,
                            &clusters[b],
                        ))
                        .then_with(|| {
                            clusters[a]
                                .first_timestamp_ms
                                .cmp(&clusters[b].first_timestamp_ms)
                        })
                        .then_with(|| clusters[a].agg_id.cmp(&clusters[b].agg_id))
                });
            let Some(cluster_index) = best else {
                break;
            };

            let cluster_remaining =
                clusters[cluster_index].quantity - clusters[cluster_index].matched_quantity;
            let allocation = event_remaining.min(cluster_remaining);
            if allocation <= Decimal::ZERO {
                break;
            }
            events[event_index].matched_quantity += allocation;
            clusters[cluster_index].matched_quantity += allocation;
            event_remaining -= allocation;
            if !clusters[cluster_index]
                .liquidity_event_ids
                .contains(&events[event_index].event_id)
            {
                clusters[cluster_index]
                    .liquidity_event_ids
                    .push(events[event_index].event_id);
            }
        }

        events[event_index].matched_fraction = decimal_fraction(
            events[event_index].matched_quantity,
            events[event_index].removed,
        );
        if events[event_index].matched_quantity > Decimal::ZERO {
            events[event_index].evidence = LiquidityEvidence::AggressionAligned;
        }
    }
}

fn cluster_distance_ms(timestamp_ms: i64, cluster: &AggressionCluster) -> i64 {
    if timestamp_ms < cluster.first_timestamp_ms {
        cluster.first_timestamp_ms.saturating_sub(timestamp_ms)
    } else if timestamp_ms > cluster.last_timestamp_ms {
        timestamp_ms.saturating_sub(cluster.last_timestamp_ms)
    } else {
        0
    }
}

fn decimal_fraction(numerator: Decimal, denominator: Decimal) -> f32 {
    if numerator <= Decimal::ZERO || denominator <= Decimal::ZERO {
        return 0.0;
    }
    (numerator / denominator)
        .to_f32()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
}

fn consumed_side(side: AggressorSide) -> RestingSide {
    match side {
        Side::Buy => BookSide::Ask,
        Side::Sell => BookSide::Bid,
    }
}

fn aggressor_side_key(side: AggressorSide) -> u8 {
    match side {
        Side::Buy => 0,
        Side::Sell => 1,
    }
}

fn resting_side_key(side: RestingSide) -> u8 {
    match side {
        BookSide::Bid => 0,
        BookSide::Ask => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::super::config::DisplayGrouping;
    use super::*;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn aggression(
        id: u64,
        timestamp_ms: i64,
        price: &str,
        quantity: &str,
        side: Side,
        stored_generation: Option<u64>,
    ) -> Aggression {
        Aggression {
            agg_id: id,
            timestamp_ms,
            price: dec(price),
            quantity: dec(quantity),
            side,
            generation: stored_generation,
        }
    }

    fn coverage(start_ms: i64, end_ms: Option<i64>) -> CoverageSegment {
        CoverageSegment {
            generation: 3,
            start_ms,
            end_ms,
        }
    }

    fn grouping() -> EffectiveGrouping {
        EffectiveGrouping::resolve(DisplayGrouping::Multiple(2), Decimal::ONE, dec("10"))
    }

    fn reduction(timestamp_ms: i64, before: &str, after: &str) -> LiquidityTransition {
        LiquidityTransition {
            generation: 3,
            side: BookSide::Ask,
            price_bucket: dec("100"),
            timestamp_ms,
            before: dec(before),
            after: dec(after),
        }
    }

    #[test]
    fn clustering_is_order_independent_and_conserves_quantity_ids_and_time() {
        let later = aggression(12, 160, "101", "3", Side::Buy, Some(999));
        let earlier = aggression(11, 100, "100", "2", Side::Buy, None);
        let clusters =
            cluster_aggressions([&later, &earlier], &[coverage(0, None)], grouping(), 100);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].agg_ids, [11, 12]);
        assert_eq!(clusters[0].quantity, dec("5"));
        assert_eq!(clusters[0].trade_count, 2);
        assert_eq!(clusters[0].first_timestamp_ms, 100);
        assert_eq!(clusters[0].last_timestamp_ms, 160);
        assert_eq!(clusters[0].timestamp_ms, 130);
        assert_eq!(clusters[0].generation, Some(3));
        assert_eq!(clusters[0].price, dec("100.6"));
    }

    #[test]
    fn zero_cluster_window_preserves_raw_prints() {
        let first = aggression(1, 100, "100", "2", Side::Buy, Some(3));
        let second = aggression(2, 100, "100", "3", Side::Buy, Some(3));
        let clusters = cluster_aggressions([&first, &second], &[coverage(0, None)], grouping(), 0);
        assert_eq!(clusters.len(), 2);
        assert!(clusters.iter().all(|cluster| cluster.trade_count == 1));
        assert_eq!(
            clusters
                .iter()
                .map(|cluster| cluster.quantity)
                .sum::<Decimal>(),
            dec("5")
        );
    }

    #[test]
    fn partial_and_full_reductions_have_factual_fractions() {
        let events = liquidity_events(&[reduction(100, "10", "6"), reduction(200, "6", "0")]);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].removed, dec("4"));
        assert_eq!(events[0].fraction, 0.4);
        assert!(!events[0].full_removal);
        assert_eq!(events[1].removed, dec("6"));
        assert_eq!(events[1].fraction, 1.0);
        assert!(events[1].full_removal);
    }

    #[test]
    fn aggression_in_a_gap_is_never_associated() {
        let trade = aggression(1, 150, "100", "5", Side::Buy, Some(3));
        let mut clusters = cluster_aggressions([&trade], &[coverage(0, Some(100))], grouping(), 0);
        assert_eq!(clusters[0].generation, None);
        let mut events = liquidity_events(&[reduction(150, "5", "0")]);
        correlate_liquidity(&mut events, &mut clusters, 250);
        assert_eq!(events[0].matched_quantity, Decimal::ZERO);
        assert_eq!(events[0].evidence, LiquidityEvidence::DepthOnly);
    }

    #[test]
    fn nearby_out_of_order_aggression_matches_but_distant_one_does_not() {
        let nearby = aggression(1, 520, "100", "3", Side::Buy, Some(999));
        let distant = aggression(2, 900, "100", "9", Side::Buy, Some(3));
        let mut clusters =
            cluster_aggressions([&distant, &nearby], &[coverage(0, None)], grouping(), 0);
        let mut events = liquidity_events(&[reduction(500, "8", "4")]);
        correlate_liquidity(&mut events, &mut clusters, 25);
        assert_eq!(events[0].matched_quantity, dec("3"));
        assert_eq!(events[0].evidence, LiquidityEvidence::AggressionAligned);
        assert_eq!(
            clusters
                .iter()
                .map(|cluster| cluster.matched_quantity)
                .sum::<Decimal>(),
            dec("3")
        );
    }

    #[test]
    fn matching_never_double_counts_aggression_or_removed_quantity() {
        let trade = aggression(1, 150, "100", "5", Side::Buy, Some(3));
        let mut clusters = cluster_aggressions([&trade], &[coverage(0, None)], grouping(), 0);
        let mut events = liquidity_events(&[reduction(140, "10", "6"), reduction(160, "8", "4")]);
        correlate_liquidity(&mut events, &mut clusters, 50);

        let matched_events: Decimal = events.iter().map(|event| event.matched_quantity).sum();
        let matched_clusters: Decimal = clusters
            .iter()
            .map(|cluster| cluster.matched_quantity)
            .sum();
        assert_eq!(matched_events, dec("5"));
        assert_eq!(matched_clusters, dec("5"));
        assert!(
            events
                .iter()
                .all(|event| event.matched_quantity <= event.removed)
        );
        assert!(
            clusters
                .iter()
                .all(|cluster| cluster.matched_quantity <= cluster.quantity)
        );
    }
}
