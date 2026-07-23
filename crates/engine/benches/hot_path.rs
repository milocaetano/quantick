//! Hot-path benchmark: how fast can each builder ingest trades?
//!
//! Tick charts update at high frequency, so the engine must keep up with live
//! trade bursts. This benchmark pushes a large, deterministic synthetic burst
//! through every builder and reports per-trade throughput. It is intentionally
//! dependency-free (no criterion) to keep the engine's dep tree small and CI
//! compile fast; run it with `cargo bench -p quantick-engine`.
//!
//! The workload is fully deterministic (derived from the trade index, no rng or
//! wall clock affects its *shape*), so throughput is comparable across commits.
//! A meaningful drop between runs is a regression to investigate as a bug.

use std::hint::black_box;
use std::time::Instant;

use quantick_engine::{
    BarBuilder, DollarBarBuilder, ImbalanceBarBuilder, Side, TickBarBuilder, TimeBarBuilder, Trade,
    VolumeBarBuilder,
};
use rust_decimal::Decimal;

/// Build a deterministic burst of `n` trades around a 36000 base price.
///
/// Generation happens outside the timed region, so only the push loop — the
/// real hot path — is measured.
fn make_trades(n: usize) -> Vec<Trade> {
    let base = Decimal::from(36_000);
    let mut trades = Vec::with_capacity(n);
    for i in 0..n {
        // Price wiggles in a deterministic ±5.00 band in 0.10 steps.
        let tick = (i % 100) as i64 - 50;
        let price = base + Decimal::new(tick, 1);
        // Quantity cycles 0.001 .. 1.000.
        let quantity = Decimal::new(((i % 1000) + 1) as i64, 3);
        let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
        trades.push(Trade {
            agg_id: i as u64,
            timestamp_ms: 1_700_000_000_000 + i as i64,
            price,
            quantity,
            side,
        });
    }
    trades
}

/// Time pushing every trade through `builder`, printing throughput.
fn bench<B: BarBuilder>(name: &str, mut builder: B, trades: &[Trade]) {
    let start = Instant::now();
    let mut closed = 0usize;
    for trade in trades {
        if builder.push(black_box(trade)).is_some() {
            closed += 1;
        }
    }
    let elapsed = start.elapsed();
    let millions_per_sec = trades.len() as f64 / elapsed.as_secs_f64() / 1e6;
    let ns_per = elapsed.as_secs_f64() * 1e9 / trades.len() as f64;
    println!(
        "{name:14} {n} trades in {elapsed:>10.2?}  =>  {millions_per_sec:6.1} M trades/s  {ns_per:6.1} ns/trade  ({closed} bars)",
        n = trades.len(),
    );
    black_box(closed);
}

fn main() {
    let n = 5_000_000;
    println!("hot-path benchmark: {n} deterministic trades per builder\n");
    let trades = make_trades(n);

    bench("tick(100)", TickBarBuilder::new(100), &trades);
    bench(
        "volume(50)",
        VolumeBarBuilder::new(Decimal::from(50)),
        &trades,
    );
    bench(
        "dollar(1e7)",
        DollarBarBuilder::new(Decimal::from(10_000_000)),
        &trades,
    );
    bench("time(1000ms)", TimeBarBuilder::new(1000), &trades);
    bench("imbalance(100)", ImbalanceBarBuilder::new(100), &trades);
}
