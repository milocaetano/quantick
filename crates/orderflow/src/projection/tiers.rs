//! One tier of the tape: the prints of one stretch of the chart, cut out of
//! the retained tape, clustered in the two views a print can be drawn in,
//! refined by the display floors and placed on the chart.
//!
//! The settled half and the live half are the same pipeline run over two
//! disjoint stretches, which is why the stretch is a value ([`TierCut`]) and
//! not a second copy of the code. The budget fold that follows lives in
//! `fold`; the reduction markers stay with the pipeline in the parent.

use rust_decimal::Decimal;

use super::model::{AggressionPrimitive, PriceWindow, normalized_area_size};
use crate::config::HeatmapConfig;
use crate::grouping::EffectiveGrouping;
use crate::history::{CoverageSegment, LiquidityHistory};
use crate::interaction::{
    AggressionCluster, cluster_aggressions, merge_dust_clusters, regionalize_clusters,
    summarize_clusters,
};
use crate::timeline::BarTimeline;

/// Where one tier is cut out of the retained tape.
///
/// The two questions travel together because they are the same decision seen
/// from both ends: which prints this tier owns at all, and which of the ones it
/// owns belong on the tape rather than in a bar slot.
#[derive(Debug, Clone, Copy)]
pub(super) struct TierCut {
    /// Half-open `[from, until)` in exchange milliseconds.
    pub(super) range: (Option<i64>, Option<i64>),
    // Where the tape begins, for the half that owns the tape — `None` for the
    // settled half, which owns none of it. Carried here rather than read off
    // the timeline because "am I on the tape?" is only a question for the live
    // half: the settled half asking it made its *content* depend on the lane's
    // left edge, and that edge moves with every print. The cached half would
    // have had to be rebuilt every frame to stay truthful, and the one that was
    // not rebuilt drew a print in a bar slot while the live half drew the same
    // print on the tape.
    pub(super) tape_from_ms: Option<i64>,
}

/// The price resolutions the two views cluster on.
///
/// One type rather than two loose arguments, because the pair *is* the rule:
/// the compressed slots fuse prints into whatever visual row the candles'
/// zoom resolved, and the tape never does.
#[derive(Debug, Clone, Copy)]
pub(super) struct TierGrouping {
    /// What the bar slots fuse prints into: the adaptive display grouping.
    pub(super) slots: EffectiveGrouping,
    /// What the tape fuses prints into: capture resolution, always.
    pub(super) lane: EffectiveGrouping,
}

/// The clustered prints of one stretch of the chart, in the two views a print
/// can be drawn in.
pub(super) struct TierClusters {
    /// Raw prints, placed by the live edge they are measured from.
    pub(super) tape: Vec<AggressionCluster>,
    /// Prints read against the bar they belong to.
    pub(super) slot: Vec<AggressionCluster>,
}

