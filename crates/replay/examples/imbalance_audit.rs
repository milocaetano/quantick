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
//!     <session.csv> [--tick-rule] [--hourly] [[unit:]target ...]
//!
//! `unit` is the spec token (`trades`, `volume`, `dollar`), defaulting to
//! `trades`; bare targets default to `2000 5000`. `volume:2000` audits the
//! same target in volume imbalance bars.
//!
//! `--tick-rule` re-derives every aggressor side from the recorded prices the
//! way [`SideMode::TickRule`] does live, instead of trusting the sides in the
//! file. A recording made from venue flags and the same symbol traded live
//! under the tick rule are two different tapes, and the bar rule reads the
//! side stream — so auditing a target for a live MT5 chart has to audit the
//! side policy that chart actually runs.
//!
//! `--hourly` breaks the same statistics down by session hour. A trader does
//! not see a whole day at once; a rule that averages out over a session can
//! still be unusable inside the window on screen.

use quantick_engine::{BarBuilder, ImbalanceBarBuilder, ImbalanceUnit, Side, Trade};
use quantick_replay::ParseOptions;
use quantick_replay::format::parse_file;

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

/// Re-label every trade's side with López de Prado's tick rule — uptick buy,
/// downtick sell, unchanged carries the previous side — the policy
/// `feed-mt5`'s `SideMode::TickRule` runs live and the MT5 default.
///
/// Trades before the first price move have no side to infer; the live mapper
/// drops them, so this drops them too rather than inventing one.
///
/// **A deliberate second copy, and the reason it is one.** `feed-mt5`'s
/// `map::tick_rule` calls itself "the one place the rule lives", and it is
/// right to: this example cannot call it, because `replay` depending on
/// `feed-mt5` would be a reverse edge (feeds are producers, and no domain
/// crate links one). The rule is pure, deterministic and clock-free, so it
/// belongs in `engine` with both callers delegating — a port extraction worth
/// its own change, not a drive-by here.
///
/// Until then, the divergence to know about: the live mapper rejects bad
/// prices, zero volumes and missing quotes *before* recording `prev_price`,
/// so a rejected tick does not move the reference. This copy advances
/// `prev_price` on every row, which is exact for a parsed recording — every
/// row already survived `parse_file`'s validation — and would not be for a
/// raw tick stream.
fn relabel_tick_rule(trades: Vec<Trade>) -> Vec<Trade> {
    let mut prev_price = None;
    let mut prev_side = None;
    let mut out = Vec::with_capacity(trades.len());
    for mut t in trades {
        let side = match prev_price {
            Some(prev) if t.price > prev => Some(Side::Buy),
            Some(prev) if t.price < prev => Some(Side::Sell),
            Some(_) => prev_side,
            None => None,
        };
        prev_price = Some(t.price);
        prev_side = side;
        if let Some(side) = side {
            t.side = side;
            out.push(t);
        }
    }
    out
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
    let path = args.next().expect(
        "usage: imbalance_audit <session.csv> [--tick-rule] [--hourly] [[unit:]target ...]",
    );
    let rest: Vec<String> = args.collect();
    let tick_rule = rest.iter().any(|a| a == "--tick-rule");
    let hourly = rest.iter().any(|a| a == "--hourly");
    // Reject unknown flags rather than ignoring them. A silently swallowed
    // `--tickrule` would print a full, confident report built on the
    // recording's venue sides — a different tape from the live one — and this
    // is the tool a trader picks a live target with.
    if let Some(bad) = rest
        .iter()
        .find(|a| a.starts_with("--") && a != &"--tick-rule" && a != &"--hourly")
    {
        panic!("unknown flag {bad}; expected --tick-rule or --hourly");
    }
    let mut targets: Vec<(ImbalanceUnit, u64)> = rest
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|a| parse_target(a))
        .collect();
    if targets.is_empty() {
        targets = vec![(ImbalanceUnit::Trades, 2000), (ImbalanceUnit::Trades, 5000)];
    }

    let text = std::fs::read_to_string(&path).expect("read session file");
    let parsed = parse_file(&text, ParseOptions::default()).expect("parse session");
    let recorded_source = parsed
        .header
        .side_source
        .as_deref()
        .unwrap_or("?")
        .to_owned();
    let recorded_len = parsed.trades.len();
    let trades = if tick_rule {
        relabel_tick_rule(parsed.trades)
    } else {
        parsed.trades
    };
    let n = trades.len();
    println!("== session {path}");
    if tick_rule {
        println!(
            "side_source={recorded_source} -> tick_rule (re-derived)  trades={n} \
             (of {recorded_len}; {} dropped before the first price move)",
            recorded_len - n
        );
    } else {
        println!("side_source={recorded_source}  trades={n}");
    }
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

    // Depends on the header alone, so it is read once rather than per target.
    let tz_ms = parsed.header.timezone.millis();

    for (unit, target) in &targets {
        let (unit, target) = (*unit, *target);
        let mut b = ImbalanceBarBuilder::with_unit(target, unit);
        let cap = b.hard_cap_trades();
        let mut counts: Vec<u64> = Vec::new();
        let mut durs_ms: Vec<u64> = Vec::new();
        let mut cap_closes = 0usize;
        // Per clock hour, in the recording's own timezone: (bars, trades in
        // them, bars that hit the cap). A `BTreeMap` so the report comes out
        // in session order whatever the tape did. Only filled when `--hourly`
        // asks for it — otherwise the whole map is built and thrown away once
        // per target over a session of hundreds of thousands of rows.
        let mut by_hour: std::collections::BTreeMap<i64, (u64, u64, u64)> =
            std::collections::BTreeMap::new();
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
                    if hourly {
                        // Saturating like every other timestamp path in the
                        // crate: `parse_file` builds `timestamp_ms` with
                        // saturating arithmetic, so an absurd `Date` column
                        // can reach `i64::MAX` and a plain `+` on an
                        // east-of-UTC offset would panic a debug build.
                        let hour = bar
                            .close_time
                            .saturating_add(tz_ms)
                            .div_euclid(3_600_000)
                            .rem_euclid(24);
                        let slot = by_hour.entry(hour).or_insert((0, 0, 0));
                        slot.0 += 1;
                        slot.1 += bar.trade_count;
                        slot.2 += u64::from(bar.trade_count >= cap);
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
        if hourly {
            // What the trader actually sees: one hour on screen, not a day
            // averaged. A target whose bar length swings between hours is a
            // different chart every hour at the same setting.
            for (hour, (bars, trades_in, capped)) in &by_hour {
                println!(
                    "   h{hour:02}: bars={bars:<5} trades/bar={:<7.0} cap_closes={capped} ({:.0}%)",
                    *trades_in as f64 / *bars as f64,
                    100.0 * *capped as f64 / *bars as f64,
                );
            }
        }
    }
}
