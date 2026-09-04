//! The order-flow engine: liquidity history, grouping, timeline and the
//! settled/live heatmap projections built from L2 depth and prints.
//!
//! Nothing in this crate depends on `egui`, a renderer, a wall clock or the
//! network. The live feed owns synchronization; this layer retains honest
//! coverage, compresses displayed liquidity into runs and projects those runs
//! onto the existing alternative-bar chart. The projection cache is *told*
//! the time by its caller ([`engine::BookEngine::project_at`]).

pub mod config;
pub mod engine;
pub mod grouping;
pub mod history;
pub mod interaction;
pub mod projection;
pub mod scale;
pub mod timeline;

// This facade is intentionally wider than the first UI integration. Keeping
// the public DTOs here gives later renderers one stable import surface.
#[allow(unused_imports)]
pub use config::{
    BubbleRenderMode, BubbleSizeReference, BubbleStyle, ConsumptionMark, DisplayGrouping,
    GOLDEN_ANGLE, HeatmapConfig, HeatmapTheme, INV_PHI, INV_PHI_2, INV_PHI_3, IntensityMode,
    LANE_WINDOW_PRESETS_MS, LaneWindow, LiveLaneStyle, MAX_BUBBLE_MAX_RADIUS,
    MAX_BUBBLE_MIN_RADIUS, MAX_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_SHARE,
    MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_MAX_RADIUS, MIN_LIVE_LANE_RADIUS_SCALE,
    MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_ZOOM, format_window_ms,
    lane_lag_label, lane_window_label, same_lane_window,
};
#[allow(unused_imports)]
pub use grouping::{
    EffectiveGrouping, GroupedLiquidity, GroupingWindow, LiquidityTransition, VisualLiquidityRun,
    bucket_for_price, sweep_grouped_runs,
};
#[allow(unused_imports)]
pub use history::{
    Aggression, AggressorSide, CoverageGap, CoverageSegment, GroupingReset, HistoryCounters,
    HistoryError, HistoryStatus, LiquidityHistory, LiquidityRun, RestingSide, TapeAge,
};
#[allow(unused_imports)]
pub use interaction::{
    AggressionCluster, LiquidityEvent, cluster_aggressions, correlate_liquidity, generation_at,
    liquidity_events, sort_clusters, summarize_clusters,
};
#[allow(unused_imports)]
pub use projection::{
    AggressionPrimitive, BEFORE_CAPTURE, GapPrimitive, HeatmapCell, HeatmapProjection,
    LiquidityEventPrimitive, LiquidityEvidence, LiveMarks, PriceWindow, SettledProjection,
    project_live, project_settled,
};
#[allow(unused_imports)]
pub use scale::SessionScale;
#[allow(unused_imports)]
pub use timeline::{BarTimeline, LiveEdge, TimelinePosition, reserved_span_ms};

/// Source-to-consumer delay observed when an event arrives.
///
/// `None` when there is no event timestamp to compare. Can be slightly negative
/// if the local clock is behind the source's — reported as-is (honest), not
/// clamped.
#[must_use]
pub fn feed_lag_ms(received_at_ms: i64, event_time_ms: Option<i64>) -> Option<i64> {
    event_time_ms.map(|timestamp_ms| received_at_ms.saturating_sub(timestamp_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_is_observation_minus_event_time() {
        assert_eq!(feed_lag_ms(1_000, Some(600)), Some(400));
        assert_eq!(feed_lag_ms(1_000, None), None);
        // Local clock behind the exchange: negative lag, reported honestly.
        assert_eq!(feed_lag_ms(500, Some(600)), Some(-100));
    }
}
