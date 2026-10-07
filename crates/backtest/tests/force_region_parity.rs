//! The `force-region` backtest, pinned byte for byte on a fixed tape.
//!
//! The armed-rectangle kernel reaches the harness through a seam the chart
//! shares, and that seam is allowed to move only if nothing it produces
//! does. This walks a deterministic synthetic tape — a mean-reverting walk
//! with periodic bursts, so force bars fire, brackets drop at fill, limits
//! rest and cancel — through every side, break policy and re-arm rule, two
//! sessions each so the per-session reset is crossed too, and compares the
//! full `SessionRun` of every run with the golden file recorded before the
//! seam moved.

use std::fmt::Write as _;
use std::path::Path;

use quantick_backtest::bars::BarSpec;
use quantick_backtest::run::run_session;
use quantick_backtest::strategies::ForceRegion;
use quantick_engine::{Side, Trade};
use quantick_replay::format::{self, ParseOptions, UtcOffset, WriteHeader};
use quantick_replay::session::Session;
use quantick_strategy::{BreakPolicy, Execution, ForceParams, Rearm, Region, StrategyParams};
use rust_decimal::Decimal;

/// Every run's `SessionRun`, as recorded on the base before the shared
/// runner existed.
const GOLDEN: &str = include_str!("fixtures/force_region_parity.txt");

/// Prints per synthetic session.
const PRINTS: usize = 2_400;

/// A deterministic tape: a walk pulled toward 1000, a burst every so often.
///
/// The generator is a fixed linear congruential sequence, so the tape is
/// the same on every machine and every run — the fixture is the seed.
fn walk(seed: u64) -> Vec<Decimal> {
    let mut state = seed;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state >> 33
    };
    let mut price: i64 = 1_000;
    let mut prices = Vec::with_capacity(PRINTS);
    for _ in 0..PRINTS {
        let roll = next() % 100;
        let pull = if price > 1_000 { -1 } else { 1 };
        let step: i64 = match roll {
            0..=2 => pull * 6,
            3..=5 => -pull * 5,
            6..=40 => 1,
            41..=75 => -1,
            76..=85 => pull,
            _ => 0,
        };
        price += step;
        prices.push(Decimal::from(price));
    }
    prices
}

fn session(date: &str, seed: u64) -> Session {
    let mut text = format::write_header(&WriteHeader {
        symbol: "WINQ26".to_string(),
        timezone: UtcOffset::UTC,
        side_source: "flags".to_string(),
        source: Some("synthetic".to_string()),
    });
    for (step, price) in walk(seed).into_iter().enumerate() {
        format::write_trade(
            &mut text,
            &Trade {
                agg_id: step as u64 + 1,
                timestamp_ms: 1_786_233_600_000 + step as i64 * 1_000,
                price,
                quantity: Decimal::ONE,
                side: if step % 2 == 0 { Side::Buy } else { Side::Sell },
            },
            None,
            UtcOffset::UTC,
        );
    }
    Session::from_text(
        Path::new(&format!("synthetic/WINQ26/{date}.csv")),
        &text,
        ParseOptions::default(),
    )
    .expect("the synthetic session parses")
}

/// Every configuration's runs, rendered one `SessionRun` per line.
fn every_run() -> String {
    let sessions = [session("20260812", 7), session("20260813", 11)];
    let mut out = String::new();
    for side in [Side::Buy, Side::Sell] {
        for on_break in [BreakPolicy::Ignore, BreakPolicy::RetestLimit] {
            for rearm in [Rearm::OneShot, Rearm::Auto] {
                let mut strategy = ForceRegion::new(
                    Region::new(Decimal::from(990), Decimal::from(1_010)),
                    StrategyParams {
                        side,
                        quantity: Decimal::ONE,
                        tp_mult: Decimal::ONE,
                        sl_mult: Decimal::ONE,
                        rearm,
                        on_break,
                        execution: Execution::Paper,
                    },
                    ForceParams {
                        window: 5,
                        min_factor: "1.5".parse().expect("fixture factor"),
                        max_factor: "4".parse().expect("fixture factor"),
                        min_range: Decimal::ZERO,
                    },
                );
                for session in &sessions {
                    let run = run_session(session, BarSpec::Tick(4), &mut strategy);
                    writeln!(out, "{side:?} {on_break:?} {rearm:?}: {run:?}")
                        .expect("a String takes every write");
                }
            }
        }
    }
    out
}

#[test]
fn the_force_region_backtest_matches_its_recorded_output() {
    let actual = every_run();
    // Line endings are the checkout's business, not the report's.
    let golden = GOLDEN.replace("\r\n", "\n");
    assert!(
        actual == golden,
        "the force-region backtest drifted from its recorded output:\n{actual}"
    );
}

/// The golden file is only a guard if the tape exercises the paths it
/// claims to: trades on both sides, and the anomalies a moving seam would
/// most easily lose (a dropped bracket, a cancelled retest limit).
#[test]
fn the_parity_tape_exercises_the_seam() {
    let golden = GOLDEN.replace("\r\n", "\n");
    for needle in [
        "Buy ",
        "Sell ",
        "TakeProfit",
        "StopLoss",
        "brackets_dropped: {\"",
        "cancels: {\"",
    ] {
        assert!(
            golden.contains(needle),
            "the parity tape never shows {needle:?}"
        );
    }
}
