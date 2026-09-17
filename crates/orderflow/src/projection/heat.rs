//! Run geometry, weighted bar summaries and the heat reference/cap policy.
use super::model::{HeatmapCell, PriceWindow, normalized_log_intensity};
use crate::config::{HeatmapConfig, IntensityMode};
use crate::grouping::{EffectiveGrouping, VisualLiquidityRun};
use crate::history::RestingSide;
use crate::timeline::BarTimeline;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

#[derive(Debug)]
struct DraftCell {
    generation: u64,
    side: RestingSide,
    price_bucket: Decimal,
    quantity: Decimal,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

/// One level's resting liquidity, summed over one bar and weighted by time.
///
/// The accumulator behind a bar slot's summary band: `weighted / span_ms` is
/// the quantity that was typically resting at that price while the bar ran.
struct SlotHeat {
    generation: u64,
    /// Σ quantity × milliseconds it was displayed for, inside this bar.
    weighted: Decimal,
    /// The bar's own duration, which the sum is averaged over — so a level
    /// present for part of the bar reads proportionally fainter.
    span_ms: i64,
    y0: f64,
    y1: f64,
}

/// Geometry before normalization: one reference sample per drawn run.
pub(super) struct HeatDrafts {
    drafts: Vec<DraftCell>,
    run_quantities: Vec<Decimal>,
}

pub(super) struct ProjectedHeat {
    pub(super) cells: Vec<HeatmapCell>,
    pub(super) liquidity_reference: Decimal,
    pub(super) dropped_cells: usize,
}

pub(super) fn project_geometry(
    runs: &[VisualLiquidityRun],
    timeline: &BarTimeline,
    prices: PriceWindow,
    effective_grouping: EffectiveGrouping,
) -> HeatDrafts {
    // Where the book is drawn, and in what form.
    //
    // A bar is timeless: its slot is a fixed width whatever market time it took,
    // so drawing sub-bar runs inside it invents a clock the bar does not have —
    // and once the tape covers the present, asking only for the newest place a
    // run belongs leaves the candles' half of the chart frozen a window in the
    // past. With a tape on screen the two halves split the job the way the
    // prints already do: the tape keeps the runs themselves, second by second,
    // and each bar slot carries *one summary band per level* — the quantity
    // that was typically resting there while the bar ran, each run weighted by
    // how much of the bar it covered. A wall that stood the whole bar reads at
    // its own size; one that flickered for a tenth of it reads a tenth as
    // bright. Both panes read on the same scale, so a stable wall looks the
    // same on either side of the divider.
    //
    // Without a tape there is nothing to carry the detail, so the chart keeps
    // drawing every run where it happened, exactly as it always has.
    let mut drafts = Vec::new();
    // One entry per run, so the intensity reference measures the book itself
    // rather than how many views a level happens to be drawn in.
    let mut run_quantities = Vec::with_capacity(runs.len());
    let mut summary: BTreeMap<(usize, Decimal, RestingSide), SlotHeat> = BTreeMap::new();
    let lane_view = timeline.lane_start_ms().is_some();
    for run in runs {
        let bucket_low = run.price_bucket;
        let bucket_high = bucket_low + effective_grouping.bucket_width;
        let clipped_low = bucket_low.max(prices.low);
        let clipped_high = bucket_high.min(prices.high);
        let Some(y0) = prices.y(clipped_high) else {
            continue;
        };
        let Some(y1) = prices.y(clipped_low) else {
            continue;
        };
        if y1 <= y0 {
            continue;
        }
        let draft = |x0: f64, x1: f64, quantity: Decimal| DraftCell {
            generation: run.generation,
            side: run.side,
            price_bucket: run.price_bucket,
            quantity,
            x0,
            x1,
            y0,
            y1,
        };

        let mut drawn = false;
        if lane_view {
            for span in timeline.slots_between(run.start_ms, run.end_ms) {
                let overlap = run
                    .end_ms
                    .min(span.end_ms)
                    .saturating_sub(run.start_ms.max(span.start_ms));
                if overlap <= 0 {
                    continue;
                }
                let entry = summary
                    .entry((span.index, run.price_bucket, run.side))
                    .or_insert(SlotHeat {
                        generation: run.generation,
                        weighted: Decimal::ZERO,
                        span_ms: (span.end_ms - span.start_ms).max(1),
                        y0,
                        y1,
                    });
                entry.weighted = entry
                    .weighted
                    .saturating_add(run.quantity.saturating_mul(Decimal::from(overlap)));
                entry.generation = entry.generation.max(run.generation);
                drawn = true;
            }
            if let (Some(x0), Some(x1)) = (
                timeline.locate_in_lane_clamped(run.start_ms),
                timeline.locate_in_lane_clamped(run.end_ms),
            ) && x1.normalized > x0.normalized
            {
                drafts.push(draft(x0.normalized, x1.normalized, run.quantity));
                drawn = true;
            }
        } else if let (Some(x0), Some(x1)) = (
            timeline.locate_clamped(run.start_ms),
            timeline.locate_clamped(run.end_ms),
        ) && x1.normalized > x0.normalized
        {
            drafts.push(draft(x0.normalized, x1.normalized, run.quantity));
            drawn = true;
        }
        if drawn {
            run_quantities.push(run.quantity);
        }
    }
    for ((index, price_bucket, side), heat) in summary {
        let (x0, x1) = timeline.slot_bounds(index);
        drafts.push(DraftCell {
            generation: heat.generation,
            side,
            price_bucket,
            quantity: heat.weighted / Decimal::from(heat.span_ms),
            x0,
            x1,
            y0: heat.y0,
            y1: heat.y1,
        });
    }

    HeatDrafts {
        drafts,
        run_quantities,
    }
}

/// Reference first, visibility second, cap third: hiding is never cap loss.
pub(super) fn finish_heat(geometry: HeatDrafts, config: &HeatmapConfig) -> ProjectedHeat {
    let HeatDrafts {
        mut drafts,
        run_quantities,
    } = geometry;
    let liquidity_reference = match config.intensity_mode {
        IntensityMode::VisibleP99 => percentile_99(run_quantities.into_iter()),
        IntensityMode::Fixed(maximum) => maximum,
    };

    // Hidden heat is gated after the reference: the drafts fed the P99 above,
    // and the depletion floors keyed to it must not move just because the map
    // behind them is switched off. Before the drop accounting, so the health
    // counters never blame the cap for cells the user chose to hide.
    if !config.show_liquidity {
        drafts.clear();
    }
    let dropped_cells = drafts.len().saturating_sub(config.max_visible_cells);
    if dropped_cells > 0 {
        // Retain the strongest walls deterministically and surface the loss.
        drafts.sort_by(|a, b| {
            b.quantity
                .cmp(&a.quantity)
                .then_with(|| a.generation.cmp(&b.generation))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.x0.total_cmp(&b.x0))
        });
        drafts.truncate(config.max_visible_cells);
    }

    let cells = drafts
        .into_iter()
        .map(|draft| {
            let intensity =
                normalized_log_intensity(draft.quantity, liquidity_reference, config.gamma);
            HeatmapCell {
                generation: draft.generation,
                side: draft.side,
                price_bucket: draft.price_bucket,
                quantity: draft.quantity,
                x0: draft.x0,
                x1: draft.x1,
                y0: draft.y0,
                y1: draft.y1,
                intensity,
                alpha: intensity * config.opacity,
            }
        })
        .collect();

    ProjectedHeat {
        cells,
        liquidity_reference,
        dropped_cells,
    }
}

fn percentile_99(values: impl Iterator<Item = Decimal>) -> Decimal {
    let mut positive: Vec<Decimal> = values
        .filter(|quantity| *quantity > Decimal::ZERO)
        .collect();
    if positive.is_empty() {
        return Decimal::ZERO;
    }
    positive.sort_unstable();
    let rank = (99 * positive.len()).div_ceil(100);
    positive[rank.saturating_sub(1)]
}

#[cfg(test)]
mod tests;
