//! Candidate-only observer; the common timed harness stays byte-identical.
use crate::indicator_worker::IndicatorWorker;
use crate::orderflow_worker::BookWorker;
use crate::worker_progress::{Phase, ProgressSnapshot};

fn observe(snapshot: ProgressSnapshot, expected: &[u64]) -> serde_json::Value {
    assert!(snapshot.valid);
    assert_eq!(snapshot.phase, Phase::Idle);
    let c = snapshot.counts;
    assert!(
        expected.contains(&c.accepted),
        "independently specified command total"
    );
    assert_eq!(c.accepted, c.retired);
    assert_eq!(
        (
            c.queued,
            c.inflight,
            c.unfinished,
            c.failed_sends,
            c.output_failures
        ),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(c.output_attempts, c.output_successes);
    serde_json::to_value(snapshot).unwrap()
}
pub(crate) fn indicator_snapshot(worker: &IndicatorWorker) -> serde_json::Value {
    observe(worker.progress(), &[3, 40_953, 40_955])
}
pub(crate) fn book_snapshot(worker: &BookWorker) -> serde_json::Value {
    observe(worker.progress(), &[4, 82_534])
}
