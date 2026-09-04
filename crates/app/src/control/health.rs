//! Frame, loading, indicator, and order-flow health projection.

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use quantick_orderflow::engine::OrderflowHealth;

use crate::{
    app::QuantickApp,
    loading::LoadingTask,
    pane::{ChartPane, PaneSide},
    tab::Tab,
};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{PaneSideDto, canonical_decimal, canonical_f32, wire_usize},
};

pub(crate) const SCOPE_ID: &str = "health.summary";
const MODULE_ID: &str = "health";
const SCHEMA_VERSION: u32 = 1;
const METRIC_DECIMAL_PLACES: u32 = 6;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct HealthSnapshot {
    pub frame: FrameHealthSnapshot,
    pub tabs: Vec<TabHealthSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct FrameHealthSnapshot {
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
pub(crate) struct TabHealthSnapshot {
    pub tab_id: WireU64,
    pub active_loading_tasks: Vec<LoadingTaskSnapshot>,
    pub panes: Vec<PaneHealthSnapshot>,
    /// How late this tab's tape is, and where the time is going. `None` while
    /// replaying — a recording's prints are as old as the day they were
    /// captured and the playback clock decides when they appear.
    pub tape: Option<TapeHealthSnapshot>,
}

/// Where a tab's tape delay is being spent.
///
/// The whole point of the breakdown is that "the chart is eighteen seconds
/// behind" is not actionable on its own: it reads the same whether the venue's
/// adapter was late, the wire was late, or this process drained late, and those
/// have different fixes. An investigation that starts here can name the hop
/// without a screenshot and without a person watching the corner of a window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TapeHealthSnapshot {
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
pub(crate) struct LoadingTaskSnapshot {
    pub task: String,
    pub operation_count: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct PaneHealthSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    pub indicator_count: WireU64,
    pub indicator_error_count: WireU64,
    pub indicator_stale_count: WireU64,
    pub indicator_issues: Vec<IndicatorIssueSnapshot>,
    pub orderflow: Option<OrderflowHealthSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct IndicatorIssueSnapshot {
    pub slot_id: WireU64,
    pub source_kind: String,
    pub state: String,
    pub detail: String,
    pub user_text_redacted: bool,
    pub bar_index: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct OrderflowHealthSnapshot {
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

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Health".to_owned(),
            description: "Frame cost and subsystem readiness observations.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "Health summary",
        "Reports frame timing, active work, indicator failures, and the last published order-flow health.",
        &[
            "observe",
            "observe.health",
            "observe.indicators",
            "observe.orderflow",
        ],
        project,
    )
}

/// The module's revision key: the per-tab subsystem state, without the
/// frame averages. Those move on every painted frame, and a revision that
/// advanced on every capture would mark nothing; what the key tracks is a
/// change in what the tabs report about their feed, book and engines.
///
/// The tape figures are coarsened here for the same reason, and it is not a
/// detail: `arrival_latency_ms` is rewritten on every drained print, so
/// carrying it verbatim would wake every `quantick_wait_for_change` waiter on
/// every trade — turning a subsystem watch into a print ticker, and paying a
/// full snapshot serialisation for each tick. What a waiter actually wants to
/// hear is that the tape *became* late, or that the hop changed, so that is
/// what the key holds. The milliseconds stay in the projection, where a reader
/// that asked for them gets them.
fn revision(app: &QuantickApp) -> Vec<TabRevisionKey> {
    snapshot(app)
        .tabs
        .into_iter()
        .map(|mut tab| {
            let tape = tab.tape.as_ref().map(tape_revision_key);
            // Dropped from the key, not from the projection: these are the
            // per-print milliseconds the doc above explains.
            tab.tape = None;
            TabRevisionKey { tab, tape }
        })
        .collect()
}

/// One tab's revision key: everything it reports, with the tape's own
/// millisecond figures replaced by the coarse reading above.
#[derive(Clone, Debug, Eq, PartialEq)]
struct TabRevisionKey {
    tab: TabHealthSnapshot,
    tape: Option<TapeRevisionKey>,
}

/// What a waiter is told about the tape: which hop, and late or not.
#[derive(Clone, Debug, Eq, PartialEq)]
struct TapeRevisionKey {
    dominant_hop: Option<String>,
    late: bool,
}

fn tape_revision_key(tape: &TapeHealthSnapshot) -> TapeRevisionKey {
    TapeRevisionKey {
        dominant_hop: tape.dominant_hop.clone(),
        // The chart's own threshold, so a waiter and a trader are told the
        // tape went late at the same instant rather than at two.
        late: tape
            .arrival_latency_ms
            .is_some_and(|ms| ms > crate::metrics::HIGH_LAG_MS),
    }
}

fn project(app: &QuantickApp, _context: CaptureContext) -> HealthSnapshot {
    snapshot(app)
}

fn snapshot(app: &QuantickApp) -> HealthSnapshot {
    let frame = app.control_frame_metrics();
    HealthSnapshot {
        frame: FrameHealthSnapshot {
            wall_average_ms: frame
                .wall_average_ms
                .and_then(|value| canonical_f32(value, METRIC_DECIMAL_PLACES)),
            wall_worst_ms: frame
                .wall_worst_ms
                .and_then(|value| canonical_f32(value, METRIC_DECIMAL_PLACES)),
            frames_per_second: frame
                .frames_per_second
                .and_then(|value| canonical_f32(value, METRIC_DECIMAL_PLACES)),
            cpu_average_ms: frame
                .cpu_average_ms
                .and_then(|value| canonical_f32(value, METRIC_DECIMAL_PLACES)),
            cpu_worst_ms: frame
                .cpu_worst_ms
                .and_then(|value| canonical_f32(value, METRIC_DECIMAL_PLACES)),
        },
        tabs: app
            .control_tabs()
            .iter()
            .map(|tab| {
                let panes: Vec<PaneHealthSnapshot> = tab
                    .panes()
                    .map(|(pane, side)| pane_health(pane, side))
                    .collect();
                TabHealthSnapshot {
                    tab_id: WireU64::new(tab.id),
                    active_loading_tasks: LoadingTask::ALL
                        .into_iter()
                        .filter_map(|task| {
                            let count = tab.loading.count(task);
                            (count > 0).then(|| LoadingTaskSnapshot {
                                task: loading_task_id(task).to_owned(),
                                operation_count: wire_usize(count),
                            })
                        })
                        .collect(),
                    panes,
                    tape: tape_health(tab),
                }
            })
            .collect(),
    }
}

/// This tab's tape delay, split as far as its provider can tell.
///
/// `None` when there is nothing measured to report at all: a tab that has not
/// seen a live print, and any tab playing a recording.
fn tape_health(tab: &Tab) -> Option<TapeHealthSnapshot> {
    let arrival_latency_ms = tab.trade_arrival_ms();
    let split = tab.feed_latency();
    if arrival_latency_ms.is_none() && split.is_none() {
        return None;
    }
    Some(TapeHealthSnapshot {
        arrival_latency_ms,
        feed_arrival_latency_ms: split.map(|s| s.arrival_lag_ms),
        source_latency_ms: split.and_then(|s| s.source_lag_ms),
        source_latency_peak_ms: split.and_then(|s| s.source_lag_peak_ms),
        transport_latency_ms: split.and_then(|s| s.transport_lag_ms),
        dominant_hop: split.and_then(|s| s.hop).map(str::to_owned),
        sampled_prints: WireU64::new(u64::from(split.map_or(0, |s| s.prints))),
    })
}

fn pane_health(pane: &ChartPane, side: PaneSide) -> PaneHealthSnapshot {
    let indicators = pane.indicators.all();
    let indicator_issues = indicators
        .iter()
        .flat_map(|view| {
            let source_kind = if view.kind.starts_with("native.") {
                "native"
            } else {
                "script"
            };
            let user_text_redacted = source_kind == "script";
            let error = view.error.as_ref().map(|error| IndicatorIssueSnapshot {
                slot_id: WireU64::new(view.slot.0),
                source_kind: source_kind.to_owned(),
                state: "error".to_owned(),
                detail: "runtime_evaluation_failed".to_owned(),
                user_text_redacted,
                bar_index: Some(wire_usize(error.bar_index)),
            });
            let stale = view.stale.as_ref().map(|_| IndicatorIssueSnapshot {
                slot_id: WireU64::new(view.slot.0),
                source_kind: source_kind.to_owned(),
                state: "stale".to_owned(),
                detail: "reload_failed_running_version_retained".to_owned(),
                user_text_redacted,
                bar_index: None,
            });
            [error, stale].into_iter().flatten()
        })
        .collect();
    PaneHealthSnapshot {
        pane_id: WireU64::new(pane.id),
        side: side.into(),
        indicator_count: wire_usize(indicators.len()),
        indicator_error_count: wire_usize(
            indicators
                .iter()
                .filter(|view| view.error.is_some())
                .count(),
        ),
        indicator_stale_count: wire_usize(
            indicators
                .iter()
                .filter(|view| view.stale.is_some())
                .count(),
        ),
        indicator_issues,
        orderflow: pane
            .orderflow
            .as_ref()
            .map(|view| orderflow_health(view.cached_health())),
    }
}

fn orderflow_health(health: &OrderflowHealth) -> OrderflowHealthSnapshot {
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

const fn loading_task_id(task: LoadingTask) -> &'static str {
    match task {
        LoadingTask::History => "history",
        LoadingTask::BarRebuild => "bar_rebuild",
        LoadingTask::BookSync => "book_sync",
        LoadingTask::ReplaySession => "replay_session",
        LoadingTask::VenueHistory => "venue_history",
    }
}
