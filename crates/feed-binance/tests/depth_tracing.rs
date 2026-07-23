//! Stable, structured depth diagnostics for AI-assisted troubleshooting.

use std::sync::{Arc, Mutex};

use quantick_feed_binance::depth::{DepthLevel, DepthSnapshot, DepthSynchronizer, DepthUpdate};
use rust_decimal::Decimal;
use tracing_subscriber::fmt::MakeWriter;

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

fn level(price: i64, quantity: i64) -> DepthLevel {
    DepthLevel {
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
    }
}

fn update(first: u64, final_id: u64) -> DepthUpdate {
    DepthUpdate {
        event_time_ms: 1_000,
        symbol: "BTCUSDT".to_string(),
        first_update_id: first,
        final_update_id: final_id,
        bids: Vec::new(),
        asks: Vec::new(),
    }
}

#[test]
fn sync_lifecycle_and_gap_have_stable_machine_fields() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(SharedBuf(bytes.clone()))
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        let mut sync = DepthSynchronizer::new("BTCUSDT", 77);
        sync.install_snapshot(
            &DepthSnapshot {
                last_update_id: 100,
                bids: vec![level(99, 2)],
                asks: vec![level(101, 3)],
            },
            5_000,
        )
        .unwrap();
        sync.apply(&update(101, 101)).unwrap();
        let _ = sync.apply(&update(103, 104));
    });

    let logs = String::from_utf8(bytes.lock().unwrap().clone()).unwrap();
    for field in [
        "schema_version=1",
        "event_code=\"depth_snapshot_installed\"",
        "event_code=\"depth_stream_bridged\"",
        "event_code=\"depth_sequence_gap\"",
        "symbol=\"BTCUSDT\"",
        "generation=77",
        "current_update_id=101",
        "expected_update_id=102",
        "first_update_id=103",
        "final_update_id=104",
        "action=\"resync\"",
    ] {
        assert!(logs.contains(field), "missing {field:?} in logs:\n{logs}");
    }
}
