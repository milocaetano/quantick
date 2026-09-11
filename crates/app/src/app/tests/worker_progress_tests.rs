use super::*;
use crate::indicator_worker::{IndicatorCommand, IndicatorWorker};
use crate::orderflow_worker::{BookCommand, BookWorker};
use crate::worker_progress::{Phase, WorkerProgress, tests::Gate};
use std::io::Write;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct LogWriter(Arc<Mutex<Vec<u8>>>);
impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn summary_cadence_and_counter_reset_survive_disabled_logging() {
    let (mut app, _events, _commands, _book) = test_app();
    let initial = app.health.last_summary;
    let ctx = egui::Context::default();
    app.health.trades_since_summary = 7;
    tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::default(), || {
        app.maybe_emit_summary(initial + Duration::from_millis(1999), &ctx);
        assert_eq!(app.health.last_summary, initial);
        assert_eq!(app.health.trades_since_summary, 7);
        app.maybe_emit_summary(initial + Duration::from_secs(2), &ctx);
        assert_eq!(app.health.last_summary, initial + Duration::from_secs(2));
        assert_eq!(app.health.trades_since_summary, 0);
    });
}

#[test]
fn existing_summary_entrypoint_emits_owned_normal_degraded_and_recovered_workers() {
    // Two tabs; tab 42 stays off-screen. Its flow indicator and book are held
    // with one in-flight command each, then one queued Flush. Explicit time
    // advances 0 -> 10 (queue) -> 40 (observe) -> release. No frame or sleeps.
    let (mut app, _events, _commands, _book) = test_app();
    let (mut other, _other_events, _other_commands, _other_book) = test_app();
    let mut tab = other.tabs.remove(0);
    tab.id = 42;
    tab.time_panes
        .push(crate::pane::ChartPane::time(900, 60_000));
    let indicator_clock = Gate::new();
    let indicator_hold = indicator_clock.hold(Phase::Applying);
    let book_clock = Gate::new();
    let book_hold = book_clock.hold(Phase::Applying);
    let (indicator, run_indicator) =
        IndicatorWorker::prepared_for_test(WorkerProgress::with_clock(indicator_clock.clone()));
    tab.flow_pane.indicator_worker = indicator;
    let (book, run_book) =
        BookWorker::prepared_for_test("TESTUSDT", WorkerProgress::with_clock(book_clock.clone()));
    let (book_first_tx, book_first_rx) = channel();
    let (book_ack_tx, book_ack_rx) = channel();
    app.tabs.push(tab);
    let flow_id = app.tabs[1].flow_pane.id;
    let indicator_id = app.tabs[1]
        .flow_pane
        .indicator_worker
        .progress()
        .instance
        .unwrap();
    let book_id = book.progress().instance.unwrap();
    let logs = Arc::new(Mutex::new(Vec::new()));
    let writer = LogWriter(logs.clone());
    let subscriber = tracing_subscriber::fmt()
        .json()
        .without_time()
        .with_writer(move || writer.clone())
        .finish();
    let ctx = egui::Context::default();
    let (first_tx, first_rx) = channel();
    let (ack_tx, ack_rx) = channel();
    // Before work, insert the actual BookWorker into the ordinary view owner.
    book.send(BookCommand::Flush(book_first_tx));
    let book_thread = std::thread::spawn(run_book);
    book_hold.reached();
    book_clock.at(10);
    book.send(BookCommand::Flush(book_ack_tx));
    book_clock.at(40);
    app.tabs[1].flow_pane.orderflow = Some(
        crate::orderflow_view::OrderflowView::with_worker_for_test("TESTUSDT", book),
    );
    let indicator_thread = tracing::subscriber::with_default(subscriber, || {
        let normal_at = app.health.last_summary + Duration::from_secs(2);
        app.maybe_emit_summary(normal_at, &ctx);
        let worker = &app.tabs[1].flow_pane.indicator_worker;
        worker.send(IndicatorCommand::Flush(first_tx));
        let thread = std::thread::spawn(run_indicator);
        indicator_hold.reached();
        indicator_clock.at(10);
        worker.send(IndicatorCommand::Flush(ack_tx));
        indicator_clock.at(40);
        app.maybe_emit_summary(normal_at + Duration::from_secs(2), &ctx);
        indicator_hold.release();
        book_hold.release();
        for ack in [first_rx, ack_rx, book_first_rx, book_ack_rx] {
            ack.recv_timeout(Duration::from_secs(10))
                .expect("real workers drained");
        }
        app.maybe_emit_summary(normal_at + Duration::from_secs(4), &ctx);
        thread
    });
    let transcript = String::from_utf8(logs.lock().unwrap().clone()).unwrap();
    println!("Q8_APP_DIAGNOSTIC_TRANSCRIPT\n{transcript}");
    let rows: Vec<serde_json::Value> = transcript
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        rows.iter()
            .filter(|row| row["fields"]["event_code"] == "APP_HEALTH_SUMMARY")
            .count(),
        3
    );
    let worker_rows: Vec<_> = rows
        .iter()
        .filter(|row| row["fields"]["event_code"] == "APP_WORKER_PROGRESS")
        .collect();
    let owners = app
        .tabs
        .iter()
        .flat_map(|t| t.panes())
        .map(|(p, _)| 1 + usize::from(p.orderflow.is_some()))
        .sum::<usize>();
    assert_eq!(worker_rows.len(), 3 * owners);
    for (kind, instance) in [("indicator", indicator_id), ("orderflow", book_id)] {
        let matching: Vec<_> = worker_rows
            .iter()
            .filter(|row| row["fields"]["instance"] == instance)
            .collect();
        assert_eq!(matching.len(), 3);
        for row in &matching {
            let fields = &row["fields"];
            assert_eq!(fields["schema_version"], 1);
            assert_eq!(fields["tab"], 42);
            assert_eq!(fields["pane"], flow_id);
            assert_eq!(fields["side"], 0);
            assert_eq!(fields["kind"], kind);
            assert_eq!(fields["clock_unit"], "ns");
        }
        let observed: Vec<serde_json::Value> = matching
            .iter()
            .map(|row| {
                serde_json::from_str(row["fields"]["observation"].as_str().unwrap()).unwrap()
            })
            .collect();
        assert_eq!(observed[1]["phase"], "applying");
        assert_eq!(observed[1]["counts"]["queued"], 1);
        assert_eq!(observed[1]["counts"]["inflight"], 1);
        assert_eq!(
            observed[1]["oldest_wait"],
            serde_json::json!({"state":"known", "ns":30})
        );
        assert_eq!(observed[2]["phase"], "idle");
        assert_eq!(observed[2]["counts"]["retired"], 2);
        assert_eq!(observed[2]["counts"]["queued"], 0);
        assert_eq!(observed[2]["counts"]["inflight"], 0);
        if kind == "indicator" {
            assert_eq!(observed[0]["phase"], "idle");
            assert_eq!(observed[0]["counts"]["accepted"], 0);
        }
    }
    assert!(
        worker_rows
            .iter()
            .any(|row| row["fields"]["pane"] == 900 && row["fields"]["side"] == 1)
    );
    drop(app);
    indicator_thread.join().expect("summary indicator closed");
    book_thread.join().expect("summary book closed");
}
