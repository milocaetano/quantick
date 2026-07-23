//! Verifies that a continuity anomaly produces a structured, labelled tracing
//! event — the "a log excerpt alone explains what happened" requirement.

use std::sync::{Arc, Mutex};

use quantick_engine::{Side, Trade};
use quantick_feed_binance::ContinuityTracker;
use rust_decimal::Decimal;
use tracing_subscriber::fmt::MakeWriter;

/// A `MakeWriter` that appends every log line to a shared buffer.
#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuf {
    type Writer = SharedBuf;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn trade(agg_id: u64, ts: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: ts,
        price: Decimal::ONE,
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn capture(body: impl FnOnce()) -> String {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(SharedBuf(buf.clone()))
        .with_max_level(tracing::Level::WARN)
        .finish();
    tracing::subscriber::with_default(subscriber, body);
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

// Both anomalies are checked in ONE test on purpose. The two checks share the
// `warn!` callsite, and `tracing` caches callsite interest globally; running two
// capture tests in parallel races on that cache (concurrent `with_default`
// swaps can cache the callsite as "not interested" and drop the events). A
// single sequential test touches the callsite from one thread only, so it's
// deterministic. See #40.
#[test]
fn anomalies_are_logged_with_structured_fields() {
    // A gap: 2,3,4 missing between agg_id 1 and 5.
    let gap = capture(|| {
        let mut t = ContinuityTracker::new();
        t.observe(&trade(1, 10));
        t.observe(&trade(5, 20));
    });
    assert!(gap.contains("quantick::feed"), "target missing: {gap}");
    assert!(gap.contains("aggTrade gap"), "message missing: {gap}");
    assert!(gap.contains("expected_agg_id=2"), "field missing: {gap}");
    assert!(gap.contains("got_agg_id=5"), "field missing: {gap}");
    assert!(gap.contains("missing=3"), "field missing: {gap}");

    // A backwards timestamp: 90 arrives after 100.
    let backwards = capture(|| {
        let mut t = ContinuityTracker::new();
        t.observe(&trade(1, 100));
        t.observe(&trade(2, 90));
    });
    assert!(
        backwards.contains("backwards"),
        "message missing: {backwards}"
    );
    assert!(
        backwards.contains("last_ms=100"),
        "field missing: {backwards}"
    );
    assert!(
        backwards.contains("got_ms=90"),
        "field missing: {backwards}"
    );
}
