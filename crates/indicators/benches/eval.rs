//! Indicator hot-path benchmark: kernel pushes, host commit runs and host
//! previews over a large deterministic bar burst.
//!
//! The §5 budgets this guards: commit run ≤ 50 µs and preview ≤ 60 µs per
//! typical indicator (native ones must sit orders of magnitude under —
//! they are the performance reference scripts are compared against), full
//! recompute non-blocking. Dependency-free like `engine/benches/hot_path.rs`
//! (no criterion); run with `cargo bench -p quantick-indicators`. The
//! workload is fully deterministic, so numbers are comparable across
//! commits; report them in PR descriptions.

use std::hint::black_box;
use std::time::Instant;

use quantick_engine::Bar;
use quantick_indicators::{
    IndicatorHost, SourceId,
    native::{Cvd, Ema},
    ta,
};
use rust_decimal::Decimal;

/// A deterministic burst of `n` closed bars wiggling around a 36000 base.
fn make_bars(n: usize) -> Vec<Bar> {
    let base = Decimal::from(36_000);
    let mut bars = Vec::with_capacity(n);
    for i in 0..n {
        let tick = (i % 100) as i64 - 50;
        let close = base + Decimal::new(tick, 1);
        let open = base + Decimal::new((i % 7) as i64 - 3, 1);
        let t0 = 1_700_000_000_000 + (i as i64) * 250;
        bars.push(Bar {
            open_time: t0,
            close_time: t0 + 249,
            open,
            high: open.max(close) + Decimal::new(5, 2),
            low: open.min(close) - Decimal::new(5, 2),
            close,
            buy_volume: Decimal::new(((i % 900) + 1) as i64, 3),
            sell_volume: Decimal::new(((i % 700) + 1) as i64, 3),
            trade_count: (i % 50 + 1) as u64,
        });
    }
    bars
}

fn report(name: &str, n: usize, elapsed: std::time::Duration) {
    let ns_per = elapsed.as_secs_f64() * 1e9 / n as f64;
    println!("{name:26} {n:>8} iters in {elapsed:>10.2?}  =>  {ns_per:9.1} ns/iter");
}

fn bench_kernels(n: usize) {
    // Raw kernel throughput on a synthetic price series.
    let xs: Vec<f64> = (0..n)
        .map(|i| 36_000.0 + ((i % 100) as f64 - 50.0) / 10.0)
        .collect();

    let mut ema = ta::Ema::new(21);
    let start = Instant::now();
    for &x in &xs {
        black_box(ema.push(black_box(x)));
    }
    report("ta::Ema(21)::push", n, start.elapsed());

    let mut sma = ta::Sma::new(50);
    let start = Instant::now();
    for &x in &xs {
        black_box(sma.push(black_box(x)));
    }
    report("ta::Sma(50)::push", n, start.elapsed());

    let mut highest = ta::Highest::new(50);
    let start = Instant::now();
    for &x in &xs {
        black_box(highest.push(black_box(x)));
    }
    report("ta::Highest(50)::push", n, start.elapsed());

    let mut rsi = ta::Rsi::new(14);
    let start = Instant::now();
    for &x in &xs {
        black_box(rsi.push(black_box(x)));
    }
    report("ta::Rsi(14)::push", n, start.elapsed());
}

fn bench_host(bars: &[Bar]) {
    // The commit path a live chart pays per closed bar: 1 projection + 2
    // native indicators (EMA overlay + CVD pane).
    let mut host = IndicatorHost::new();
    host.add(Box::new(Ema::new(21, SourceId::Close)));
    host.add(Box::new(Cvd::new()));
    let start = Instant::now();
    for bar in bars {
        host.push_closed_bar(black_box(bar));
    }
    report("host commit (EMA+CVD)", bars.len(), start.elapsed());

    // The preview path, re-run per worker batch while a bar forms.
    let partial = bars.last().expect("bars are non-empty").clone();
    let previews = 100_000;
    let start = Instant::now();
    for _ in 0..previews {
        host.set_partial(Some(black_box(&partial)));
    }
    report("host preview (EMA+CVD)", previews, start.elapsed());

    // Full recompute: the §5 budget says 10 scripts x 5000 bars <= 1.5 s;
    // 2 native indicators over the full burst approximate one script's
    // recompute share.
    let start = Instant::now();
    host.rebuild(bars, None);
    let elapsed = start.elapsed();
    println!(
        "host rebuild (EMA+CVD)     {n:>8} bars  in {elapsed:>10.2?}",
        n = bars.len()
    );
}

fn main() {
    let n = 100_000;
    println!("indicator eval benchmark: {n} deterministic bars\n");
    let bars = make_bars(n);
    bench_kernels(n);
    bench_host(&bars);
}
