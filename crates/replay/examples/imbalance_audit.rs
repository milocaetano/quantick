//! Audit an imbalance-bar target against a recorded session.
//!
//! The imbalance rule is adaptive: how long its bars actually run depends on
//! the tape, not on the target alone, and the honest way to pick a target is
//! to measure it against the session you trade. This example replays a
//! recorded session straight through `ImbalanceBarBuilder` at one or more
//! targets — in any of the three units — and prints the closed-bar length
//! distribution, plus the tape's side-run structure that drives the closing
//! speed.
//!
//! Usage:
//!   cargo run -p quantick-replay --release --example imbalance_audit -- \
//!     <session.csv> [[unit:]target ...]
//!
//! `unit` is the spec token (`trades`, `volume`, `dollar`), defaulting to
//! `trades`; bare targets default to `2000 5000`. `volume:2000` audits the
//! same target in volume imbalance bars.

use quantick_engine::{BarBuilder, ImbalanceBarBuilder, ImbalanceUnit, Side};
use quantick_replay::ParseOptions;
use quantick_replay::format::parse_file;

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

fn parse_target(arg: &str) -> (ImbalanceUnit, u64) {
    let (unit, target) = match arg.split_once(':') {
        None => (ImbalanceUnit::Trades, arg),
        Some((token, target)) => (
            ImbalanceUnit::parse_token(token).expect("unit must be one of trades, volume, dollar"),
            target,
        ),
    };
    (unit, target.parse().expect("target must be u64"))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: imbalance_audit <session.csv> [[unit:]target ...]");
    let mut targets: Vec<(ImbalanceUnit, u64)> = args.map(|a| parse_target(&a)).collect();
    if targets.is_empty() {
        targets = vec![(ImbalanceUnit::Trades, 2000), (ImbalanceUnit::Trades, 5000)];
    }

    let text = std::fs::read_to_string(&path).expect("read session file");
    let parsed = parse_file(&text, ParseOptions::default()).expect("parse session");
    let trades = parsed.trades;
    let n = trades.len();
    println!("== session {path}");
    println!(
        "side_source={}  trades={n}",
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
    // A one-trade session has no previous side to compare against; print the
    // fact instead of the NaN the division would produce.
    let same_side_frac = if n >= 2 {
        format!("{:.3}", same_side as f64 / (n - 1) as f64)
    } else {
        "n/a".to_owned()
    };
    println!(
        "span={span_min:.0}min  buy_frac={:.3}  P(same side as prev)={same_side_frac}  runs: mean={mean_run:.1} p50={} p90={} max={}",
        buys as f64 / n as f64,
        percentile(&runs, 0.5),
        percentile(&runs, 0.9),
        runs.last().unwrap(),
    );

    for (unit, target) in &targets {
        let (unit, target) = (*unit, *target);
        let mut b = ImbalanceBarBuilder::with_unit(target, unit);
        let cap = b.hard_cap_trades();
        let mut counts: Vec<u64> = Vec::new();
        let mut durs_ms: Vec<u64> = Vec::new();
        let mut cap_closes = 0usize;
        for t in &trades {
            if let Some(bar) = b.push(t) {
                counts.push(bar.trade_count);
                // The warm-up bar answers a fixed rule, not the adaptive one;
                // keep every statistic below to the same post-warm-up
                // population.
                if counts.len() > 1 {
                    durs_ms.push((bar.close_time - bar.open_time).max(0) as u64);
                    if bar.trade_count >= cap {
                        cap_closes += 1;
                    }
                }
            }
        }
        let bars = counts.len();
        let label = match unit {
            ImbalanceUnit::Trades => format!("{target}"),
            _ => format!("{}:{target}", unit.as_str()),
        };
        if bars == 0 {
            println!("-- target={label}: no closed bars (partial only)");
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
        // A sub-minute recording makes a per-minute rate meaningless; say so
        // rather than printing `inf`.
        let bars_per_min = if span_min > 0.0 {
            format!("{:.1}", adaptive.len() as f64 / span_min)
        } else {
            "n/a".to_owned()
        };
        println!("-- target={label}: bars={bars} (warmup={warmup})  bars/min={bars_per_min}");
        println!(
            "   trades/bar after warmup: mean={mean:.0} min={} p10={} p50={} p90={} max={}  cap_closes={cap_closes} ({:.1}%)",
            sorted.first().copied().unwrap_or(0),
            percentile(&sorted, 0.10),
            percentile(&sorted, 0.50),
            percentile(&sorted, 0.90),
            sorted.last().copied().unwrap_or(0),
            if adaptive.is_empty() {
                0.0
            } else {
                100.0 * cap_closes as f64 / adaptive.len() as f64
            },
        );
        println!(
            "   bar duration: p50={:.1}s p90={:.1}s max={:.1}s",
            percentile(&durs_ms, 0.50) as f64 / 1000.0,
            percentile(&durs_ms, 0.90) as f64 / 1000.0,
            durs_ms.last().copied().unwrap_or(0) as f64 / 1000.0,
        );
    }
}
