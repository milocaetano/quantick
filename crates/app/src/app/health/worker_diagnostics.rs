//! Internal owner attribution at the existing health-summary cadence.
use crate::pane::ChartPane;
use crate::tab::Tab;
use crate::worker_progress::ProgressSnapshot;

// Preserve the existing health-summary sampling cadence for every worker owner.
const SUMMARY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

pub(super) fn emit_if_due(tabs: &[Tab], elapsed: std::time::Duration) -> bool {
    if elapsed < SUMMARY_INTERVAL {
        return false;
    }
    if !tracing::enabled!(target: "quantick::app", tracing::Level::INFO) {
        return true;
    }
    for tab in tabs {
        for (pane, side) in tab.panes() {
            for (kind, progress) in pane_workers(pane) {
                record(tab.id, pane.id, side.index(), kind, progress);
            }
        }
    }
    true
}

/// Every worker a pane owns, by kind, with its progress now. The one list
/// both this diagnostic and the live-envelope figures read, so a worker added
/// to a pane cannot be observed by one and missed by the other.
pub(super) fn pane_workers(
    pane: &ChartPane,
) -> impl Iterator<Item = (&'static str, ProgressSnapshot)> + '_ {
    std::iter::once(("indicator", pane.indicator_worker.progress())).chain(
        pane.orderflow
            .as_ref()
            .map(|view| ("orderflow", view.worker_progress())),
    )
}

fn record(tab: u64, pane: u64, side: usize, kind: &str, progress: ProgressSnapshot) {
    // Serialization happens only every two seconds, after releasing the ledger.
    // An internal JSON payload preserves typed unavailable/invalid age readings.
    let observation = serde_json::to_string(&progress).expect("worker observation is JSON data");
    tracing::info!(target: "quantick::app", schema_version = 1_u8,
        event_code = "APP_WORKER_PROGRESS", tab, pane, side, kind,
        instance = progress.instance, clock_unit = "ns",
        observation = %observation, "owned worker progress");
}
