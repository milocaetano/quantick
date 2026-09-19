//! Frame, loading, indicator, and order-flow health projection.

use quantick_control_host::wire::{PaneSideDto, canonical_decimal, canonical_f32, wire_usize};

use quantick_control::wire::{CanonicalDecimal, WireU64};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use quantick_orderflow::engine::OrderflowHealth;

pub const SCOPE_ID: &str = "health.summary";

pub const MODULE_ID: &str = "health";

pub const SCHEMA_VERSION: u32 = 1;

pub const METRIC_DECIMAL_PLACES: u32 = 6;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HealthSnapshot {
    pub frame: FrameHealthSnapshot,
    pub tabs: Vec<TabHealthSnapshot>,
    /// Present when this session writes no store (decision DS7): the
    /// `QUANTICK_*` names set at launch that this build does not read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saves_off_unread_hooks: Option<Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FrameHealthSnapshot {
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub wall_average_ms: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub wall_worst_ms: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "frames_per_second"))]
    pub frames_per_second: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub cpu_average_ms: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub cpu_worst_ms: Option<CanonicalDecimal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabHealthSnapshot {
    pub tab_id: WireU64,
    /// Cumulative source diagnostics, independent of latency and gap eviction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_integrity: Option<FeedIntegritySnapshot>,
    pub active_loading_tasks: Vec<LoadingTaskSnapshot>,
    pub panes: Vec<PaneHealthSnapshot>,
    /// How late this tab's tape is, and where the time is going. `None` while
    /// replaying — a recording's prints are as old as the day they were
    /// captured and the playback clock decides when they appear.
    pub tape: Option<TapeHealthSnapshot>,
}

/// Counts source messages, never estimates how many executed trades were lost.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedIntegritySnapshot {
    pub anomalies: WireU64,
    pub missing_messages: WireU64,
    pub unknown_loss: WireU64,
    pub non_monotonic: WireU64,
}

