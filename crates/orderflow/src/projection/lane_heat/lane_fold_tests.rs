use super::*;
use crate::config::{DisplayGrouping, HeatmapConfig};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(price.into(), quantity.into()).unwrap()
}

fn history() -> LiquidityHistory {
    LiquidityHistory::new(HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        ..HeatmapConfig::default()
    })
}

fn snapshot(
    history: &mut LiquidityHistory,
    at_ms: i64,
    generation: u64,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
) {
    history
        .install_snapshot(
            at_ms,
            generation,
            BookSnapshot::new(generation * 1_000, bids, asks, BookCoverage::Full),
        )
        .unwrap();
}

/// Apply `bids` and `asks` at `at_ms`, numbered after the book's last update.
fn delta(history: &mut LiquidityHistory, at_ms: i64, bids: Vec<BookLevel>, asks: Vec<BookLevel>) {
    let id = history.book().last_update_id().unwrap() + 1;
    history
        .apply_delta(at_ms, &BookDelta::new(id, id, bids, asks))
        .unwrap();
}

fn native() -> EffectiveGrouping {
    EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, 8.into())
}

fn window(start_ms: i64, end_ms: i64) -> GroupingWindow {
    GroupingWindow {
        start_ms,
        end_ms,
        open_run_end_ms: end_ms,
        price_low: 90.into(),
        price_high: 110.into(),
    }
}

fn run(
    generation: u64,
    side: RestingSide,
    price: i64,
    quantity: Decimal,
    start_ms: i64,
    end_ms: i64,
) -> VisualLiquidityRun {
    VisualLiquidityRun {
        generation,
        side,
        price_bucket: price.into(),
        quantity,
        start_ms,
        end_ms,
    }
}

fn project(
    history: &LiquidityHistory,
    window: GroupingWindow,
    observed_end_ms: i64,
    cap: usize,
) -> Vec<DepthRun> {
    LaneFold::default().project(history, native(), window, observed_end_ms, cap)
}

#[test]
fn duration_weighted_bins_include_observed_zero_and_preserve_volume_time() {
    let mut history = history();
    snapshot(&mut history, 0, 1, vec![level(99, 10)], vec![]);
    for (at_ms, quantity) in [(100, 0), (200, 20), (300, 0), (400, 30), (500, 0), (900, 1)] {
        delta(&mut history, at_ms, vec![level(99, quantity)], vec![]);
    }
    let output = project(&history, window(0, 1_000), 900, 2);
    assert_eq!(output.len(), 2);
    let average = output.iter().find(|depth| depth.time_averaged).unwrap();
    assert_eq!(
        average.run,
        run(
            1,
            RestingSide::Bid,
            99,
            Decimal::from(6_000) / Decimal::from(900),
            0,
            900
        )
    );
    let volume_time: Decimal = output
        .iter()
        .map(|depth| depth.run.quantity * Decimal::from(depth.run.end_ms - depth.run.start_ms))
        .sum();
    assert!((volume_time - Decimal::from(6_100)).abs() < Decimal::new(1, 20));
    let current = output.iter().find(|depth| !depth.time_averaged).unwrap();
    assert_eq!(
        current.run,
        run(1, RestingSide::Bid, 99, Decimal::ONE, 900, 1_000)
    );
}

#[test]
fn averaging_never_bridges_a_gap_generation_or_side() {
    let mut history = history();
    snapshot(&mut history, 0, 1, vec![level(100, 10)], vec![]);
    delta(&mut history, 100, vec![level(100, 20)], vec![]);
    history.mark_gap(200, "transport_closed").unwrap();
    snapshot(&mut history, 800, 2, vec![], vec![level(100, 30)]);
    delta(&mut history, 900, vec![], vec![level(100, 40)]);
    history.mark_gap(1_000, "transport_closed").unwrap();
    let output = project(&history, window(0, 1_100), 1_000, 2);
    assert_eq!(output.len(), 2);
    assert!(output.iter().all(|depth| depth.time_averaged));
    assert_eq!(
        output[0].run,
        run(1, RestingSide::Bid, 100, 15.into(), 0, 200)
    );
    assert_eq!(
        output[1].run,
        run(2, RestingSide::Ask, 100, 35.into(), 800, 1_000)
    );
}

#[test]
fn current_intervals_stay_exact_over_any_cap() {
    let mut history = history();
    snapshot(&mut history, 0, 1, vec![level(99, 1)], vec![level(101, 2)]);
    let expected = vec![
        run(1, RestingSide::Bid, 99, Decimal::ONE, 0, 100),
        run(1, RestingSide::Ask, 101, Decimal::TWO, 0, 100),
    ];
    for cap in [0, 1, 2, usize::MAX] {
        let output = project(&history, window(0, 100), 100, cap);
        assert!(output.iter().all(|depth| !depth.time_averaged), "cap {cap}");
        let mut runs: Vec<_> = output.into_iter().map(|depth| depth.run).collect();
        runs.sort_by_key(|run| run.side == RestingSide::Ask);
        assert_eq!(runs, expected, "cap {cap}");
    }
}

