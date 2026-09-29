//! Bounded facts bridging ingestion to an asynchronous tape publication.
use super::dots::{DotHorizon, fold_dots, native_grouping, window_start};
use super::tiers::{TierClusters, native_tape_primitives, tier_primitives};
use super::{
    AggressionPrimitive, DOT_WINDOW_LADDER_MS, HeatmapProjection, PriceWindow, TapeFacts,
    VolumeDots, dot_full_quantity,
};
use crate::HeatmapConfig;
use crate::history::{Aggression, RecordedOpenings};
use crate::interaction::{AggressionCluster, cluster_aggressions, sort_clusters};
use crate::timeline::BarTimeline;
use quantick_engine::Trade;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

/// A view owns this suffix; a frame-bound receipt retires its raw facts.
/// Retained id/time metadata keeps capacity eviction identical after receipt.
#[derive(Default)]
pub struct PendingTape {
    epoch: u64,
    next_ordinal: u64,
    trades: VecDeque<(u64, Trade)>,
    retained: VecDeque<(u64, i64, i64)>,
    newest_ms: Option<i64>,
    evicted_through_ms: Option<i64>,
    opening_bursts: RecordedOpenings,
}

impl PendingTape {
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn len(&self) -> usize {
        self.trades.len()
    }
    pub fn is_empty(&self) -> bool {
        self.trades.is_empty()
    }
    pub fn latest_ms(&self) -> Option<i64> {
        self.newest_ms
    }
    pub fn started(&self) -> bool {
        self.next_ordinal != 0
    }

    /// A new worker/display epoch invalidates receipts, retaining the source's
    /// opening metadata. Source resets additionally call `clear_opening_bursts`.
    pub fn reset(&mut self) -> u64 {
        let epoch = self.epoch.wrapping_add(1);
        let opening_bursts = std::mem::take(&mut self.opening_bursts);
        *self = Self {
            epoch,
            opening_bursts,
            ..Self::default()
        };
        epoch
    }

    pub fn record(&mut self, trade: &Trade, config: &HeatmapConfig) -> u64 {
        self.observe_opening_burst(trade.timestamp_ms);
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        let ordinal = self.next_ordinal;
        let newest = self
            .newest_ms
            .map_or(trade.timestamp_ms, |time| time.max(trade.timestamp_ms));
        self.newest_ms = Some(newest);
        self.trades.push_back((ordinal, trade.clone()));
        self.retained
            .push_back((ordinal, trade.timestamp_ms, newest));
        let cutoff = newest.saturating_sub(config.retention_ms);
        while self.retained.len() > config.max_aggressions
            || self
                .retained
                .front()
                .is_some_and(|record| record.2 < cutoff)
        {
            let (id, time, _) = self
                .retained
                .pop_front()
                .expect("the retained prefix exists");
            self.evicted_through_ms =
                Some(self.evicted_through_ms.map_or(time, |old| old.max(time)));
            self.acknowledge(id);
        }
        ordinal
    }

    /// A small mirror also observes ordinary-mode admissions, so enabling a
    /// tape mid-session cannot mistake the newest visible print for its opening.
    pub fn observe_opening_burst(&mut self, timestamp_ms: i64) {
        self.opening_bursts.observe(timestamp_ms);
    }

    /// Source/session resets clear facts; display and worker epochs do not.
    pub fn clear_opening_bursts(&mut self) {
        self.opening_bursts = RecordedOpenings::default();
    }

    pub fn opening_bursts(&self, published: Option<&HeatmapProjection>) -> Vec<i64> {
        let mut openings = self.opening_bursts.clone();
        if let Some(facts) = published.and_then(|frame| frame.tape_facts.as_deref()) {
            openings.merge(&facts.opening_bursts);
        }
        openings.windows().to_vec()
    }

    /// Called only with a receipt belonging to the exact adopted frame/epoch.
    pub fn acknowledge(&mut self, through_id: u64) {
        while self
            .trades
            .front()
            .is_some_and(|(ordinal, _)| *ordinal <= through_id)
        {
            self.trades.pop_front();
        }
    }