/// Cluster the retained prints timestamped inside `range`, as `[from, until)`.
///
/// The whole retained tape is walked, but the range is tested first and it is
/// two integer comparisons: the half that runs every frame pays the per-print
/// cost — locating it, placing it, clustering it — only for its own prints.
/// The walk is deliberately linear rather than a search inward from the newest
/// print: the retained tape is only *almost* ordered by timestamp, and one
/// print delivered out of order must not be able to hide every print behind
/// it.
pub(super) fn cluster_tier(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    coverage: &[CoverageSegment],
    grouping: TierGrouping,
    cut: TierCut,
    summarizing: bool,
) -> TierClusters {
    let config = history.config();
    let TierCut {
        range: (from_ms, until_ms),
        tape_from_ms,
    } = cut;
    let mut tape_prints = Vec::new();
    let mut slot_prints = Vec::new();
    for trade in history.aggressions() {
        if from_ms.is_some_and(|from| trade.timestamp_ms < from)
            || until_ms.is_some_and(|until| trade.timestamp_ms >= until)
        {
            continue;
        }
        if timeline.locate(trade.timestamp_ms).is_none() || prices.y(trade.price).is_none() {
            continue;
        }
        let on_tape = tape_from_ms.is_some_and(|start| trade.timestamp_ms >= start);
        if on_tape {
            tape_prints.push(trade);
        }
        // Exactly one pane draws a print, and which one is the tape's window:
        // while a print is inside it the tape has it, and when it falls out of
        // the window it lands in the slot of the bar it happened in. Widening
        // the tape therefore *moves* marks from the candles to the tape and
        // never deletes one — the alternative, drawing the print in both, puts
        // one execution on the canvas twice, which reads as two trades and is
        // the dishonesty this whole change exists to remove. The summary is the
        // one exception: a pie is an aggregate of the bar, not a second copy of
        // a print, so the bar keeps counting prints the tape is still showing.
        if summarizing || !on_tape {
            slot_prints.push(trade);
        }
    }

    // Each view clusters on its own window: the tape has room the compressed
    // slots do not, and the split is also what keeps a cluster from straddling
    // the boundary between them.
    TierClusters {
        tape: cluster_aggressions(
            tape_prints,
            coverage,
            grouping.lane,
            config.live_lane.effective_cluster_ms(
                config.bubble_cluster_ms,
                // No lane means no tape prints to cluster, so the reference
                // only has to be a number the scale can divide by.
                timeline.lane_reference_ms().unwrap_or(1),
            ),
        ),
        slot: cluster_aggressions(
            slot_prints,
            coverage,
            grouping.slots,
            config.bubble_cluster_ms,
        ),
    }
}

/// Apply the display floors, fold what is too small to read, and summarize.
pub(super) fn refine_tier(
    mut tier: TierClusters,
    config: &HeatmapConfig,
    print_reference: Decimal,
    timeline: &BarTimeline,
    grouping: EffectiveGrouping,
    summarizing: bool,
) -> (TierClusters, Decimal) {
    let regionalizing = config.bubble_region_rows > 1;
    // What the trader's own display floor takes off the canvas. Counted rather
    // than merely applied: it is the one discard left in this pipeline, it is a
    // setting the trader chose, and a setting that quietly removes contracts
    // without saying how many is the same dishonesty the budget just stopped
    // committing.
    let mut floored = Decimal::ZERO;

    // Display floor for bubbles. Applied after association so a hidden small
    // print still counts as the evidence behind an aligned reduction: the
    // marker keeps saying "a trade ate this", the tape just stays readable.
    // With the regional fold on, the slot's floor waits until after the fold:
    // a region's area claims the zone's summed volume, and a floor applied to
    // the members would silently thin the very sum the mark reports — small
    // prints fold into their region, and only still-small *regions* are
    // hidden.
    if let Some(floor) = config.bubbles.min_quantity_decimal() {
        tier.tape.retain(|cluster| {
            let kept = cluster.quantity >= floor;
            if !kept {
                floored += cluster.quantity;
            }
            kept
        });
        if !regionalizing {
            tier.slot.retain(|cluster| {
                let kept = cluster.quantity >= floor;
                if !kept {
                    floored += cluster.quantity;
                }
                kept
            });
        }
    }

    // Readability floor, then the closed-bar summary. Association already
    // happened, so folding moves no evidence: a merged bubble carries the
    // summed quantity and the union of the event ids its parts pointed at.
    // Sized against the reference computed above, which is deliberately *not*
    // recomputed — merging is a drawing decision and must not rescale the
    // bubbles that were already readable.
    if let Some(dust) = config.bubbles.dust_quantity(print_reference) {
        tier.tape = merge_dust_clusters(tier.tape, dust, config.bubble_dust_merge_ms);
        tier.slot = merge_dust_clusters(tier.slot, dust, config.bubble_dust_merge_ms);
    }

    // Which bar slot a cluster falls in — the key the regional fold and the
    // summary both group by, so a fold can never credit one bar with volume
    // its neighbour traded.
    let bar_of = |cluster: &AggressionCluster| {
        timeline
            .locate(cluster.timestamp_ms)
            .map(|position| position.bar_index)
    };

    // The regional fold, above one row: same-side clusters sharing a region
    // `bubble_region_rows` rows tall, inside one bar, become one bubble
    // anchored at the region's point of control. After association and after
    // the dust merge for the same reason those run where they do — folding is
    // a drawing decision, and by now it moves no evidence and rescales
    // nothing. The summary below then works at region granularity, because
    // the fold rewrote each cluster's bucket to its region's lower edge.
    //
    // Slots only, never the tape: the live lane draws prints one by one
    // because it has the room to, and a scalper reads the forming edge from
    // exactly that granularity — a region there would hold the newest print
    // hostage to a window that has not closed. The compressed history is
    // where per-row marks stack into bead necklaces, so the history is where
    // the fold pays.
    if regionalizing {
        let region_width = grouping.bucket_width * Decimal::from(config.bubble_region_rows);
        tier.slot = regionalize_clusters(tier.slot, region_width, config.bubble_region_ms, bar_of);
        if let Some(floor) = config.bubbles.min_quantity_decimal() {
            tier.slot.retain(|cluster| {
                let kept = cluster.quantity >= floor;
                if !kept {
                    floored += cluster.quantity;
                }
                kept
            });
        }
    }

    // Every bar with prints in its slot gets one mark per price range, the
    // forming one included: its pie is a running total that grows with each
    // order, which is how the compressed left side says what is happening now
    // rather than only what already happened. It is honest because it is
    // exactly what the bar has taken so far — and the tape beside it still
    // shows those same prints one by one.
    if summarizing {
        tier.slot = summarize_clusters(std::mem::take(&mut tier.slot), bar_of);
    }
    (tier, floored)
}