#[test]
fn deleted_rows_never_average_into_unobserved_carry() {
    let mut history = history();
    snapshot(&mut history, 0, 1, vec![level(99, 10)], vec![level(101, 1)]);
    delta(&mut history, 100, vec![level(99, 20)], vec![]);
    delta(&mut history, 200, vec![level(99, 0)], vec![]);
    delta(&mut history, 1_000, vec![], vec![]);
    let output = project(&history, window(0, 1_100), 1_000, 2);
    let deleted = output
        .iter()
        .find(|depth| depth.run.side == RestingSide::Bid)
        .unwrap();
    assert!(deleted.time_averaged);
    assert_eq!(deleted.run.end_ms, 200);
    assert_eq!(deleted.run.quantity, Decimal::from(15));
    assert!(
        output
            .iter()
            .filter(|depth| depth.run.end_ms > 1_000)
            .all(|depth| !depth.time_averaged)
    );
}

#[test]
fn first_observation_clips_unknown_time_but_keeps_internal_observed_zeros() {
    let mut history = history();
    // The ask at 120 is off the window: the bid row is the only one drawn.
    snapshot(&mut history, 0, 1, vec![], vec![level(120, 1)]);
    for (at_ms, quantity) in [(600, 10), (650, 0), (700, 20), (750, 0), (900, 1)] {
        delta(&mut history, at_ms, vec![level(95, quantity)], vec![]);
    }
    let output = project(&history, window(0, 1_000), 900, 2);
    let average = output.iter().find(|depth| depth.time_averaged).unwrap();
    assert_eq!(
        average.run,
        run(1, RestingSide::Bid, 95, 5.into(), 600, 900)
    );
    assert_eq!(output.len(), 2);
}

/// A busy book on a sliding lane: the fold kept between requests draws
/// exactly what a fold built from the whole history at once draws, through
/// a gap and a resync, at every width the lane passes through.
#[test]
fn the_kept_fold_draws_what_a_fresh_fold_draws() {
    let mut history = history();
    snapshot(
        &mut history,
        0,
        1,
        vec![level(95, 10), level(96, 10), level(97, 10)],
        vec![level(101, 10), level(102, 10)],
    );
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Multiple(2), Decimal::ONE, 8.into());
    let mut kept = LaneFold::default();
    for step in 1..400_i64 {
        let at_ms = step * 50;
        if step == 200 {
            history.mark_gap(at_ms, "transport_closed").unwrap();
        }
        if step == 210 {
            snapshot(
                &mut history,
                at_ms,
                2,
                vec![level(96, 7)],
                vec![level(102, 9)],
            );
        }
        if (200..=210).contains(&step) {
            continue;
        }
        let size = |price: i64| {
            level(
                price,
                if (step / (price % 4 + 1)) % 3 == 0 {
                    5
                } else {
                    30
                },
            )
        };
        delta(
            &mut history,
            at_ms,
            vec![size(95), size(96), size(97)],
            vec![size(101), size(102)],
        );
        if step % 7 != 0 {
            continue;
        }
        let window = GroupingWindow {
            start_ms: (at_ms - 6_000).max(0),
            end_ms: at_ms + 20,
            open_run_end_ms: at_ms + 20,
            price_low: 94.into(),
            price_high: 103.into(),
        };
        let cap = if step % 2 == 0 { 12 } else { 40 };
        let incremental = kept.project(&history, grouping, window, at_ms, cap);
        let width_ms = kept.fold.as_ref().unwrap().key.width_ms;
        let mut fresh = LaneFold {
            fold: Some(Fold::build(
                &history,
                FoldKey {
                    capture: Decimal::ONE,
                    grouping,
                    width_ms,
                },
                window.start_ms,
            )),
        };
        let rebuilt = fresh.project(&history, grouping, window, at_ms, cap);
        let shape = |runs: &[DepthRun]| {
            runs.iter()
                .map(|depth| (depth.run.clone(), depth.time_averaged))
                .collect::<Vec<_>>()
        };
        assert_eq!(shape(&incremental), shape(&rebuilt), "step {step}");
    }
}

/// The bins of a static book keep their edges and means as the lane's start
/// walks through them; only the oldest is cut at the start.
#[test]
fn a_static_book_keeps_its_bins_as_the_lane_slides() {
    let mut history = history();
    snapshot(
        &mut history,
        0,
        1,
        vec![level(95, 10), level(96, 10)],
        vec![],
    );
    for step in 1..300_i64 {
        let size = |price: i64| {
            level(
                price,
                if (step / (price % 3 + 1)) % 2 == 0 {
                    10
                } else {
                    40
                },
            )
        };
        delta(&mut history, step * 100, vec![size(95), size(96)], vec![]);
    }
    let mut fold = LaneFold::default();
    let mut averaged = |start_ms: i64| {
        fold.project(&history, native(), window(start_ms, 30_000), 29_900, 16)
            .into_iter()
            .filter(|depth| depth.time_averaged)
            .map(|depth| depth.run)
            .collect::<Vec<_>>()
    };
    let first = averaged(10_000);
    assert!(first.len() > 2, "the lane averages its history");
    for start_ms in [10_016, 10_032, 10_500, 12_345] {
        let later = averaged(start_ms);
        for bin in &later {
            let same = first
                .iter()
                .find(|earlier| {
                    earlier.side == bin.side
                        && earlier.price_bucket == bin.price_bucket
                        && earlier.end_ms == bin.end_ms
                })
                .unwrap_or_else(|| panic!("start {start_ms}: a new bin edge {bin:?}"));
            assert_eq!(
                same.quantity, bin.quantity,
                "start {start_ms}: its mean moved"
            );
            assert_eq!(bin.start_ms, same.start_ms.max(start_ms));
        }
    }
}
