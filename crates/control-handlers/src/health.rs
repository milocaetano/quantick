//! Frame, loading, indicator, and order-flow health projection.
//!
//! The window lends its frame cost and each of its tabs through
//! [`HealthPort`]; a tab answers through [`HealthTab`] with the readings only
//! it holds — the feed's integrity counters and latency split, its loading
//! tracker, and each pane's indicators and published order-flow health. What
//! is worth reporting is decided here: integrity only once something was
//! wrong, the loading work actually in flight, no tape at all while nothing
//! was measured, and a revision key that ignores the per-frame averages.

use quantick_chart::{
    indicator_views::IndicatorViews,
    loading::{LoadingTask, LoadingTracker},
};
use quantick_control::{
    id::{ModuleId, SnapshotScopeId},
    registry::ModuleDescriptor,
    wire::WireU64,
};
use quantick_control_host::{
    projection::{CaptureContext, ProjectionRegistryError},
    wire::{PaneSideDto, wire_usize},
};
use quantick_control_schema::health::{
    FeedIntegritySnapshot, FrameHealthSnapshot, HealthSnapshot, IndicatorHealthRead,
    IntakeLagSnapshot, LoadingTaskSnapshot, MODULE_ID, PaneHealthSnapshot, SCHEMA_VERSION,
    SCOPE_ID, TabHealthRevisionKey, TabHealthSnapshot, TapeHealthSnapshot,
};
use quantick_orderflow::engine::OrderflowHealth;
use quantick_sources::timing::FeedLatency;

use crate::dock::ProjectionDock;

/// The frame cost the window last measured; `None` before it measured any.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameReading {
    pub wall_average_ms: Option<f32>,
    pub wall_worst_ms: Option<f32>,
    pub frames_per_second: Option<f32>,
    pub cpu_average_ms: Option<f32>,
    pub cpu_worst_ms: Option<f32>,
}

/// One chart pane as the health scope reads it.
pub struct PaneHealthRead<'a> {
    pub pane_id: u64,
    pub side: PaneSideDto,
    pub indicators: &'a IndicatorViews,
    /// The health its order-flow engine last published, when it has one.
    pub orderflow: Option<&'a OrderflowHealth>,
}

