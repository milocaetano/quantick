//! Renderer-independent projection of RLE history into normalized primitives.

use super::history::LiquidityHistory;
pub use super::interaction::LiquidityEvidence;
use super::timeline::BarTimeline;
use std::sync::Arc;

mod evidence;
mod fold;
mod gaps;
mod heat;
mod model;
mod settled_flow;
mod tiers;
mod window;

pub use model::{
    AggressionPrimitive, BEFORE_CAPTURE, GapPrimitive, HeatmapCell, HeatmapProjection,
    LiquidityEventPrimitive, LiveMarks, PriceWindow, SettledProjection, normalized_area_size,
    normalized_log_intensity,
};

use evidence::{correlate_tier, event_primitives, filter_events};
use fold::{FoldOrder, fold_to_budget, pane_budgets};
use tiers::{TierCut, TierGrouping, cluster_tier, refine_tier, tier_primitives};
use window::lane_grouping;

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
    let window = match window::resolve_window(history, timeline, prices) {
        window::WindowResolution::Disabled(grouping) => {
            return SettledProjection::empty(false, grouping);
        }
        window::WindowResolution::Empty(grouping) => {
            return SettledProjection::empty(true, grouping);
        }
        window::WindowResolution::Ready(window) => window,
    };
    let liquidity = window::sweep_window(history, &window, prices);
    let geometry = heat::project_geometry(
        &liquidity.grouped.runs,
        timeline,
        prices,
        window.effective_grouping,
    );
    let heat = heat::finish_heat(geometry, history.config());
    let flow = settled_flow::cluster_settled(
        history,
        timeline,
        prices,
        &liquidity.coverage,
        window.effective_grouping,
        &liquidity.grouped.transitions,
    );
    let marks = settled_flow::finish_settled(
        history,
        timeline,
        prices,
        window.effective_grouping,
        heat.liquidity_reference,
        flow.clusters,
    );
    let gaps = gaps::project_gaps(
        history,
        timeline,
        window.time_start,
        window.time_end,
        window.depth_enabled,
    );
    SettledProjection {
        enabled: true,
        summarized: history.config().bubble_candle_summary,
        floored_quantity: marks.floored_quantity,
        cells: Arc::new(heat.cells),
        aggressions: marks.aggressions,
        liquidity_events: marks.liquidity_events,
        gaps: Arc::new(gaps),
        effective_grouping: window.effective_grouping,
        liquidity_reference: heat.liquidity_reference,
        aggression_reference: marks.aggression_reference,
        summary_reference: marks.summary_reference,
        dropped_cells: heat.dropped_cells,
        folded_aggressions: marks.folded_aggressions,
        dropped_liquidity_events: marks.dropped_liquidity_events,
        live_from_ms: flow.live_from_ms,
        live_events: flow.live_events,
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

#[cfg(test)]
mod tests;