    pub fn price_range(
        &self,
        published: Option<&HeatmapProjection>,
        now_ms: Option<i64>,
        window_ms: i64,
    ) -> Option<(f64, f64)> {
        let from = now_ms
            .or(self.newest_ms)
            .unwrap_or(i64::MAX)
            .saturating_sub(window_ms);
        let facts = published.and_then(|frame| frame.tape_facts.as_deref());
        let evicted = self.eviction_horizon(facts);
        let recent = |time| {
            let start = window_start(time, DOT_WINDOW_LADDER_MS[0]);
            start >= from && evicted.is_none_or(|horizon| start > horizon)
        };
        let native = facts
            .into_iter()
            .flat_map(|facts| &facts.clusters)
            .filter(|cluster| recent(cluster.first_timestamp_ms))
            .map(|cluster| cluster.price);
        let fallback = published
            .filter(|_| facts.is_none())
            .into_iter()
            .flat_map(|frame| &frame.aggressions)
            .filter(|mark| mark.live && recent(mark.first_timestamp_ms))
            .map(|mark| mark.price);
        native
            .chain(fallback)
            .chain(
                self.trades
                    .iter()
                    .map(|(_, trade)| trade)
                    .filter(|trade| recent(trade.timestamp_ms))
                    .map(|trade| trade.price),
            )
            .filter_map(|price| price.to_f64())
            .fold(None, |range, price| {
                Some(range.map_or((price, price), |(low, high): (f64, f64)| {
                    (low.min(price), high.max(price))
                }))
            })
    }

    fn eviction_horizon(&self, facts: Option<&TapeFacts>) -> Option<i64> {
        self.evicted_through_ms
            .max(facts.and_then(|facts| facts.evicted_through_ms))
    }

    /// Complete the native tape keys through the UI's accepted suffix using
    /// the same fold and placement functions as the authoritative worker.
    pub fn project(
        &self,
        published: Option<&HeatmapProjection>,
        config: &HeatmapConfig,
        timeline: &BarTimeline,
        prices: PriceWindow,
        dots: &VolumeDots,
    ) -> HeatmapProjection {
        let native = native_grouping(config);
        let from = timeline.lane_start_ms().unwrap_or(i64::MIN);
        let raw: Vec<_> = self
            .trades
            .iter()
            .filter(|(_, trade)| window_start(trade.timestamp_ms, dots.tape_window_ms) >= from)
            .map(|(_, trade)| Aggression {
                agg_id: trade.agg_id,
                timestamp_ms: trade.timestamp_ms,
                price: trade.price,
                quantity: trade.quantity,
                side: trade.side,
                generation: None,
            })
            .collect();
        let facts = published.and_then(|frame| frame.tape_facts.as_deref());
        let evicted_through_ms = self.eviction_horizon(facts);
        // The worker owns validated book coverage. Pending tape geometry does
        // not wait for it and never guesses the generation of a queued print.
        let mut clusters = cluster_aggressions(&raw, &[], native, 0);
        let mut unchanged = Vec::new();
        if let Some(facts) = facts {
            // A published native cell is already a canonical fold. Only keys
            // touched by this suffix need to repeat that work. The width check
            // leaves a grouping transition on the complete fold path.
            let native_cells = dots.native_tape
                && dots.tape_window_ms == DOT_WINDOW_LADDER_MS[0]
                && dots.tape_level_ticks == 1
                && facts
                    .clusters
                    .iter()
                    .all(|cluster| cluster.price_span == native.bucket_width);
            if native_cells {
                let touched: BTreeSet<_> = clusters
                    .iter()
                    .map(|cluster| {
                        (
                            window_start(cluster.timestamp_ms, dots.tape_window_ms),
                            (cluster.price / native.bucket_width).floor() * native.bucket_width,
                        )
                    })
                    .collect();
                for cluster in &facts.clusters {
                    let start = window_start(cluster.first_timestamp_ms, dots.tape_window_ms);
                    if start < from || evicted_through_ms.is_some_and(|horizon| start <= horizon) {
                        continue;
                    }
                    if touched.contains(&(start, cluster.price_bucket)) {
                        clusters.push(cluster.clone());
                    } else {
                        unchanged.push(cluster.clone());
                    }
                }
            } else {
                clusters.extend(facts.clusters.iter().cloned());
            }
        } else if let Some(frame) = published {
            clusters.extend(
                frame
                    .aggressions
                    .iter()
                    .filter(|mark| mark.live)
                    .map(as_cluster),
            );
        }
        clusters.retain(|cluster| {
            window_start(cluster.first_timestamp_ms, dots.tape_window_ms) >= from
        });
        let mut folded = fold_dots(
            clusters,
            true,
            dots,
            native,
            DotHorizon {
                recorded_from_ms: None,
                evicted_through_ms,
            },
        );
        if !unchanged.is_empty() {
            folded.extend(unchanged);
            sort_clusters(&mut folded);
        }
        let floor = config.bubbles.min_quantity_decimal().unwrap_or_default();
        let floored_quantity: Decimal = folded
            .iter()
            .filter(|cluster| cluster.quantity < floor)
            .map(|cluster| cluster.quantity)
            .sum();
        let tape_facts = Arc::new(TapeFacts {
            clusters: folded,
            evicted_through_ms,
            floored_quantity,
            opening_bursts: self.opening_bursts(published),
        });
        let reference = dot_full_quantity(config);
        let mut projection = published
            .map(without_live_aggressions)
            .unwrap_or_else(|| HeatmapProjection::empty(config.enabled, native));
        projection.floored_quantity +=
            floored_quantity - facts.map_or(Decimal::ZERO, |facts| facts.floored_quantity);
        let visible = tape_facts
            .clusters
            .iter()
            .filter(|cluster| cluster.quantity >= floor)
            .cloned();
        let primitives = if dots.native_tape {
            native_tape_primitives(visible, timeline, prices, reference, dots)
        } else {
            tier_primitives(
                TierClusters {
                    tape: visible.collect(),
                    slot: Vec::new(),
                    tape_facts: None,
                },
                timeline,
                prices,
                reference,
                reference,
                Some(dots),
            )
        };
        projection.tape_facts = Some(tape_facts);
        if projection.aggressions.is_empty() {
            projection.aggressions = primitives;
        } else {
            projection.aggressions.extend(primitives);
        }
        projection
            .aggressions
            .sort_by_cached_key(|mark| mark.quantity);
        projection.volume_dots = true;
        projection.live_now_x = timeline
            .live_now_position()
            .map(|position| position.normalized);
        projection
    }
}

