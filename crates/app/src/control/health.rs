//! Frame, loading, indicator, and order-flow health projection.

pub(crate) use quantick_control_schema::health::*;

use crate::app::{HealthPort, TabsPort};
use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};

use crate::{
    loading::LoadingTask,
    pane::{ChartPane, PaneSide},
    tab::Tab,
};

use super::{
    registry::{CaptureContext, ProjectionDock, ProjectionRegistry, ProjectionRegistryError},
    types::wire_usize,
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

fn revision<P: TabsPort + HealthPort + ?Sized>(app: &P) -> Vec<TabHealthRevisionKey> {
    snapshot(app).revision_keys(crate::metrics::HIGH_LAG_MS)
}

fn project<P: TabsPort + HealthPort + ?Sized>(app: &P, _context: CaptureContext) -> HealthSnapshot {
    snapshot(app)
}

fn snapshot<P: TabsPort + HealthPort + ?Sized>(app: &P) -> HealthSnapshot {
    let frame = app.health_reads().frame_metrics();
    HealthSnapshot {
        frame: FrameHealthSnapshot::from_measurements(
            frame.wall_average_ms,
            frame.wall_worst_ms,
            frame.frames_per_second,
            frame.cpu_average_ms,
            frame.cpu_worst_ms,
        ),
        tabs: app
            .tab_reads()
            .tabs()
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
        saves_off_unread_hooks: crate::store_home::writes_refused().map(|refused| refused.hooks),
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
    PaneHealthSnapshot::from_indicators(
        pane.id,
        side.into(),
        pane.indicators
            .all()
            .iter()
            .map(|view| IndicatorHealthRead {
                slot_id: view.slot.0,
                kind: &view.kind,
                error_bar_index: view.error.as_ref().map(|error| error.bar_index),
                stale: view.stale.is_some(),
            }),
        pane.orderflow.as_ref().map(|view| view.cached_health()),
    )
}

const fn loading_task_id(task: LoadingTask) -> &'static str {
    match task {
        LoadingTask::History => "history",
        LoadingTask::BarRebuild => "bar_rebuild",
        LoadingTask::BookSync => "book_sync",
        LoadingTask::ReplaySession => "replay_session",
        LoadingTask::VenueHistory => "venue_history",
        LoadingTask::HistoryRebuild => "history_rebuild",
    }
}

/// The health projection driven through the tabs and health families of a
/// fake window, with no application behind it.
#[cfg(test)]
mod port_tests {
    use super::*;
    use crate::app::control_host::tests::fake::FakeWindow;

    #[test]
    fn a_window_that_measured_nothing_reports_no_frame_metrics() {
        let snapshot = snapshot(&FakeWindow::new());
        assert_eq!(snapshot.frame.wall_average_ms, None);
        assert_eq!(snapshot.tabs.len(), 1);
    }
}