/// Where a tab's tape delay is being spent.
///
/// The whole point of the breakdown is that "the chart is eighteen seconds
/// behind" is not actionable on its own: it reads the same whether the venue's
/// adapter was late, the wire was late, or this process drained late, and those
/// have different fixes. An investigation that starts here can name the hop
/// without a screenshot and without a person watching the corner of a window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeHealthSnapshot {
    /// Newest print: venue stamp to this chart drawing it. The end-to-end
    /// figure the status bar shows.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub arrival_latency_ms: Option<i64>,
    /// The same measurement taken where the feed read the print off the wire,
    /// one hop earlier.
    ///
    /// The gap between this and `arrival_latency_ms` is what quantick's own
    /// queue and frame drain cost. It is a *derived* reading, not a measured
    /// one — the two are sampled at different instants — but a gap of seconds
    /// between them is unambiguous, and it is the only way to see that hop at
    /// all. `None` on a provider that cannot cut its own chain.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub feed_arrival_latency_ms: Option<i64>,
    /// Venue stamp to the source handing the print over: everything upstream
    /// of quantick.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub source_latency_ms: Option<i64>,
    /// The worst `source_latency_ms` over the sampled prints.
    ///
    /// The only peak reported, and deliberately: it is two source-side stamps
    /// subtracted per print, so every print contributes with no clock involved.
    /// A peak on the arrival or wire figures would need the reader's clock
    /// applied to a print that arrived earlier, which measures that print's age
    /// rather than its delay — on a quiet tape, the sampling interval itself.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub source_latency_peak_ms: Option<i64>,
    /// The source handing it over to quantick reading it: the wire.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub transport_latency_ms: Option<i64>,
    /// The provider's own name for the hop that owns most of the delay.
    pub dominant_hop: Option<String>,
    /// How many live prints the split covers.
    pub sampled_prints: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoadingTaskSnapshot {
    pub task: String,
    pub operation_count: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneHealthSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub indicator_count: WireU64,
    pub indicator_error_count: WireU64,
    pub indicator_stale_count: WireU64,
    pub indicator_issues: Vec<IndicatorIssueSnapshot>,
    pub orderflow: Option<OrderflowHealthSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndicatorIssueSnapshot {
    pub slot_id: WireU64,
    pub source_kind: String,
    pub state: String,
    pub detail: String,
    pub user_text_redacted: bool,
    pub bar_index: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrderflowHealthSnapshot {
    pub enabled: bool,
    pub status: String,
    pub generation: Option<WireU64>,
    pub last_update_id: Option<WireU64>,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub last_event_unix_ms: Option<i64>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub arrival_latency_ms: Option<i64>,
    pub bid_levels: WireU64,
    pub ask_levels: WireU64,
    pub active_levels: WireU64,
    pub archived_runs: WireU64,
    pub aggression_count: WireU64,
    #[schemars(extend("x-unit" = "bytes"))]
    pub history_bytes: WireU64,
    pub projection_cells: WireU64,
    pub projection_aggressions: WireU64,
    pub projection_liquidity_events: WireU64,
    pub dropped_cells: WireU64,
    /// Aggressions the projection budget folded into a neighbour instead of
    /// drawing alone. Folded, not dropped: the quantity is still on the canvas.
    pub folded_aggressions: WireU64,
    /// Exact quantity the trader's own display floor kept off the canvas.
    pub floored_quantity: CanonicalDecimal,
    pub dropped_liquidity_events: WireU64,
    pub effective_price_grouping: CanonicalDecimal,
    pub effective_grouping_multiple: u32,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub projection_ms: Option<CanonicalDecimal>,
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub live_projection_ms: Option<CanonicalDecimal>,
    pub projection_builds: WireU64,
    pub projection_cache_hits: WireU64,
    pub config_revision: WireU64,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub last_snapshot_observed_unix_ms: Option<i64>,
    pub depth_updates: WireU64,
    pub depth_updates_since_summary: WireU64,
    pub snapshots: WireU64,
    pub gaps: WireU64,
}

pub fn orderflow_health(health: &OrderflowHealth) -> OrderflowHealthSnapshot {
    OrderflowHealthSnapshot {
        enabled: health.enabled,
        status: health.status.to_owned(),
        generation: health.generation.map(WireU64::new),
        last_update_id: health.last_update_id.map(WireU64::new),
        last_event_unix_ms: health.last_event_ms,
        arrival_latency_ms: health.arrival_latency_ms,
        bid_levels: wire_usize(health.bid_levels),
        ask_levels: wire_usize(health.ask_levels),
        active_levels: wire_usize(health.active_levels),
        archived_runs: wire_usize(health.archived_runs),
        aggression_count: wire_usize(health.aggression_count),
        history_bytes: wire_usize(health.history_bytes),
        projection_cells: wire_usize(health.projection_cells),
        projection_aggressions: wire_usize(health.projection_aggressions),
        projection_liquidity_events: wire_usize(health.projection_liquidity_events),
        dropped_cells: wire_usize(health.dropped_cells),
        folded_aggressions: wire_usize(health.folded_aggressions),
        floored_quantity: canonical_decimal(health.floored_quantity),
        dropped_liquidity_events: wire_usize(health.dropped_liquidity_events),
        effective_price_grouping: canonical_decimal(health.effective_grouping),
        effective_grouping_multiple: health.effective_grouping_multiple,
        projection_ms: canonical_f32(health.projection_ms, METRIC_DECIMAL_PLACES),
        live_projection_ms: canonical_f32(health.live_ms, METRIC_DECIMAL_PLACES),
        projection_builds: WireU64::new(health.projection_builds),
        projection_cache_hits: WireU64::new(health.projection_cache_hits),
        config_revision: WireU64::new(health.config_revision),
        last_snapshot_observed_unix_ms: health.last_snapshot_observed_ms,
        depth_updates: WireU64::new(health.depth_updates),
        depth_updates_since_summary: WireU64::new(health.depth_updates_since_summary),
        snapshots: WireU64::new(health.snapshots),
        gaps: WireU64::new(health.gaps),
    }
}
