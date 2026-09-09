//! Identical baseline/candidate diagnostic-cadence harness; no worker telemetry
//! API is called here, so the baseline can execute its existing summary path.
use super::*;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct CountingSink(Arc<Mutex<SinkState>>);
#[derive(Default)]
struct SinkState {
    bytes: u64,
    records: u64,
    probe: Option<Vec<u8>>,
}
impl SinkState {
    fn counts(&self) -> (u64, u64) {
        (self.bytes, self.records)
    }
}
impl Write for CountingSink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut counts = self.0.lock().unwrap();
        counts.bytes += bytes.len() as u64;
        counts.records += bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
        if let Some(probe) = &mut counts.probe {
            assert!(probe.len() + bytes.len() <= 65_536, "bounded untimed probe");
            probe.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
#[ignore = "manual exclusive-host Q8 diagnostic summary cost"]
fn worker_summary_cadence_cost() {
    const WARMUP: usize = 30;
    const MEASURED: usize = 600;
    let (mut app, _events, _commands, _book) = test_app();
    let (mut other, _other_events, _other_commands, _other_book) = test_app();
    app.tabs[0].id = 41;
    app.tabs[0].flow_pane.id = 901;
    app.tabs[0].time_panes.clear();
    let mut tab = other.tabs.remove(0);
    tab.id = 42;
    tab.flow_pane.id = 902;
    tab.time_panes.clear();
    tab.time_panes
        .push(crate::pane::ChartPane::time(900, 60_000));
    app.tabs.push(tab);
    assert_eq!(app.active_tab, 0);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(
        app.tabs
            .iter()
            .map(|tab| tab.panes().count())
            .sum::<usize>(),
        3
    );
    assert_eq!(
        app.tabs
            .iter()
            .flat_map(|tab| tab.panes())
            .filter(|(pane, _)| pane.orderflow.is_some())
            .count(),
        2
    );
    let expected_owners = serde_json::json!([
        {"tab":41,"pane":901,"side":0,"kind":"indicator"},
        {"tab":41,"pane":901,"side":0,"kind":"orderflow"},
        {"tab":42,"pane":902,"side":0,"kind":"indicator"},
        {"tab":42,"pane":902,"side":0,"kind":"orderflow"},
        {"tab":42,"pane":900,"side":1,"kind":"indicator"}
    ]);
    let counts = Arc::new(Mutex::new(SinkState::default()));
    let writer = CountingSink(counts.clone());
    let subscriber = tracing_subscriber::fmt()
        .json()
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let ctx = egui::Context::default();
    let mut elapsed_ns = Vec::with_capacity(MEASURED);
    let mut bytes_per_summary = Vec::with_capacity(MEASURED);
    let mut records_per_summary = Vec::with_capacity(MEASURED);
    let initial = app.health.last_summary;
    tracing::subscriber::with_default(subscriber, || {
        for index in 0..WARMUP + MEASURED {
            let before = counts.lock().unwrap().counts();
            let now = initial + Duration::from_secs(2 * (index as u64 + 1));
            let started = Instant::now();
            app.maybe_emit_summary(now, &ctx);
            let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap();
            let after = counts.lock().unwrap().counts();
            if index >= WARMUP {
                elapsed_ns.push(elapsed);
                bytes_per_summary.push(after.0 - before.0);
                records_per_summary.push(after.1 - before.1);
            }
        }
        // One bounded correctness probe, deliberately outside every timed sample.
        counts.lock().unwrap().probe = Some(Vec::new());
        app.maybe_emit_summary(
            initial + Duration::from_secs(2 * (WARMUP + MEASURED + 1) as u64),
            &ctx,
        );
    });
    let probe_jsonl = String::from_utf8(counts.lock().unwrap().probe.take().unwrap()).unwrap();
    assert!(records_per_summary.iter().all(|count| *count > 0));
    println!(
        "\nQ8_SUMMARY_BENCH {}",
        serde_json::json!({
            "fixture_id": "q8-summary-cadence-v1", "warmup": WARMUP, "measured": MEASURED,
            "tabs": 2, "panes": 3, "owned_workers": 5, "cadence_seconds": 2,
            "summary_ns": elapsed_ns, "bytes_per_summary": bytes_per_summary,
            "records_per_summary": records_per_summary,
            "probe_jsonl": probe_jsonl, "expected_owners": expected_owners,
            "scope": "actual summary reader and JSON counting sink; synthetic two-second timestamps, back-to-back calls; excludes asynchronous ResetSummaryCounters completion, disk IO and GUI frame cost"
        })
    );
}
