//! The order-flow engine: liquidity history, grouping, timeline and the
//! settled/live heatmap projections built from L2 depth and prints.
//!
//! Nothing in this crate depends on `egui`, a renderer, a wall clock or the
//! network. The live feed owns synchronization; this layer retains honest
//! coverage, compresses displayed liquidity into runs and projects those runs
//! onto the existing alternative-bar chart. The projection cache is *told*
//! the time by its caller ([`engine::BookEngine::project_at`]).

pub mod config;
mod constants;
pub mod engine;
pub mod grouping;
pub mod health_words;
pub mod history;
pub mod interaction;
pub mod pane_tape;
pub mod projection;
pub mod scale;
pub mod tape_clock;
pub mod tape_view;
pub mod timeline;
pub mod widgets;

// This facade is intentionally wider than the first UI integration. Keeping
// the public DTOs here gives later renderers one stable import surface.
#[allow(unused_imports)]
pub use config::{
    BubbleDisplayScale, BubbleRenderMode, BubbleScaleTarget, BubbleSizeReference, BubbleStyle,
    ConsumptionMark, DEFAULT_BUBBLE_DISPLAY_SCALE, DEFAULT_VOLUME_DOT_FULL_QUANTITY,
    DOT_TAPE_WINDOW_MS, DisplayGrouping, FoldRail, GOLDEN_ANGLE, HeatmapConfig, HeatmapTheme,
    INV_PHI, INV_PHI_2, INV_PHI_3, IntensityMode, LANE_FOLD_AT_PX, LANE_FOLD_HIT_PX,
    LANE_FOLD_RAIL_PX, LANE_WINDOW_PRESETS_MS, LaneDividerDrag, LaneFold, LaneSide, LaneView,
    LaneWindow, LiveLaneStyle, MAX_BUBBLE_DISPLAY_SCALE, MAX_BUBBLE_MAX_RADIUS,
    MAX_BUBBLE_MIN_RADIUS, MAX_LIVE_LANE_RADIUS_SCALE, MAX_LIVE_LANE_SHARE,
    MAX_LIVE_LANE_WINDOW_MS, MAX_LIVE_LANE_ZOOM, MIN_BUBBLE_DISPLAY_SCALE, MIN_BUBBLE_MAX_RADIUS,
    MIN_LIVE_LANE_RADIUS_SCALE, MIN_LIVE_LANE_SHARE, MIN_LIVE_LANE_WINDOW_MS, MIN_LIVE_LANE_ZOOM,
    MarketHold, OldTapeFold, TickBubbleStyle, TickLook, TickReferenceMode, TickTuning,
    ViewportBubbleScale, VolumeDotStyle, admitted_bubble_display_scale, bubble_halo_padding,
    bubble_impact_ring_padding, bubble_radius, format_window_ms, lane_lag_label, lane_time_ticks,
    lane_window_label, lane_window_options, lane_window_pick, same_lane_window,
    sane_bubble_display_scale, sane_volume_dot_full_quantity, side_offset_y,
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
    AggressionPrimitive, BEFORE_CAPTURE, BOOK_PENDING, DOT_LEVEL_LADDER_TICKS, DOT_WINDOW_CELL_PX,
    DOT_WINDOW_LADDER_MS, DotRungMemory, DotScale, DotSizing, DotZoom, FlowCellHit, GapPrimitive,
    HeatmapCell, HeatmapProjection, LiquidityEventPrimitive, LiquidityEvidence, LiveMarks,
    PaneGeometry, PriceWindow, SettledProjection, VolumeDots, dot_level_ticks, dot_window_ms,
    lane_bars, project_live, project_settled,
};
#[allow(unused_imports)]
pub use scale::SessionScale;
#[allow(unused_imports)]
pub use timeline::{BarTimeline, LiveEdge, TimelinePosition, reserved_span_ms};

// One owner for the arithmetic, below both the depth engine here and the
// feed link that books trade prints, so the two lags cannot diverge.
pub use quantick_engine::feed_lag_ms;

#[cfg(test)]
mod pane_tape_tests;

#[cfg(test)]
mod tape_clock_tests;

#[cfg(test)]
mod tape_view_tests;

pub mod status_appearance;