/// The live primitives are about to be replaced; copying their execution-id
/// vectors only to discard them makes each accepted suffix pay for history.
fn without_live_aggressions(frame: &HeatmapProjection) -> HeatmapProjection {
    HeatmapProjection {
        enabled: frame.enabled,
        floored_quantity: frame.floored_quantity,
        summarized: frame.summarized,
        cells: Arc::clone(&frame.cells),
        aggressions: frame
            .aggressions
            .iter()
            .filter(|mark| !mark.live)
            .cloned()
            .collect(),
        tape_facts: frame.tape_facts.clone(),
        volume_dots: frame.volume_dots,
        liquidity_events: frame.liquidity_events.clone(),
        gaps: Arc::clone(&frame.gaps),
        live_now_x: frame.live_now_x,
        effective_grouping: frame.effective_grouping,
        liquidity_reference: frame.liquidity_reference,
        aggression_reference: frame.aggression_reference,
        summary_reference: frame.summary_reference,
        dropped_cells: frame.dropped_cells,
        folded_aggressions: frame.folded_aggressions,
        dropped_liquidity_events: frame.dropped_liquidity_events,
    }
}

fn as_cluster(mark: &AggressionPrimitive) -> AggressionCluster {
    AggressionCluster {
        agg_id: mark.agg_id,
        agg_ids: mark.agg_ids.clone(),
        generation: mark.generation,
        side: mark.side,
        consumed_side: mark.consumed_side,
        price_bucket: mark.price_bucket,
        price_span: mark.price_span,
        quantity: mark.quantity,
        buy_quantity: mark.buy_quantity,
        price: mark.price,
        timestamp_ms: mark.first_timestamp_ms,
        timestamp_quantity: mark.timestamp_quantity,
        first_timestamp_ms: mark.first_timestamp_ms,
        last_timestamp_ms: mark.last_timestamp_ms,
        trade_count: mark.trade_count,
        matched_quantity: mark.matched_quantity,
        liquidity_event_ids: mark.liquidity_event_ids.clone(),
    }
}
