//! [`FormingRun`]: the lane ladder's prefixes, and what a walk may cost.
//!
//! The oracle is the fold as it shipped before the walk was bounded, kept
//! verbatim: every prefix the bounded walk returns must equal the one the
//! oracle returns, byte for byte, on the engine's golden tapes and on
//! synthetic runs appended in irregular batches.

use super::forming_run::{CHECKPOINT_SPACING, FormingRun, folds_on_this_thread};
use super::*;
use quantick_engine::{Side, fixture::parse_trades};
use rust_decimal::Decimal;

/// `lane_prefixes` as it stood at `2452e577` (`indicator_worker.rs`), before
/// the fold was bounded. It folds the whole run on every call.
fn oracle(run: &[Trade], rungs: usize) -> Vec<Bar> {
    if run.is_empty() || rungs == 0 {
        return Vec::new();
    }
    let step = run.len().div_ceil(rungs.min(run.len())).max(1);
    let mut prefixes = Vec::with_capacity(run.len().div_ceil(step) + 1);
    let mut forming: Option<Bar> = None;
    for (index, trade) in run.iter().enumerate() {
        match &mut forming {
            None => forming = Some(Bar::opened_by(trade)),
            Some(bar) => bar.extend(trade),
        }
        let last = index + 1 == run.len();
        if last || (index + 1) % step == 0 {
            prefixes.push(forming.clone().expect("a trade opened the bar"));
        }
    }
    prefixes
}

/// The engine's golden trade tapes, and all of them end to end.
fn golden_tapes() -> Vec<Vec<Trade>> {
    let files = [
        include_str!("../../../engine/tests/fixtures/sample_trades.csv"),
        include_str!("../../../engine/tests/fixtures/tick_trades.csv"),
        include_str!("../../../engine/tests/fixtures/time_trades.csv"),
        include_str!("../../../engine/tests/fixtures/volume_trades.csv"),
        include_str!("../../../engine/tests/fixtures/dollar_trades.csv"),
        include_str!("../../../engine/tests/fixtures/imbalance_trades.csv"),
    ];
    let mut tapes: Vec<Vec<Trade>> = files
        .iter()
        .map(|text| parse_trades(text).expect("a golden fixture parses"))
        .collect();
    let joined: Vec<Trade> = tapes.iter().flatten().cloned().collect();
    tapes.push(joined);
    tapes
}

/// A synthetic print whose every field moves: the price walks both ways and
/// repeats, sides alternate irregularly, quantities carry decimals, and some
/// prints share a timestamp. The two near-maximum quantities make the side
/// totals saturate, so the fold's order is observable in the output.
fn synthetic(id: u64) -> Trade {
    let quantity = if id == 97 || id == 98 {
        Decimal::MAX - Decimal::from(id)
    } else {
        Decimal::new((1 + (id * 37) % 500) as i64, 3)
    };
    Trade {
        agg_id: id,
        timestamp_ms: 1_700_000_000_000 + (id / 3) as i64 * 7,
        price: Decimal::new(3_600_000 + ((id * 13) % 41) as i64 - 20, 2),
        quantity,
        side: if (id * 7) % 5 < 2 {
            Side::Sell
        } else {
            Side::Buy
        },
    }
}

/// Batch sizes that never line up with the checkpoint spacing for long.
fn irregular(batch: usize) -> usize {
    [1, 2, 3, 5, 8, 13, 64, 65, 127, 1][batch % 10]
}

/// Append `tape` in irregular batches, and after every batch compare the
/// run's prefixes with the oracle's at every rung count in `rungs`.
fn assert_identical_while_appending(tape: &[Trade], rungs: &[usize]) {
    let mut run = FormingRun::default();
    for rung in rungs {
        assert_eq!(run.prefixes(*rung), oracle(&[], *rung), "an empty run");
    }
    let mut sent = 0;
    let mut batch = 0;
    while sent < tape.len() {
        let end = (sent + irregular(batch)).min(tape.len());
        run.extend(tape[sent..end].to_vec());
        sent = end;
        batch += 1;
        for &rungs in rungs {
            assert_eq!(
                run.prefixes(rungs),
                oracle(&tape[..sent], rungs),
                "{sent} prints, {rungs} rungs"
            );
        }
    }
}

#[test]
fn the_bounded_walk_returns_the_oracles_prefixes_on_every_golden_tape() {
    let every_rung_count: Vec<usize> = (0..=70).collect();
    for tape in golden_tapes() {
        assert_identical_while_appending(&tape, &every_rung_count);
    }
}

#[test]
fn the_bounded_walk_returns_the_oracles_prefixes_on_synthetic_runs() {
    let every_rung_count: Vec<usize> = (0..=70).collect();
    let tape: Vec<Trade> = (1..=600).map(synthetic).collect();
    assert_identical_while_appending(&tape, &every_rung_count);

    // Long runs, where most rungs sit between two checkpoints.
    let tape: Vec<Trade> = (1..=20_000).map(synthetic).collect();
    assert_identical_while_appending(&tape, &[1, 2, 3, 7, 16, 63, 64, 65, 70, 1_000]);
}

#[test]
fn a_cut_run_starts_its_prefixes_again_from_its_own_first_print() {
    let tape: Vec<Trade> = (1..=300).map(synthetic).collect();
    let mut run = FormingRun::default();
    run.extend(tape[..200].to_vec());
    run = FormingRun::default();
    assert!(run.prefixes(16).is_empty(), "a cut run has nothing to walk");
    run.extend(tape[200..].to_vec());
    assert_eq!(run.prefixes(16), oracle(&tape[200..], 16));
}

/// The limit itself: whatever the forming bar's length, a walk folds at most
/// `CHECKPOINT_SPACING - 1` prints per rung, and appending folds each new
/// print once.
#[test]
fn a_walk_folds_a_bounded_number_of_prints_whatever_the_runs_length() {
    let per_rung = (CHECKPOINT_SPACING - 1) as u64;
    for length in [1_000_usize, 1_000_000] {
        let mut run = FormingRun::default();
        let tape: Vec<Trade> = (1..=length as u64).map(synthetic).collect();
        for batch in tape.chunks(5) {
            let before = folds_on_this_thread();
            run.extend(batch.to_vec());
            assert!(
                folds_on_this_thread() - before <= batch.len() as u64,
                "appending {} prints folds each once",
                batch.len()
            );
        }
        for rungs in [1_usize, 16, MAX_LANE_RUNGS] {
            let before = folds_on_this_thread();
            let prefixes = run.prefixes(rungs);
            let walked = folds_on_this_thread() - before;
            assert!(
                walked <= rungs as u64 * per_rung,
                "{length} prints, {rungs} rungs: the walk folded {walked}, budget {}",
                rungs as u64 * per_rung
            );
            assert_eq!(
                prefixes.last().map(|bar| bar.trade_count),
                Some(length as u64)
            );
        }
        assert_eq!(run.len(), length);
    }
}