/// Place one tier's marks on the chart, each on the scale its view reads on.
pub(super) fn tier_primitives(
    marks: TierClusters,
    timeline: &BarTimeline,
    prices: PriceWindow,
    print_reference: Decimal,
    summary_reference: Decimal,
) -> Vec<AggressionPrimitive> {
    marks
        .tape
        .into_iter()
        .map(|cluster| (cluster, true, print_reference))
        .chain(
            marks
                .slot
                .into_iter()
                .map(|cluster| (cluster, false, summary_reference)),
        )
        .filter_map(|(cluster, live, reference)| {
            // A tape mark is placed by the live edge it is measured from; a
            // slot mark by the bar it belongs to. `locate` answers the first,
            // so a settled mark has to ask for its bar's slot explicitly.
            let position = if live {
                timeline.locate(cluster.timestamp_ms)?
            } else {
                timeline.locate_in_slot(cluster.timestamp_ms)?
            };
            let y = prices.y(cluster.price)?;
            let size = normalized_area_size(cluster.quantity, reference);
            Some(aggression_primitive(
                cluster,
                position.normalized,
                y,
                size,
                live,
            ))
        })
        .collect()
}

fn aggression_primitive(
    cluster: AggressionCluster,
    x: f64,
    y: f64,
    size: f32,
    live: bool,
) -> AggressionPrimitive {
    let matched_fraction = cluster.matched_fraction();
    let buy_share = cluster.buy_share();
    AggressionPrimitive {
        agg_id: cluster.agg_id,
        agg_ids: cluster.agg_ids,
        generation: cluster.generation,
        side: cluster.side,
        consumed_side: cluster.consumed_side,
        quantity: cluster.quantity,
        buy_share,
        live,
        price_bucket: cluster.price_bucket,
        price_span: cluster.price_span,
        trade_count: cluster.trade_count,
        first_timestamp_ms: cluster.first_timestamp_ms,
        last_timestamp_ms: cluster.last_timestamp_ms,
        matched_quantity: cluster.matched_quantity,
        matched_fraction,
        liquidity_event_ids: cluster.liquidity_event_ids,
        x,
        y,
        size,
        folded_marks: 0,
    }
}
