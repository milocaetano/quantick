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

#[test]
fn a_gap_is_logged_with_structured_fields() {
    let out = capture(|| {
        let mut t = ContinuityTracker::new();
        t.observe(&trade(1, 10));
        t.observe(&trade(5, 20)); // gap: 2,3,4 missing
    });

    assert!(out.contains("quantick::feed"), "target missing: {out}");
    assert!(out.contains("aggTrade gap"), "message missing: {out}");
    assert!(out.contains("expected_agg_id=2"), "field missing: {out}");
    assert!(out.contains("got_agg_id=5"), "field missing: {out}");
    assert!(out.contains("missing=3"), "field missing: {out}");
}

#[test]
fn a_backwards_timestamp_is_logged() {
    let out = capture(|| {
        let mut t = ContinuityTracker::new();
        t.observe(&trade(1, 100));
        t.observe(&trade(2, 90)); // timestamp goes backwards
    });
    assert!(out.contains("backwards"), "message missing: {out}");
    assert!(out.contains("last_ms=100"), "field missing: {out}");
    assert!(out.contains("got_ms=90"), "field missing: {out}");
}
