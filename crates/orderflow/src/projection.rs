//! Renderer-independent projection of RLE history into normalized primitives.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use super::config::{DisplayGrouping, HeatmapConfig, IntensityMode};
use super::grouping::{EffectiveGrouping, GroupedLiquidity, GroupingWindow, sweep_grouped_runs};
use super::history::{LiquidityHistory, RestingSide};
pub use super::interaction::LiquidityEvidence;
use super::interaction::{LiquidityEvent, correlate_liquidity, liquidity_events};
use super::timeline::BarTimeline;

mod fold;
mod model;
mod tiers;

pub use model::{
    AggressionPrimitive, BEFORE_CAPTURE, GapPrimitive, HeatmapCell, HeatmapProjection,
    LiquidityEventPrimitive, LiveMarks, PriceWindow, SettledProjection,
};

use fold::{FoldOrder, fold_to_budget, pane_budgets};
use tiers::{TierClusters, TierCut, TierGrouping, cluster_tier, refine_tier, tier_primitives};

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

/// Project retained order flow into `[0,1] × [0,1]` chart primitives.
///
/// Capture buckets are swept into visual ranges only for this frame. Retained
/// base history remains untouched when grouping or zoom changes.
///
/// The whole frame in one call, which is how the tests read a projection.
///
/// The app itself never takes this path: it redraws the tape far faster than it
/// redraws the map, so it holds the two halves apart — [`project_settled`] and
/// [`project_live`] — and puts them together per frame with
/// [`SettledProjection::with_live`]. Composing them here, in one call, is what
/// keeps that split honest: every test that asserts on a whole frame is
/// asserting on both halves joined exactly as the app joins them.
#[cfg(test)]
#[must_use]
pub fn project(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> HeatmapProjection {
    let settled = project_settled(history, timeline, prices);
    let live = project_live(history, timeline, prices, &settled);
    settled.with_live(live, history.config())
}

/// Build the half of the frame that is finished.
///
/// Everything here is a statement about time that has stopped moving: resting
/// liquidity as it was, the reductions it went through, and the bubbles of bars
/// whose prints are all in. It is what a caller may keep and redraw unchanged
/// until the layout moves under it.
#[must_use]
pub fn project_settled(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> SettledProjection {
    let config = history.config();
    let effective_grouping = EffectiveGrouping::resolve(
        config.display_grouping,
        config.price_grouping,
        prices.high - prices.low,
    );
    if !config.any_layer_enabled() {
        return SettledProjection::empty(false, effective_grouping);
    }
    let Some((time_start, time_end)) = timeline.timestamp_range() else {
        return SettledProjection::empty(true, effective_grouping);
    };

    // The depth layer is projected only while the map is both recording and on
    // screen — on *either* pane. Retained runs survive hiding it untouched:
    // they simply stop being drawn and keep accumulating, so the aggression
    // layer can render without the map behind it and reopening repaints the
    // whole retained past.
    //
    // "Either pane" is the whole point and was the bug: these cells span the
    // normalized x axis, tape included, and the renderer clips them per pane
    // (`layer_clip`). Gating production on the *candles'* switch therefore
    // deleted the tape's map along with the chart's — the projection decided
    // there was nothing to draw before the renderer ever got to decide where.
    let depth_enabled = config.depth_visible_anywhere();
    let retained_start = history
        .retention_start_ms()
        .map_or(time_start, |start| start.max(time_start));
    let open_run_end_ms = history.latest_book_ms().unwrap_or(time_end);
    let coverage: Vec<_> = if depth_enabled {
        history.coverage_segments().cloned().collect()
    } else {
        Vec::new()
    };
    let grouped = if depth_enabled {
        sweep_grouped_runs(
            history.runs_intersecting(retained_start, time_end),
            coverage.iter(),
            effective_grouping,
            GroupingWindow {
                start_ms: retained_start,
                end_ms: time_end,
                open_run_end_ms,
                price_low: prices.low,
                price_high: prices.high,
            },
        )
    } else {
        GroupedLiquidity::default()
    };

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
    let mut run_quantities = Vec::with_capacity(grouped.runs.len());
    let mut summary: BTreeMap<(usize, Decimal, RestingSide), SlotHeat> = BTreeMap::new();
    let lane_view = timeline.lane_start_ms().is_some();
    for run in &grouped.runs {
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

    // The chart draws the same flow in two views. The tape shows the last
    // stretch of market time print by print; a bar slot shows what its bar has
    // come to. A print is on the tape while it is inside the rolling window,
    // and it belongs to its bar's slot either once it has aged out of the
    // window or — while summarizing — immediately, because a summary is a
    // running statement about its bar and has to be complete at every instant:
    // the bar that just closed the moment it closes, and the forming bar as its
    // orders arrive, so the left side reads what is happening now instead of
    // only what already happened. Raw prints are still drawn exactly once; only
    // the aggregate is allowed to overlap the tape it was computed from.
    // Whether the bar is summarized is the trader's summary switch and nothing
    // else. It used to also demand both side switches — a two-sided mark would
    // lie about its size with one side hidden — but that put a *display*
    // choice back inside the projection, and the live strip reads these same
    // clusters: hiding one side of the bubbles reshaped the strip's histogram
    // (the hostage relationship this branch exists to end, and the shipped
    // presets turn the summary on). The honesty it protected is enforced where
    // the ink is now: `RenderContext::bubbles` refuses to draw a two-sided
    // mark while a side is hidden.
    let summarizing = config.bubble_candle_summary;

    // The chart is cut in two at the oldest bar still taking orders. What
    // follows that instant is redrawn from the tape every frame, so this half
    // deliberately stops short of it; what precedes it is finished and is what
    // this build is for.
    let live_from_ms = timeline.live_boundary_ms();
    let mut settled = cluster_tier(
        history,
        timeline,
        prices,
        &coverage,
        TierGrouping {
            slots: effective_grouping,
            lane: lane_grouping(config),
        },
        TierCut {
            range: (None, live_from_ms),
            tape_from_ms: None,
        },
        summarizing,
    );
    // The size scale is a statement about the session, never about the
    // screen: zoom decides what is visible, not what a quantity means, so the
    // same print keeps the same area through every window, and a cluster the
    // viewport merges past the scale saturates at full size — the honest
    // reading of "more than anything the scale measures". The history
    // accumulates it one print at a time (`SessionScale`), so reading it here
    // costs the same whether ten prints are retained or a million — and it is
    // independent of the display filter below, so hiding small prints never
    // silently rescales the ones left on screen.
    let aggression_reference = history.bubble_size_reference();

    // A reduction is allocated by the half that owns the prints around it, so
    // the same removed quantity is never claimed as evidence twice. Cut at the
    // same instant the prints were.
    let mut events = if config.liquidity_events_enabled() {
        liquidity_events(&grouped.transitions)
    } else {
        Vec::new()
    };
    let live_events = match live_from_ms {
        Some(from) => {
            let mut live = Vec::new();
            let mut kept = Vec::with_capacity(events.len());
            for event in std::mem::take(&mut events) {
                if event.timestamp_ms >= from {
                    live.push(event);
                } else {
                    kept.push(event);
                }
            }
            events = kept;
            live
        }
        None => Vec::new(),
    };
    correlate_tier(&mut events, &mut settled, config, summarizing);
    let dropped_liquidity_events = filter_events(&mut events, config, liquidity_reference);

    let (settled_marks, floored_quantity) = refine_tier(
        settled,
        config,
        aggression_reference,
        timeline,
        effective_grouping,
        summarizing,
    );
    // While every mark is a raw print they share the session print scale
    // above, so an area means the same thing everywhere. The summary breaks
    // that premise: a pie carries a whole bar and a tape mark carries one
    // print, quantities an order of magnitude apart, and one shared reference
    // would peg every pie at the largest radius while flattening the tape
    // into dots. Pies then get their own scale — as session-anchored as the
    // print scale, measured against the busiest minute a price level saw
    // (`SummaryScale`), never against whatever pies happen to be on screen:
    // pies dominate a summarized chart, so a viewport reference here would
    // hand the zoom the very rescale the print scale just took away. Under a
    // fixed reference both getters return the pinned quantity, because the
    // user chose it precisely so that nothing on screen may rescale a mark.
    let summary_reference = if summarizing {
        history.bubble_summary_reference()
    } else {
        aggression_reference
    };

    let mut aggressions = tier_primitives(
        settled_marks,
        timeline,
        prices,
        aggression_reference,
        summary_reference,
    );
    let (chart_budget, _) = pane_budgets(config.max_aggression_primitives, &config.live_lane);
    let before_fold = aggressions.len();
    fold_to_budget(
        &mut aggressions,
        chart_budget,
        FoldOrder::SmallestFirst,
        // The scale the marks were drawn on. With the summary on these are
        // pies carrying whole bars, sized against `summary_reference`; folding
        // them against the print scale pegs every one at the largest radius
        // (`normalized_area_size` clamps the ratio), so a summarized chart over
        // budget would fill with max-size pies beside honestly sized ones.
        summary_reference,
        Some(timeline),
    );
    let folded_aggressions = before_fold.saturating_sub(aggressions.len());

    let liquidity_events = event_primitives(events, timeline, prices, effective_grouping);

    // Coverage primitives describe the depth layer. With L2 capture off there
    // is no map whose absence needs explaining, so a bubbles-only frame emits
    // no gap marks at all.
    let mut gaps: Vec<GapPrimitive> = if depth_enabled && config.show_gaps {
        history
            .coverage_gaps()
            .filter_map(|gap| {
                let gap_end = gap.end_ms.unwrap_or(time_end);
                if gap_end <= time_start || gap.start_ms >= time_end {
                    return None;
                }
                let x0 = timeline.locate_clamped(gap.start_ms.max(time_start))?;
                let x1 = timeline.locate_clamped(gap_end.min(time_end))?;
                (x1.normalized > x0.normalized).then(|| GapPrimitive {
                    from_generation: gap.from_generation,
                    to_generation: gap.to_generation,
                    x0: x0.normalized,
                    x1: x1.normalized,
                    reason: gap.reason.clone(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // Historical trades can precede the first locally captured L2 snapshot.
    // Make that absence an explicit primitive instead of a transparent region
    // that could be mistaken for zero resting liquidity. The gap switch covers
    // this leading span too: it is one legend entry, and half-hiding it would
    // leave the legend describing marks the viewer cannot see.
    if depth_enabled && config.show_gaps {
        match history.coverage_segments().next() {
            Some(first_coverage) if first_coverage.start_ms > time_start => {
                let unavailable_end = first_coverage.start_ms.min(time_end);
                if let (Some(x0), Some(x1)) = (
                    timeline.locate_clamped(time_start),
                    timeline.locate_clamped(unavailable_end),
                ) && x1.normalized > x0.normalized
                {
                    gaps.push(GapPrimitive {
                        from_generation: None,
                        to_generation: Some(first_coverage.generation),
                        x0: x0.normalized,
                        x1: x1.normalized,
                        reason: BEFORE_CAPTURE.to_owned(),
                    });
                }
            }
            None => gaps.push(GapPrimitive {
                from_generation: None,
                to_generation: None,
                x0: 0.0,
                x1: 1.0,
                reason: "book_unavailable_before_capture".to_owned(),
            }),
            Some(_) => {}
        }
    }
    gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));

    SettledProjection {
        enabled: true,
        summarized: summarizing,
        floored_quantity,
        cells: Arc::new(cells),
        aggressions,
        liquidity_events,
        gaps: Arc::new(gaps),
        effective_grouping,
        liquidity_reference,
        aggression_reference,
        summary_reference,
        dropped_cells,
        folded_aggressions,
        dropped_liquidity_events,
        live_from_ms,
        live_events,
    }
}

/// Build the marks of the part of the chart that is still moving: the prints
/// rolling through the lane, and the bar still taking orders.
///
/// Cheap by construction — it only ever touches the prints after
/// [`SettledProjection::live_from_ms`] — so a caller may run it on every frame
/// and have a print reach the screen in the frame after it arrived.
///
/// `settled` supplies the two size scales, so a bubble drawn here reads on
/// exactly the scale the settled bubbles beside it were drawn on. Those scales
/// are as old as the settled half; a print large enough to move them therefore
/// resizes the chart when that half is next rebuilt, not the instant it lands.
#[must_use]
pub fn project_live(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    settled: &SettledProjection,
) -> LiveMarks {
    let config = history.config();
    if !settled.enabled || !config.any_layer_enabled() {
        return LiveMarks::default();
    }
    let live_now_x = timeline
        .live_now_position()
        .map(|position| position.normalized);
    // No boundary means no bar is represented at all, so there is no live half
    // to draw — and no reason to walk the retained tape looking for one.
    //
    // Hidden bubbles are *not* a reason to skip this: a reduction is labelled
    // as aggression-aligned by the print that explains it, so the prints are
    // still clustered and matched, and only the marks are dropped later.
    let Some(live_from_ms) = settled.live_from_ms else {
        return LiveMarks {
            live_now_x,
            ..LiveMarks::default()
        };
    };
    // Literally the same rule as the settled half. It used to also require
    // both sides visible, so hiding one side moved the seam's marks between
    // pies and raw prints — two halves disagreeing about what a bar is. The
    // renderer is where a one-sided frame is handled (`RenderContext::bubbles`
    // refuses to draw a two-sided mark while a side is hidden).
    let summarizing = config.bubble_candle_summary;
    // Same rule as the settled half: this is the *tape's* projection, so a
    // switch on the candles may not empty it.
    let coverage: Vec<_> = if config.depth_visible_anywhere() {
        history.coverage_segments().cloned().collect()
    } else {
        Vec::new()
    };
    let mut tier = cluster_tier(
        history,
        timeline,
        prices,
        &coverage,
        TierGrouping {
            slots: settled.effective_grouping,
            lane: lane_grouping(config),
        },
        TierCut {
            range: (Some(live_from_ms), None),
            tape_from_ms: timeline.lane_start_ms(),
        },
        summarizing,
    );
    let mut events = settled.live_events.clone();
    correlate_tier(&mut events, &mut tier, config, summarizing);
    let dropped_liquidity_events = filter_events(&mut events, config, settled.liquidity_reference);
    let (marks, floored_quantity) = refine_tier(
        tier,
        config,
        settled.aggression_reference,
        timeline,
        settled.effective_grouping,
        summarizing,
    );
    // This half carries marks for *both* panes: the prints rolling through the
    // tape, and — while the summary is on — the forming bar's own slot marks.
    // They are folded apart, because a fold that mixed them would draw one
    // pane's volume inside the other: the renderer clips by `x`, sizes by the
    // pane's own radius range and gates on the pane's own switch, so a merged
    // mark would be clipped into one pane, sized for the other, and hidden by
    // the wrong control.
    let (mut tape_marks, mut slot_marks): (Vec<_>, Vec<_>) = tier_primitives(
        marks,
        timeline,
        prices,
        settled.aggression_reference,
        settled.summary_reference,
    )
    .into_iter()
    .partition(|mark| mark.live);
    let before = tape_marks.len() + slot_marks.len();
    let (chart_budget, lane_budget) =
        pane_budgets(config.max_aggression_primitives, &config.live_lane);
    fold_to_budget(
        &mut tape_marks,
        lane_budget,
        FoldOrder::OldestFirst,
        settled.aggression_reference,
        None,
    );
    // The forming bar's marks are candle marks and answer to the candles'
    // budget and the candles' ranking. Sized on the summary scale when there is
    // one, for the same reason `tier_primitives` drew them on it: a pie carries
    // a whole bar and a tape mark carries one print, so folding a pie against
    // the print scale would peg it at the largest radius.
    fold_to_budget(
        &mut slot_marks,
        chart_budget,
        FoldOrder::SmallestFirst,
        settled.summary_reference,
        Some(timeline),
    );
    tape_marks.append(&mut slot_marks);
    let folded_aggressions = before.saturating_sub(tape_marks.len());

    LiveMarks {
        aggressions: tape_marks,
        liquidity_events: event_primitives(events, timeline, prices, settled.effective_grouping),
        dropped_liquidity_events,
        folded_aggressions,
        floored_quantity,
        live_now_x,
    }
}

/// Allocate `events` to the prints of one half of the chart.
///
/// Each half is handed only the reductions timestamped inside it, so a removed
/// quantity is claimed as evidence exactly once however the chart is cut.
fn correlate_tier(
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
fn filter_events(
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

/// How the safety cap ranks reductions, wherever it is applied: aligned
/// evidence first, because it is what a bubble points at, then the biggest
/// reduction, then time and id so the same book always yields the same markers.
fn event_cap_key(
    evidence: LiquidityEvidence,
    removed: Decimal,
    timestamp_ms: i64,
    event_id: u64,
) -> (Reverse<bool>, Reverse<Decimal>, i64, u64) {
    (
        Reverse(matches!(evidence, LiquidityEvidence::AggressionAligned)),
        Reverse(removed),
        timestamp_ms,
        event_id,
    )
}

/// Keep the strongest `limit` reductions of a whole frame.
fn cap_events(events: &mut Vec<LiquidityEventPrimitive>, limit: usize) {
    if events.len() <= limit {
        return;
    }
    events.sort_by_key(|event| {
        event_cap_key(
            event.evidence,
            event.removed,
            event.timestamp_ms,
            event.event_id,
        )
    });
    events.truncate(limit);
}

/// Place reductions on the chart.
fn event_primitives(
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

/// The price resolution the tape clusters on.
///
/// Native, always: the adaptive display grouping is a *compression* device for
/// the candles, where a wide window has to fit many bars into few pixels, and
/// resolving it against the visible price span is what made the candles' zoom
/// decide which of the tape's prints fused together. The tape is not
/// compressed — it has the room to draw prints one by one, which is the whole
/// reason a scalper reads it — so it clusters at capture resolution and stays
/// the same picture through every zoom of the pane beside it.
fn lane_grouping(config: &HeatmapConfig) -> EffectiveGrouping {
    // Only the adaptive mode is overridden. `Adaptive` resolves against the
    // visible price span, which is how the candles' zoom came to decide which
    // of the tape's prints fused together — that is the leak. `Multiple(n)` is
    // the trader saying "give me rows this wide" and has nothing to do with
    // zoom, so it is obeyed on the tape as it is on the candles; overriding it
    // would leave an illiquid instrument readable on one pane and not the
    // other, with no control and no explanation.
    let display = match config.display_grouping {
        DisplayGrouping::Adaptive { .. } => DisplayGrouping::Native,
        chosen => chosen,
    };
    EffectiveGrouping::resolve(display, config.price_grouping, Decimal::ZERO)
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

/// Shared with the live strip in the chart, which normalizes its
/// depth rows against the same reference the heatmap cells used, so one wall
/// reads with one colour on both sides of the chart edge.
pub fn normalized_log_intensity(quantity: Decimal, reference: Decimal, gamma: f32) -> f32 {
    if quantity <= Decimal::ZERO || reference <= Decimal::ZERO {
        return 0.0;
    }
    let ratio = (quantity / reference).to_f64().unwrap_or(0.0).max(0.0);
    let logarithmic = ((1.0 + 9.0 * ratio).ln() / 10.0_f64.ln()).clamp(0.0, 1.0);
    logarithmic.powf(f64::from(gamma)) as f32
}

/// Shared with the live strip's aggression histogram, which sizes its bars by
/// the same square-root area rule the bubbles use — twice the quantity reads
/// as twice the ink, on both.
pub fn normalized_area_size(quantity: Decimal, reference: Decimal) -> f32 {
    if quantity <= Decimal::ZERO || reference <= Decimal::ZERO {
        return 0.0;
    }
    (quantity / reference)
        .to_f64()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        .sqrt() as f32
}

#[cfg(test)]
mod tests;
