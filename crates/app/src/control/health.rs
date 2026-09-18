//! Frame, loading, indicator, and order-flow health projection.

pub(crate) use quantick_control_schema::health::*;

use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};

use crate::{
    app::QuantickApp,
    loading::LoadingTask,
    pane::{ChartPane, PaneSide},
    tab::Tab,
};

use super::{
    registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError},
    types::{canonical_f32, wire_usize},
};

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
            .iter_with_ids()
            .map(|(tab_id, tab)| {
                let panes: Vec<PaneHealthSnapshot> = tab
                    .panes()
                    .map(|(pane, side)| pane_health(pane, side))
                    .collect();
                TabHealthSnapshot {
                    tab_id: WireU64::new(tab_id),
                    feed_integrity: (tab.feed_integrity.anomalies > 0).then(|| {
                        let integrity = tab.feed_integrity;
                        FeedIntegritySnapshot {
                            anomalies: WireU64::new(integrity.anomalies),
                            missing_messages: WireU64::new(integrity.missing_messages),
                            unknown_loss: WireU64::new(integrity.unknown_loss),
                            non_monotonic: WireU64::new(integrity.non_monotonic),
                        }
                    }),
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

const fn loading_task_id(task: LoadingTask) -> &'static str {
    match task {
        LoadingTask::History => "history",
        LoadingTask::BarRebuild => "bar_rebuild",
        LoadingTask::BookSync => "book_sync",
        LoadingTask::ReplaySession => "replay_session",
        LoadingTask::VenueHistory => "venue_history",
    }
}
