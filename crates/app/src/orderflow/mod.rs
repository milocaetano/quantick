//! Pure order-flow view model used by the chart heatmap.
//!
//! Nothing in this module depends on `egui`, a renderer, a wall clock or the
//! network. The live feed owns synchronization; this layer retains honest
//! coverage, compresses displayed liquidity into runs and projects those runs
//! onto the existing alternative-bar chart.

pub mod config;
pub mod history;
pub mod projection;
pub mod timeline;

// This facade is intentionally wider than the first UI integration. Keeping
// the public DTOs here gives later renderers one stable import surface.
#[allow(unused_imports)]
pub use config::{HeatmapConfig, IntensityMode};
#[allow(unused_imports)]
pub use history::{
    Aggression, AggressorSide, CoverageGap, CoverageSegment, GroupingReset, HistoryCounters,
    HistoryError, HistoryStatus, LiquidityHistory, LiquidityRun, RestingSide,
};
#[allow(unused_imports)]
pub use projection::{
    AggressionPrimitive, GapPrimitive, HeatmapCell, HeatmapProjection, PriceWindow, project,
};
#[allow(unused_imports)]
pub use timeline::{BarTimeline, TimelinePosition};
