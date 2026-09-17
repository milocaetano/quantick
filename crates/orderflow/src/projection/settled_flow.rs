//! Settled print ownership, seam partition and evidence-preserving finishing.
use super::evidence::{correlate_tier, event_primitives, filter_events};
use super::fold::{FoldOrder, fold_to_budget, pane_budgets};
use super::model::{AggressionPrimitive, LiquidityEventPrimitive, PriceWindow};
use super::tiers::{
    TierClusters, TierCut, TierGrouping, cluster_tier, refine_tier, tier_primitives,
};
use super::window::lane_grouping;
use crate::grouping::{EffectiveGrouping, LiquidityTransition};
use crate::history::{CoverageSegment, LiquidityHistory};
use crate::interaction::{LiquidityEvent, liquidity_events};
use crate::timeline::BarTimeline;
use rust_decimal::Decimal;

pub(super) struct SettledClusters {
    tier: TierClusters,
    events: Vec<LiquidityEvent>,
}

pub(super) struct SettledFlow {
    pub(super) clusters: SettledClusters,
    pub(super) live_events: Vec<LiquidityEvent>,
    pub(super) live_from_ms: Option<i64>,
}

pub(super) struct SettledMarks {
    pub(super) aggressions: Vec<AggressionPrimitive>,
    pub(super) liquidity_events: Vec<LiquidityEventPrimitive>,
    pub(super) floored_quantity: Decimal,
    pub(super) aggression_reference: Decimal,
    pub(super) summary_reference: Decimal,
    pub(super) folded_aggressions: usize,
    pub(super) dropped_liquidity_events: usize,
}

pub(super) fn cluster_settled(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    coverage: &[CoverageSegment],
    effective_grouping: EffectiveGrouping,
    transitions: &[LiquidityTransition],
) -> SettledFlow {
    let config = history.config();
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
    let settled = cluster_tier(
        history,
        timeline,
        prices,
        coverage,
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
    // A reduction is allocated by the half that owns the prints around it, so
    // the same removed quantity is never claimed as evidence twice. Cut at the
    // same instant the prints were.
    let mut events = if config.liquidity_events_enabled() {
        liquidity_events(transitions)
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
    SettledFlow {
        clusters: SettledClusters {
            tier: settled,
            events,
        },
        live_events,
        live_from_ms,
    }
}

pub(super) fn finish_settled(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    effective_grouping: EffectiveGrouping,
    liquidity_reference: Decimal,
    clusters: SettledClusters,
) -> SettledMarks {
    let config = history.config();
    let summarizing = config.bubble_candle_summary;
    let SettledClusters {
        tier: mut settled,
        mut events,
    } = clusters;
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

    SettledMarks {
        aggressions,
        liquidity_events,
        floored_quantity,
        aggression_reference,
        summary_reference,
        folded_aggressions,
        dropped_liquidity_events,
    }
}

#[cfg(test)]
mod tests;
