//! Shared settled/live evidence allocation, visibility and marker placement.
use super::model::{LiquidityEventPrimitive, PriceWindow, event_cap_key};
use super::tiers::TierClusters;
use crate::config::HeatmapConfig;
use crate::grouping::EffectiveGrouping;
use crate::interaction::{LiquidityEvent, LiquidityEvidence, correlate_liquidity};
use crate::timeline::BarTimeline;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;

/// Allocate `events` to the prints of one half of the chart.
///
/// Each half is handed only the reductions timestamped inside it, so a removed
/// quantity is claimed as evidence exactly once however the chart is cut.
pub(super) fn correlate_tier(
    events: &mut [LiquidityEvent],
    tier: &mut TierClusters,
    config: &HeatmapConfig,
    summarizing: bool,
) {
    if summarizing {
        // The views overlap, so they are correlated apart: neither may allocate
        // the same reduction twice within itself, and a print drawn in both
        // places carries the same fact in both. The markers themselves keep the
        // slot pass's numbers, which cover every print of every bar.
        correlate_liquidity(
            &mut events.to_vec(),
            &mut tier.tape,
            config.liquidity_correlation_ms,
        );
        correlate_liquidity(events, &mut tier.slot, config.liquidity_correlation_ms);
    } else {
        // Disjoint: one pass over both, so a reduction is allocated across the
        // whole visible tape exactly as it always was. `correlate_liquidity`
        // mutates in place without reordering, so the halves split back apart.
        let tape_len = tier.tape.len();
        let mut both = std::mem::take(&mut tier.tape);
        both.append(&mut tier.slot);
        correlate_liquidity(events, &mut both, config.liquidity_correlation_ms);
        tier.slot = both.split_off(tape_len);
        tier.tape = both;
    }
}

/// Drop the reductions not worth a marker, and report how many the safety cap
/// took with them.
pub(super) fn filter_events(
    events: &mut Vec<LiquidityEvent>,
    config: &HeatmapConfig,
    liquidity_reference: Decimal,
) -> usize {
    // Display floors: a busy book shrinks buckets constantly, and a marker per
    // wiggle is violet drizzle. An unattributed pull must be deep (fraction of
    // its level, or a full pull) AND big (share of the visible full-intensity
    // reference) to draw. Aggression-aligned reductions are exempt from the
    // floors, and the safety cap below keeps them ahead of unattributed ones,
    // so a bubble can only point at a hidden event in the extreme case where
    // aligned events alone exceed the cap (reported via dropped counters).
    let pull_floor = Decimal::from_f32(config.min_unattributed_pull_share)
        .map(|share| liquidity_reference * share)
        .unwrap_or(Decimal::ZERO);
    events.retain(|event| {
        if matches!(event.evidence, LiquidityEvidence::AggressionAligned) {
            return true;
        }
        (event.full_removal || event.fraction >= config.min_unattributed_reduction)
            && event.removed >= pull_floor
    });

    // Per-layer display switches, applied after correlation so bubbles keep
    // their matched evidence (and their consumption marks) even when the
    // depletion markers themselves are hidden.
    events.retain(|event| match event.evidence {
        LiquidityEvidence::AggressionAligned => config.show_aligned_depletion,
        LiquidityEvidence::DepthOnly => config.show_unattributed_reductions,
    });

    let dropped = events.len().saturating_sub(config.max_visible_cells);
    if dropped > 0 {
        events.sort_by_key(|event| {
            event_cap_key(
                event.evidence,
                event.removed,
                event.timestamp_ms,
                event.event_id,
            )
        });
        events.truncate(config.max_visible_cells);
    }
    dropped
}

/// Place reductions on the chart.
pub(super) fn event_primitives(
    events: Vec<LiquidityEvent>,
    timeline: &BarTimeline,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
) -> Vec<LiquidityEventPrimitive> {
    events
        .into_iter()
        .filter_map(|event| {
            let x = timeline.locate(event.timestamp_ms)?.normalized;
            let clipped_low = event.price_bucket.max(prices.low);
            let clipped_high = (event.price_bucket + grouping.bucket_width).min(prices.high);
            let y0 = prices.y(clipped_high)?;
            let y1 = prices.y(clipped_low)?;
            (y1 > y0).then_some(LiquidityEventPrimitive {
                event_id: event.event_id,
                generation: event.generation,
                side: event.side,
                price_bucket: event.price_bucket,
                timestamp_ms: event.timestamp_ms,
                before: event.before,
                after: event.after,
                removed: event.removed,
                fraction: event.fraction,
                full_removal: event.full_removal,
                matched_quantity: event.matched_quantity,
                matched_fraction: event.matched_fraction,
                evidence: event.evidence,
                x,
                y0,
                y1,
            })
        })
        .collect()
}
