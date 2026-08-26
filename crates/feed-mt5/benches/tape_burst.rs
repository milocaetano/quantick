//! What one print costs on the way from the wire to a `Trade`.
//!
//! The latency split added work to the per-print path — one subtraction and one
//! comparison per print, plus a wider tick line to decode — and "that is
//! obviously cheap" is a belief, not a measurement. This is the measurement.
//!
//! It drives the real decoder over a synthetic burst: `parse_line` on lines
//! shaped exactly like the bridge's, `sent_ms` included, then `TickMapper::map`,
//! then `LatencyTracker::observe_live`. That is the whole of what runs per
//! print, and all this claims to measure.
//!
//! **What it deliberately leaves out**, so the number is not read as more than
//! it is: there is no channel here, so the `try_send` that replaced a `send` in
//! `stream.rs` is not timed — on the happy path the two are the same call shape
//! and the difference is not resolvable against a ~650 ns decode. And
//! `LatencyTracker::sample` is drawn once per run rather than once per
//! `SAMPLE_EVERY_PRINTS`, because its cost is a system clock read that belongs
//! to the sampling rate, not to a print. Both are stated rather than folded in.
//!
//! Dependency-free and `harness = false`, like `engine/benches/hot_path.rs`:
//! the workload is deterministic (derived from the print index, no rng and no
//! wall clock in its *shape*), so the figure is comparable across commits and a
//! meaningful drop between two of them is a regression to investigate.
//!
//! ```sh
//! cargo bench -p quantick-feed-mt5
//! ```
//!
//! To compare against a branch that has no tracker, delete the `observe_live`
//! call and the `LatencyTracker` beside it — the rest compiles unchanged,
//! because `sent_ms` is an additive field a decoder without it simply ignores.

use std::hint::black_box;
use std::time::Instant;

use quantick_feed_mt5::latency::LatencyTracker;
use quantick_feed_mt5::map::{SideMode, TickMapper};
use quantick_feed_mt5::protocol::{BridgeMsg, parse_line};

/// Prints per run. Two hundred thousand is far past any real burst — B3's
/// WIN$N averages tens of prints a second — so the per-print figure is stable
/// rather than a reading of one noisy moment.
const PRINTS: usize = 200_000;

/// Runs. The first pays for cache warming; reporting the best of several is
/// what makes two runs on two branches comparable.
const ROUNDS: usize = 5;

/// B3: server time is UTC-3.
const OFFSET_S: i64 = -10_800;

fn main() {
    let lines = burst(PRINTS);
    let mut best_ns = u128::MAX;
    for round in 1..=ROUNDS {
        let mut mapper = TickMapper::new(SideMode::TickRule, OFFSET_S);
        let mut latency = LatencyTracker::new();
        let started = Instant::now();
        let mut mapped = 0_usize;
        for line in &lines {
            let Ok(BridgeMsg::Tick(tick)) = parse_line(line) else {
                panic!("the generator emitted something the decoder rejects");
            };
            if let quantick_feed_mt5::map::MapOutcome::Trade { trade, .. } = mapper.map(&tick) {
                latency.observe_live(tick.time_ms, tick.sent_ms);
                mapped += 1;
                black_box(&trade);
            }
        }
        let elapsed = started.elapsed();
        // Drawn once per run, exactly as the session does: the clock read is
        // the reason the tracker samples rather than reporting per print.
        black_box(latency.sample(1_785_000_000_000, OFFSET_S * 1_000));
        let per_print = elapsed.as_nanos() / mapped as u128;
        println!(
            "round {round}: {mapped} prints in {:?} — {per_print} ns/print",
            elapsed
        );
        best_ns = best_ns.min(per_print);
    }
    println!("best: {best_ns} ns/print over {PRINTS} prints, {ROUNDS} rounds");
}

/// A burst of bridge tick lines, shaped exactly like `PROTOCOL.md`.
///
/// Prices walk a few ticks around the WIN$N level the recording in this repo
/// sits at, so the tick rule has real work to do rather than seeing one price
/// forever. Built ahead of the timed loop: generating strings is not what any
/// of this is measuring.
fn burst(count: usize) -> Vec<String> {
    let mut lines = Vec::with_capacity(count);
    let mut price = 177_795_i64;
    for i in 0..count {
        // A deterministic walk. No randomness: the same run twice must be the
        // same run, here as everywhere else in this workspace.
        price += match i % 7 {
            0 | 3 => 5,
            1 | 4 | 6 => -5,
            _ => 0,
        };
        let time_ms = 1_785_000_000_000 + i as i64;
        lines.push(format!(
            "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"sent_ms\":{sent_ms},\
             \"bid\":\"0\",\"ask\":\"0\",\"last\":\"{price}\",\"volume\":{volume},\"flags\":1080}}",
            seq = i + 1,
            sent_ms = time_ms + 4,
            volume = (i % 9) + 1,
        ));
    }
    lines
}
