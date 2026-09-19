use super::fixtures::{CASES, Corpus};
use std::time::Instant;

fn case() -> usize {
    let name = std::env::var("A1C_CASE").expect("A1C_CASE selects exactly one frozen case");
    CASES
        .iter()
        .position(|candidate| *candidate == name)
        .expect("known frozen case")
}

fn block(corpus: &Corpus, case: usize, count: u64) {
    for _ in 0..count {
        corpus.operation(case);
    }
}

/// Existing globally active work_meter remains in this binary. These are
/// instrumented app-test timings, not uninstrumented production latency.
#[test]
#[ignore]
fn baseline_calibration() {
    let case = case();
    let corpus = Corpus::new();
    corpus.assert_cases();
    for _ in 0..3 {
        corpus.operation(case);
    }
    let mut count = if case == 6 { 1 } else { 1_000 };
    let ceiling = if case == 6 { 128 } else { 1_048_576 };
    loop {
        let started = Instant::now();
        block(&corpus, case, count);
        let elapsed_ns = started.elapsed().as_nanos();
        eprintln!(
            "A1C_CALIBRATION case={} count={count} elapsed_ns={elapsed_ns} instrumented=true",
            CASES[case]
        );
        if elapsed_ns >= 250_000_000 {
            eprintln!("A1C_FROZEN_COUNT case={} count={count}", CASES[case]);
            return;
        }
        assert!(
            count < ceiling,
            "calibration ceiling reached below250ms; new root decision required"
        );
        count = (count * 2).min(ceiling);
    }
}

#[test]
#[ignore]
fn matched_observation() {
    let case = case();
    let count: u64 = std::env::var("A1C_COUNT")
        .expect("frozen calibrated count")
        .parse()
        .unwrap();
    assert!(count > 0);
    let mode = std::env::var("A1C_MODE").expect("timing or allocations");
    let corpus = Corpus::new();
    corpus.assert_cases();
    if mode == "timing" {
        for warmup in 1..=3 {
            let started = Instant::now();
            block(&corpus, case, count);
            eprintln!(
                "A1C_WARMUP case={} count={count} index={warmup} elapsed_ns={}",
                CASES[case],
                started.elapsed().as_nanos()
            );
        }
        let started = Instant::now();
        block(&corpus, case, count);
        eprintln!(
            "A1C_TIMING case={} count={count} elapsed_ns={} instrumented=true",
            CASES[case],
            started.elapsed().as_nanos()
        );
    } else {
        assert_eq!(mode, "allocations");
        for observation in 1..=3 {
            crate::work_meter::reset_largest();
            let before = crate::work_meter::tally();
            block(&corpus, case, count);
            let delta = crate::work_meter::tally().since(before);
            eprintln!(
                "A1C_ALLOCATIONS case={} count={count} observation={observation} allocs={} alloc_bytes={} reallocs={} realloc_copy_bytes={} largest_realloc_copy={}",
                CASES[case],
                delta.allocs,
                delta.alloc_bytes,
                delta.reallocs,
                delta.realloc_copy_bytes,
                delta.largest_realloc_copy
            );
        }
    }
}
