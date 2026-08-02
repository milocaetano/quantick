//! Pure order-flow view model used by the chart heatmap.
//!
//! Nothing in this module depends on `egui`, a renderer, a wall clock or the
//! network. The live feed owns synchronization; this layer retains honest
//! coverage, compresses displayed liquidity into runs and projects those runs
//! onto the existing alternative-bar chart.

pub mod config;
pub mod grouping;
pub mod history;
pub mod interaction;
pub mod projection;
pub mod timeline;

// This facade is intentionally wider than the first UI integration. Keeping
// the public DTOs here gives later renderers one stable import surface.
#[allow(unused_imports)]
pub use config::{
    BubbleRenderMode, BubbleSizeReference, BubbleStyle, DisplayGrouping, HeatmapConfig,
    HeatmapTheme, IntensityMode, LiveLaneStyle, MAX_BUBBLE_MAX_RADIUS, MAX_BUBBLE_MIN_RADIUS,
    MAX_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_SHARE, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_MAX_RADIUS,
    MIN_LIVE_LANE_RADIUS_SCALE, MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_ZOOM,
};
#[allow(unused_imports)]
pub use grouping::{
    EffectiveGrouping, GroupedLiquidity, GroupingWindow, LiquidityTransition, VisualLiquidityRun,
    bucket_for_price, sweep_grouped_runs,
};
#[allow(unused_imports)]
pub use history::{
    Aggression, AggressorSide, CoverageGap, CoverageSegment, GroupingReset, HistoryCounters,
    HistoryError, HistoryStatus, LiquidityHistory, LiquidityRun, RestingSide,
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
pub use timeline::{BarTimeline, LiveEdge, TimelinePosition, reserved_span_ms};
