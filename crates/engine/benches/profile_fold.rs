//! Range-profile benchmark: how fast does a profile fold a long venue prefix?
//!
//! A fixed-range volume profile over a time-based chart folds whatever the
//! range covers, and on a time chart that is the venue's backfilled candles —
//! tens of thousands of them. Those candles carry no tape, so each joins as an
//! *approximated* ladder: its volume spread over its high–low span. Spelling
//! that spread out row by row costs `O(candles x rows)`, and on a wide candle
//! `rows` runs to the level cap.
//!
//! Dependency-free like `hot_path`, deterministic in shape, run with
//! `cargo bench -p quantick-engine --bench profile_fold`.

use std::hint::black_box;
use std::time::Instant;

use quantick_engine::{Bar, BarFootprint, DEFAULT_LEVEL_CAP, ProfileFold, VolumeProfile};
use rust_decimal::Decimal;

/// A deterministic run of `n` venue candles around a 36000 base price, each
/// spanning `span` in price — the shape of an hourly crypto candle, which is
/// what a range profile on a time chart folds by the thousand.
fn make_candles(n: usize, span: i64) -> Vec<Bar> {
    let base = Decimal::from(36_000);
    let mut candles = Vec::with_capacity(n);
    for i in 0..n {
        // Drift deterministically so the candles do not all stack on one span.
        let drift = Decimal::from((i % 500) as i64);
        let low = base + drift;
        let high = low + Decimal::from(span);
        let close = low + Decimal::from(span / 2);
        candles.push(Bar {
            open_time: 1_700_000_000_000 + i as i64 * 3_600_000,
            close_time: 1_700_000_000_000 + (i as i64 + 1) * 3_600_000 - 1,
            open: low,
            high,
            low,
            close,
            buy_volume: Decimal::new(((i % 97) + 1) as i64, 2),
            sell_volume: Decimal::new(((i % 89) + 1) as i64, 2),
            trade_count: (i % 500) as u64 + 1,
        });
    }
    candles
}

/// A ladder per candle, then one merge — the fold before [`ProfileFold`],
/// kept as the yardstick the range fold is measured against.
fn ladder_per_candle(candles: &[Bar], group: Decimal) -> (f64, usize) {
    let start = Instant::now();
    let ladders: Vec<BarFootprint> = candles
        .iter()
        .filter_map(|bar| BarFootprint::approximated(bar, group, DEFAULT_LEVEL_CAP))
        .collect();
    let rows: usize = ladders.iter().map(|l| l.levels().len()).sum();
    let profile = VolumeProfile::merge(ladders.iter(), DEFAULT_LEVEL_CAP);
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    black_box(&profile);
    (elapsed, rows)
}

/// The same fold, each candle added as a range instead of a ladder.
fn spread_fold(candles: &[Bar], group: Decimal) -> (f64, usize) {
    let start = Instant::now();
    let mut fold = ProfileFold::new(group, DEFAULT_LEVEL_CAP);
    for bar in candles {
        fold.push_candle(bar);
    }
    let held = fold.rows_held();
    let profile = fold.profile();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    black_box(&profile);
    (elapsed, held)
}

fn main() {
    let candles = make_candles(25_000, 400);
    println!("25000 venue candles, 400-wide spans — the fold behind one range profile\n");
    println!(
        "{:>6}  {:>13}  {:>13}  {:>9}  {:>12}  {:>10}",
        "group", "ladders (ms)", "spreads (ms)", "speed-up", "ladder rows", "held rows"
    );
    for group in ["0.1", "1", "10"] {
        let group: Decimal = group.parse().unwrap();
        let (slow, rows) = ladder_per_candle(&candles, group);
        let (fast, held) = spread_fold(&candles, group);
        println!(
            "{group:>6}  {slow:>13.1}  {fast:>13.1}  {:>8.0}x  {rows:>12}  {held:>10}",
            slow / fast.max(f64::MIN_POSITIVE),
        );
    }

    // The partial read a painting consumer takes mid-fold: it must cost the
    // profile, not the range, or progressive loading would pay for itself.
    let mut fold = ProfileFold::new("0.1".parse().unwrap(), DEFAULT_LEVEL_CAP);
    for bar in &candles {
        fold.push_candle(bar);
    }
    let reads = 100;
    let start = Instant::now();
    for _ in 0..reads {
        black_box(fold.profile());
    }
    println!(
        "\npartial read of a 25000-candle fold: {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(reads),
    );
}
