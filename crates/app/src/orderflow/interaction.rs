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
    /// The share of [`quantity`](Self::quantity) taken by buyers; the rest was
    /// taken by sellers.
    ///
    /// A cluster built by [`cluster_aggressions`] has one side, so this is
    /// either all of the quantity or none of it. It becomes interesting after
    /// [`summarize_clusters`], which is the one place both sides land in the
    /// same mark.
    pub buy_quantity: Decimal,
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

    /// `[0,1]` share of this bubble's quantity that buyers took.
    ///
    /// Exactly `1.0` or `0.0` on a single-sided bubble, which is what lets the
    /// renderer tell a pie from a plain disc without a second flag.
    #[must_use]
    pub fn buy_share(&self) -> f32 {
        decimal_fraction(self.buy_quantity, self.quantity)
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
        let buy_quantity = match self.side {
            AggressorSide::Buy => self.quantity,
            AggressorSide::Sell => Decimal::ZERO,
        };
        AggressionCluster {
            agg_id: self.agg_ids[0],
            agg_ids: self.agg_ids,
            generation: self.key.generation,
            side: self.side,
            consumed_side: consumed_side(self.side),
            price_bucket: self.key.price_bucket,
            quantity: self.quantity,
            buy_quantity,
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

/// Accumulator folding several clusters into one bubble.
///
/// Both folds the projection performs — the dust merge below and the closed-bar
/// summary — sum the same things in the same way: exact quantity, the buy share
/// of it, matched evidence, ids and the time span. Only the grouping key and
/// the admission rule differ, so those stay with the callers and the summing
/// lives here once.
#[derive(Debug)]
struct ClusterFold {
    cluster: AggressionCluster,
    price_quantity: Decimal,
    count: usize,
}

impl ClusterFold {
    fn new(cluster: AggressionCluster) -> ClusterFold {
        let price_quantity = cluster.price * cluster.quantity;
        ClusterFold {
            cluster,
            price_quantity,
            count: 1,
        }
    }

    /// The cluster this fold is anchored at, before anything was folded in.
    fn anchor(&self) -> &AggressionCluster {
        &self.cluster
    }

    fn push(&mut self, other: AggressionCluster) {
        self.price_quantity += other.price * other.quantity;
        self.cluster.quantity += other.quantity;
        self.cluster.buy_quantity += other.buy_quantity;
        self.cluster.matched_quantity += other.matched_quantity;
        self.cluster.agg_ids.extend(other.agg_ids);
        self.cluster
            .liquidity_event_ids
            .extend(other.liquidity_event_ids);
        self.cluster.first_timestamp_ms = self
            .cluster
            .first_timestamp_ms
            .min(other.first_timestamp_ms);
        self.cluster.last_timestamp_ms =
            self.cluster.last_timestamp_ms.max(other.last_timestamp_ms);
        self.count += 1;
    }

    fn finish(mut self) -> AggressionCluster {
        // A lone cluster is returned exactly as it arrived: merging must be
        // invisible where there was nothing to merge.
        if self.count == 1 {
            return self.cluster;
        }
        if self.cluster.quantity > Decimal::ZERO {
            self.cluster.price = self.price_quantity / self.cluster.quantity;
        }
        // The side a mixed bubble reports is the one that took more. A tie
        // keeps the anchor's side, which is deterministic because the caller
        // sorted the input. Single-sided folds are unaffected: every member
        // contributed to the same total, so the winner is the side they share.
        let sell_quantity = self.cluster.quantity - self.cluster.buy_quantity;
        if self.cluster.buy_quantity > sell_quantity {
            self.cluster.side = AggressorSide::Buy;
        } else if sell_quantity > self.cluster.buy_quantity {
            self.cluster.side = AggressorSide::Sell;
        }
        self.cluster.consumed_side = consumed_side(self.cluster.side);
        self.cluster.timestamp_ms = self.cluster.first_timestamp_ms.saturating_add(
            self.cluster
                .last_timestamp_ms
                .saturating_sub(self.cluster.first_timestamp_ms)
                / 2,
        );
        self.cluster.agg_ids.sort_unstable();
        self.cluster.agg_ids.dedup();
        self.cluster.liquidity_event_ids.sort_unstable();
        self.cluster.liquidity_event_ids.dedup();
        self.cluster.trade_count = self.cluster.agg_ids.len();
        if let Some(first) = self.cluster.agg_ids.first() {
            self.cluster.agg_id = *first;
        }
        self.cluster
    }
}

/// Deterministic order every clustering and folding step emits its result in.
///
/// Also what the projection restores after clustering the live lane and the
/// history behind it separately: two sorted halves concatenated are not sorted,
/// and iteration order must never leak into what the chart draws.
pub fn sort_clusters(clusters: &mut [AggressionCluster]) {
    clusters.sort_by(|a, b| {
        a.first_timestamp_ms
            .cmp(&b.first_timestamp_ms)
            .then_with(|| a.last_timestamp_ms.cmp(&b.last_timestamp_ms))
            .then_with(|| aggressor_side_key(a.side).cmp(&aggressor_side_key(b.side)))
            .then_with(|| a.price_bucket.cmp(&b.price_bucket))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });
}

/// Fold prints too small to draw as anything but a dot into one bubble per
/// visual price range.
///
/// The bulk of a busy tape is dust: clusters whose quantity puts them at the
/// minimum radius, where neither the fill colour nor the side nudge survives.
/// Drawn one by one they are a rash of identical specks that says only "trades
/// happened here"; folded together they become a single mark carrying the same
/// exact quantity, which is the part worth reading.
///
/// A merge is anchored at its first cluster and never spans more than
/// `window_ms`, so a quiet price range cannot pull unrelated prints together
/// across the whole visible history, and a cluster above `dust_quantity` both
/// passes through untouched and closes any open merge — a merged bubble is
/// always contiguous in time. Quantities, ids and matched evidence are summed:
/// nothing is dropped, and `trade_count` still reports every aggregate trade
/// standing behind the mark.
#[must_use]
pub fn merge_dust_clusters(
    clusters: Vec<AggressionCluster>,
    dust_quantity: Decimal,
    window_ms: i64,
) -> Vec<AggressionCluster> {
    if dust_quantity <= Decimal::ZERO || window_ms <= 0 {
        return clusters;
    }

    let mut keyed: Vec<(ClusterKey, AggressionCluster)> = clusters
        .into_iter()
        .map(|cluster| {
            let key = ClusterKey {
                generation: cluster.generation,
                side: aggressor_side_key(cluster.side),
                price_bucket: cluster.price_bucket,
            };
            (key, cluster)
        })
        .collect();
    keyed.sort_by(|(a_key, a), (b_key, b)| {
        a_key
            .cmp(b_key)
            .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });

    let mut merged: Vec<AggressionCluster> = Vec::with_capacity(keyed.len());
    let mut open: Option<(ClusterKey, ClusterFold)> = None;
    for (key, cluster) in keyed {
        if cluster.quantity >= dust_quantity {
            if let Some((_, pending)) = open.take() {
                merged.push(pending.finish());
            }
            merged.push(cluster);
            continue;
        }
        let accepts = open.as_ref().is_some_and(|(open_key, pending)| {
            *open_key == key
                && cluster
                    .last_timestamp_ms
                    .saturating_sub(pending.anchor().first_timestamp_ms)
                    <= window_ms
        });
        match open.as_mut() {
            Some((_, pending)) if accepts => pending.push(cluster),
            _ => {
                if let Some((_, pending)) = open.replace((key, ClusterFold::new(cluster))) {
                    merged.push(pending.finish());
                }
            }
        }
    }
    if let Some((_, pending)) = open {
        merged.push(pending.finish());
    }

    sort_clusters(&mut merged);
    merged
}

/// Fold every print a closed bar left in one visual price range into a single
/// bubble carrying both sides.
///
/// The live lane draws prints one by one because it has the room to; a closed
/// bar owns one slot, and the same prints compressed into it stack until buy
/// and sell hide each other. The summary trades that pile for one honest mark
/// per bar and price range: exact summed quantity, the buy share preserved in
/// [`AggressionCluster::buy_quantity`] so the renderer can show the proportion,
/// and every id and matched event still attached. Nothing is dropped and
/// `trade_count` still reports every aggregate trade behind the mark.
///
/// `bar_of` reports which bar slot a cluster falls in; clusters it declines to
/// place — the live lane's, which have not finished happening — pass through
/// untouched. Coverage generation stays in the key, so a summary never spans a
/// break in the recording.
#[must_use]
pub fn summarize_clusters(
    clusters: Vec<AggressionCluster>,
    bar_of: impl Fn(&AggressionCluster) -> Option<usize>,
) -> Vec<AggressionCluster> {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    struct SummaryKey {
        bar_index: usize,
        generation: Option<u64>,
        price_bucket: Decimal,
    }

    let mut passthrough: Vec<AggressionCluster> = Vec::new();
    let mut keyed: Vec<(SummaryKey, AggressionCluster)> = Vec::with_capacity(clusters.len());
    for cluster in clusters {
        match bar_of(&cluster) {
            Some(bar_index) => keyed.push((
                SummaryKey {
                    bar_index,
                    generation: cluster.generation,
                    price_bucket: cluster.price_bucket,
                },
                cluster,
            )),
            None => passthrough.push(cluster),
        }
    }
    keyed.sort_by(|(a_key, a), (b_key, b)| {
        a_key
            .cmp(b_key)
            .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
            .then_with(|| aggressor_side_key(a.side).cmp(&aggressor_side_key(b.side)))
            .then_with(|| a.agg_id.cmp(&b.agg_id))
    });

    let mut merged = passthrough;
    merged.reserve(keyed.len());
    let mut open: Option<(SummaryKey, ClusterFold)> = None;
    for (key, cluster) in keyed {
        match open.as_mut() {
            Some((open_key, pending)) if *open_key == key => pending.push(cluster),
            _ => {
                if let Some((_, pending)) = open.replace((key, ClusterFold::new(cluster))) {
                    merged.push(pending.finish());
                }
            }
        }
    }
    if let Some((_, pending)) = open {
        merged.push(pending.finish());
    }

    sort_clusters(&mut merged);
    merged
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

    /// One raw cluster per print, which is what the dust merge is handed once
    /// the temporal clustering has already run.
    fn raw_clusters(prints: &[Aggression]) -> Vec<AggressionCluster> {
        cluster_aggressions(prints.iter(), &[coverage(0, None)], grouping(), 0)
    }

    #[test]
    fn dust_folds_into_one_bubble_and_conserves_quantity_ids_and_span() {
        let prints = [
            aggression(1, 100, "100", "1", Side::Buy, Some(3)),
            aggression(2, 300, "101", "2", Side::Buy, Some(3)),
            aggression(3, 500, "100", "1", Side::Buy, Some(3)),
        ];
        let clusters = raw_clusters(&prints);
        assert_eq!(clusters.len(), 3);

        let merged = merge_dust_clusters(clusters, dec("10"), 1_000);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].quantity, dec("4"));
        assert_eq!(merged[0].agg_ids, [1, 2, 3]);
        assert_eq!(merged[0].trade_count, 3);
        assert_eq!(merged[0].first_timestamp_ms, 100);
        assert_eq!(merged[0].last_timestamp_ms, 500);
        assert_eq!(merged[0].timestamp_ms, 300);
        // Quantity-weighted: (100·1 + 101·2 + 100·1) / 4.
        assert_eq!(merged[0].price, dec("100.5"));
    }

    #[test]
    fn a_dust_merge_never_spans_more_than_its_window() {
        let prints = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 200, "100", "1", Side::Buy, Some(3)),
            aggression(3, 900, "100", "1", Side::Buy, Some(3)),
        ];
        let merged = merge_dust_clusters(raw_clusters(&prints), dec("10"), 500);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].agg_ids, [1, 2]);
        assert_eq!(merged[1].agg_ids, [3]);
    }

    #[test]
    fn a_readable_print_passes_through_untouched_and_splits_the_merge() {
        let prints = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 100, "100", "50", Side::Buy, Some(3)),
            aggression(3, 200, "100", "1", Side::Buy, Some(3)),
        ];
        let merged = merge_dust_clusters(raw_clusters(&prints), dec("10"), 5_000);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[1].agg_ids, [2]);
        assert_eq!(merged[1].quantity, dec("50"));
    }

    #[test]
    fn dust_never_merges_across_side_or_price_range() {
        let prints = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 10, "100", "1", Side::Sell, Some(3)),
            aggression(3, 20, "110", "1", Side::Buy, Some(3)),
        ];
        let merged = merge_dust_clusters(raw_clusters(&prints), dec("10"), 5_000);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn a_zero_window_or_threshold_draws_every_print() {
        let prints = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 10, "100", "1", Side::Buy, Some(3)),
        ];
        let clusters = raw_clusters(&prints);
        assert_eq!(merge_dust_clusters(clusters.clone(), dec("10"), 0).len(), 2);
        assert_eq!(merge_dust_clusters(clusters, Decimal::ZERO, 5_000).len(), 2);
    }

    /// The closed-bar summary is the one fold that mixes the two sides, so it
    /// is the one that has to keep the arithmetic honest: nothing dropped, the
    /// proportion preserved, and the reported side the one that took more.
    #[test]
    fn a_summary_merges_both_sides_and_keeps_the_proportion() {
        let prints = [
            aggression(1, 0, "100", "3", Side::Buy, Some(3)),
            aggression(2, 40, "100", "1", Side::Sell, Some(3)),
            aggression(3, 80, "100", "2", Side::Buy, Some(3)),
        ];
        let clusters = raw_clusters(&prints);
        assert_eq!(clusters.len(), 3, "the tape starts as three prints");

        let summarized = summarize_clusters(clusters, |_| Some(7));
        assert_eq!(summarized.len(), 1);
        let bubble = &summarized[0];
        assert_eq!(bubble.quantity, dec("6"));
        assert_eq!(bubble.buy_quantity, dec("5"));
        assert!((bubble.buy_share() - 5.0 / 6.0).abs() < 1e-6);
        // Buyers took more, so the mark reports their side and the passive
        // side it consumed follows from that.
        assert_eq!(bubble.side, Side::Buy);
        assert_eq!(bubble.consumed_side, BookSide::Ask);
        assert_eq!(bubble.agg_ids, vec![1, 2, 3]);
        assert_eq!(bubble.trade_count, 3);
        assert_eq!(bubble.first_timestamp_ms, 0);
        assert_eq!(bubble.last_timestamp_ms, 80);
        assert_eq!(bubble.timestamp_ms, 40);

        // Reverse the sides and the reported side reverses with them.
        let mirrored = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 40, "100", "4", Side::Sell, Some(3)),
        ];
        let mirrored = summarize_clusters(raw_clusters(&mirrored), |_| Some(7));
        assert_eq!(mirrored.len(), 1);
        assert_eq!(mirrored[0].side, Side::Sell);
        assert_eq!(mirrored[0].consumed_side, BookSide::Bid);
        assert!((mirrored[0].buy_share() - 0.2).abs() < 1e-6);
    }

    #[test]
    fn a_summary_never_crosses_a_bar_a_price_range_or_a_coverage_break() {
        let prints = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            // Same bar and range, other side: this one joins.
            aggression(2, 10, "100", "1", Side::Sell, Some(3)),
            // Another visual range.
            aggression(3, 20, "108", "1", Side::Buy, Some(3)),
        ];
        let clusters = raw_clusters(&prints);
        // Bar index derived from the timestamp: the first two share a bar.
        let summarized = summarize_clusters(clusters.clone(), |cluster| {
            Some(usize::from(cluster.first_timestamp_ms >= 15))
        });
        assert_eq!(summarized.len(), 3 - 1, "only the shared bar+range merges");

        // Clusters the caller declines to place are the live lane's: they pass
        // through exactly as they arrived.
        let untouched = summarize_clusters(clusters.clone(), |_| None);
        assert_eq!(untouched, clusters);

        // A coverage break splits the summary even inside one bar and range.
        let across_break = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 10, "100", "1", Side::Sell, Some(3)),
        ];
        let split = summarize_clusters(
            cluster_aggressions(
                &across_break,
                &[
                    coverage(0, Some(5)),
                    CoverageSegment {
                        generation: 4,
                        start_ms: 5,
                        end_ms: None,
                    },
                ],
                grouping(),
                0,
            ),
            |_| Some(0),
        );
        assert_eq!(split.len(), 2, "a summary must not span a recording break");
    }

    /// Every cluster leaves the builder single-sided, which is what lets the
    /// renderer read `buy_share` as "pie or plain disc" with no second flag.
    #[test]
    fn a_plain_cluster_is_all_of_one_side() {
        let prints = [
            aggression(1, 0, "100", "2", Side::Buy, Some(3)),
            aggression(2, 10, "100", "3", Side::Sell, Some(3)),
        ];
        for cluster in raw_clusters(&prints) {
            match cluster.side {
                Side::Buy => {
                    assert_eq!(cluster.buy_quantity, cluster.quantity);
                    assert_eq!(cluster.buy_share(), 1.0);
                }
                Side::Sell => {
                    assert_eq!(cluster.buy_quantity, Decimal::ZERO);
                    assert_eq!(cluster.buy_share(), 0.0);
                }
            }
        }
    }

    #[test]
    fn merging_dust_carries_its_matched_evidence_along() {
        let prints = [
            aggression(1, 0, "100", "1", Side::Buy, Some(3)),
            aggression(2, 100, "100", "1", Side::Buy, Some(3)),
        ];
        let mut clusters = raw_clusters(&prints);
        clusters[0].matched_quantity = dec("0.5");
        clusters[0].liquidity_event_ids = vec![7];
        clusters[1].matched_quantity = dec("1");
        clusters[1].liquidity_event_ids = vec![7, 9];

        let merged = merge_dust_clusters(clusters, dec("10"), 5_000);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].matched_quantity, dec("1.5"));
        assert_eq!(merged[0].liquidity_event_ids, [7, 9]);
        assert_eq!(merged[0].matched_fraction(), 0.75);
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