/// One tab as the health scope reads it.
pub trait HealthTab {
    /// The feed's cumulative source-message integrity counters.
    fn feed_integrity(&self) -> FeedIntegritySnapshot;
    /// The work the tab has in flight.
    fn loading(&self) -> &LoadingTracker;
    /// How late the newest live print arrived; `None` before one, and while
    /// replaying.
    fn arrival_latency_ms(&self) -> Option<i64>;
    /// Where that delay is spent, as the provider splits it; `None` while
    /// replaying and on a provider that cannot cut its own chain.
    fn latency_split(&self) -> Option<FeedLatency>;
    /// How long live prints waited inside a drain; `None` before the feed
    /// session's first live print.
    fn intake_lag(&self) -> Option<IntakeLagSnapshot>;
    /// Hand `visit` the tab's panes in its order, as one sized iterator so
    /// the summary's list is allocated to fit.
    fn visit_panes(&self, visit: &mut dyn FnMut(&mut dyn Iterator<Item = PaneHealthRead<'_>>));
}

/// The window as the health scope reads it.
pub trait HealthPort {
    /// The arrival latency past which the chart calls its tape late.
    const HIGH_LAG_MS: i64;
    /// The frame cost last measured.
    fn frame(&self) -> FrameReading;
    /// Hand `visit` every tab in screen order with its stable id, as one
    /// sized iterator so the summary's list is allocated to fit.
    fn visit_health_tabs(&self, visit: &mut TabVisit<'_>);
}

/// What sees the window's tabs: each with its stable id, as one sized iterator.
pub type TabVisit<'a> = dyn FnMut(&mut dyn Iterator<Item = (u64, &dyn HealthTab)>) + 'a;

/// Dock the health module and its summary scope.
///
/// # Errors
/// A module or scope already docked.
pub fn register<D, H>(registry: &mut D) -> Result<(), ProjectionRegistryError>
where
    D: ProjectionDock<H>,
    H: HealthPort + ?Sized + 'static,
{
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "Health".to_owned(),
            description: "Frame cost and subsystem readiness observations.".to_owned(),
        },
        revision::<H>,
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
        project::<H>,
    )
}

fn revision<H: HealthPort + ?Sized>(host: &H) -> Vec<TabHealthRevisionKey> {
    snapshot(host).revision_keys(H::HIGH_LAG_MS)
}

fn project<H: HealthPort + ?Sized>(host: &H, _context: CaptureContext) -> HealthSnapshot {
    snapshot(host)
}

/// The health summary as the window reads now.
pub fn snapshot<H: HealthPort + ?Sized>(host: &H) -> HealthSnapshot {
    let frame = host.frame();
    let mut tabs = Vec::new();
    host.visit_health_tabs(&mut |all| {
        tabs = exact(all.map(|(tab_id, tab)| tab_health(tab_id, tab)));
    });
    HealthSnapshot {
        frame: FrameHealthSnapshot::from_measurements(
            frame.wall_average_ms,
            frame.wall_worst_ms,
            frame.frames_per_second,
            frame.cpu_average_ms,
            frame.cpu_worst_ms,
        ),
        tabs,
        saves_off_unread_hooks: quantick_workspace::write_refusal::writes_refused()
            .map(|refused| refused.hooks),
    }
}

/// Collect at the length the iterator promises: behind `dyn Iterator` the
/// exact length is lost, and `collect` would reserve room for four.
fn exact<T>(items: impl Iterator<Item = T>) -> Vec<T> {
    let mut collected = Vec::with_capacity(items.size_hint().0);
    collected.extend(items);
    collected
}

fn tab_health(tab_id: u64, tab: &dyn HealthTab) -> TabHealthSnapshot {
    let mut panes = Vec::new();
    tab.visit_panes(&mut |all| panes = exact(all.map(|pane| pane_health(&pane))));
    let integrity = tab.feed_integrity();
    TabHealthSnapshot {
        tab_id: WireU64::new(tab_id),
        feed_integrity: (integrity.anomalies.get() > 0).then_some(integrity),
        active_loading_tasks: loading_tasks(tab.loading()),
        panes,
        tape: tape_health(tab),
    }
}

/// The kinds of work in flight, in [`LoadingTask::ALL`]'s order.
fn loading_tasks(loading: &LoadingTracker) -> Vec<LoadingTaskSnapshot> {
    LoadingTask::ALL
        .into_iter()
        .filter_map(|task| {
            let count = loading.count(task);
            (count > 0).then(|| LoadingTaskSnapshot {
                task: task.id().to_owned(),
                operation_count: wire_usize(count),
            })
        })
        .collect()
}

/// This tab's tape delay, split as far as its provider can tell.
///
/// `None` when there is nothing measured to report at all: a tab that has not
/// seen a live print, and any tab playing a recording.
fn tape_health(tab: &dyn HealthTab) -> Option<TapeHealthSnapshot> {
    let arrival_latency_ms = tab.arrival_latency_ms();
    let split = tab.latency_split();
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
        intake_lag: tab.intake_lag(),
    })
}

fn pane_health(pane: &PaneHealthRead<'_>) -> PaneHealthSnapshot {
    PaneHealthSnapshot::from_indicators(
        pane.pane_id,
        pane.side,
        pane.indicators
            .all()
            .iter()
            .map(|view| IndicatorHealthRead {
                slot_id: view.slot.0,
                kind: &view.kind,
                error_bar_index: view.error.as_ref().map(|error| error.bar_index),
                stale: view.stale.is_some(),
            }),
        pane.orderflow,
    )
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod tests;
