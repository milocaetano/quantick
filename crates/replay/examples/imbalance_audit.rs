//! Audit an imbalance-bar target against a recorded session.
//!
//! The imbalance rule is adaptive: how long its bars actually run depends on
//! the tape, not on the target alone, and the honest way to pick a target is
//! to measure it against the session you trade. This example replays a
//! recorded session straight through `ImbalanceBarBuilder` at one or more
//! targets and prints the closed-bar length distribution, plus the tape's
//! side-run structure that drives the closing speed.
//!
//! Usage:
//!   cargo run -p quantick-replay --release --example imbalance_audit -- \
//!     <session.csv> [target ...]        (targets default to 2000 5000)

use quantick_engine::{BarBuilder, ImbalanceBarBuilder, Side};
use quantick_replay::ParseOptions;
use quantick_replay::format::parse_file;

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: imbalance_audit <session.csv> [target ...]");
    let mut targets: Vec<u64> = args
        .map(|a| a.parse().expect("target must be u64"))
        .collect();
    if targets.is_empty() {
        targets = vec![2000, 5000];
    }

    let text = std::fs::read_to_string(&path).expect("read session file");
    let parsed = parse_file(&text, ParseOptions::default()).expect("parse session");
    let trades = parsed.trades;
    let n = trades.len();
    println!("== session {path}");
    println!(
        "side_source={:?}  trades={n}",
        parsed.header.side_source.as_deref().unwrap_or("?")
    );
    if n == 0 {
        return;
    }
    let span_min = (trades.last().unwrap().timestamp_ms - trades.first().unwrap().timestamp_ms)
        as f64
        / 60_000.0;

    // Tape structure: how one-sided and how run-heavy is this stream?
    let buys = trades.iter().filter(|t| t.side == Side::Buy).count();
    let mut same_side = 0usize;
    let mut runs: Vec<u64> = Vec::new();
    let mut run_len = 1u64;
    for w in trades.windows(2) {
        if w[1].side == w[0].side {
            same_side += 1;
            run_len += 1;
        } else {
            runs.push(run_len);
            run_len = 1;
        }
    }
    runs.push(run_len);
    let mean_run = n as f64 / runs.len() as f64;
    runs.sort_unstable();
    println!(
        "span={span_min:.0}min  buy_frac={:.3}  P(same side as prev)={:.3}  runs: mean={mean_run:.1} p50={} p90={} max={}",
        buys as f64 / n as f64,
        same_side as f64 / (n - 1) as f64,
        percentile(&runs, 0.5),
        percentile(&runs, 0.9),
        runs.last().unwrap(),
    );

    for target in &targets {
        let target = *target;
        let mut b = ImbalanceBarBuilder::new(target);
        let mut counts: Vec<u64> = Vec::new();
        let mut durs_ms: Vec<u64> = Vec::new();
        let mut cap_closes = 0usize;
        for t in &trades {
            if let Some(bar) = b.push(t) {
                counts.push(bar.trade_count);
                durs_ms.push((bar.close_time - bar.open_time).max(0) as u64);
                if bar.trade_count >= 3 * target {
                    cap_closes += 1;
                }
            }
        }
        let bars = counts.len();
        if bars == 0 {
            println!("-- target={target}: no closed bars (partial only)");
            continue;
        }
        let warmup = counts[0];
        let adaptive = &counts[1..];
        let mut sorted: Vec<u64> = adaptive.to_vec();
        sorted.sort_unstable();
        durs_ms.sort_unstable();
        let mean = if adaptive.is_empty() {
            0.0
        } else {
            adaptive.iter().sum::<u64>() as f64 / adaptive.len() as f64
        };
        println!(
            "-- target={target}: bars={bars} (warmup={warmup})  bars/min={:.1}",
            bars as f64 / span_min
        );
        println!(
            "   trades/bar after warmup: mean={mean:.0} min={} p10={} p50={} p90={} max={}  cap_closes={cap_closes} ({:.1}%)",
            sorted.first().copied().unwrap_or(0),
            percentile(&sorted, 0.10),
            percentile(&sorted, 0.50),
            percentile(&sorted, 0.90),
            sorted.last().copied().unwrap_or(0),
            100.0 * cap_closes as f64 / bars as f64,
        );
        println!(
            "   bar duration: p50={:.1}s p90={:.1}s max={:.1}s",
            percentile(&durs_ms, 0.50) as f64 / 1000.0,
            percentile(&durs_ms, 0.90) as f64 / 1000.0,
            durs_ms.last().copied().unwrap_or(0) as f64 / 1000.0,
        );
    }
}
