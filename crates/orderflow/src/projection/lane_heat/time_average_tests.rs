use super::*;
use crate::RestingSide;

fn run(
    generation: u64,
    side: RestingSide,
    price: i64,
    quantity: i64,
    start_ms: i64,
    end_ms: i64,
) -> VisualLiquidityRun {
    VisualLiquidityRun {
        generation,
        side,
        price_bucket: price.into(),
        quantity: quantity.into(),
        start_ms,
        end_ms,
    }
}

#[test]
fn duration_weighted_bins_include_observed_zero_and_preserve_volume_time() {
    let input = vec![
        run(1, RestingSide::Bid, 99, 10, 0, 100),
        run(1, RestingSide::Bid, 99, 20, 200, 300),
        run(1, RestingSide::Bid, 99, 30, 400, 500),
        run(1, RestingSide::Bid, 99, 1, 900, 1_000),
    ];
    let original: Decimal = input
        .iter()
        .map(|run| run.quantity * Decimal::from(run.end_ms - run.start_ms))
        .sum();
    let output = project(
        input,
        [CoverageSegment {
            generation: 1,
            start_ms: 0,
            end_ms: None,
        }]
        .into_iter(),
        0,
        1_000,
        1_000,
        2,
    );
    assert_eq!(output.len(), 2);
    let average = output.iter().find(|depth| depth.time_averaged).unwrap();
    assert_eq!(average.run.start_ms, 0);
    assert_eq!(average.run.end_ms, 900);
    assert_eq!(
        average.run.quantity,
        Decimal::from(6_000) / Decimal::from(900)
    );
    let actual: Decimal = output
        .iter()
        .map(|depth| depth.run.quantity * Decimal::from(depth.run.end_ms - depth.run.start_ms))
        .sum();
    assert!((actual - original).abs() < Decimal::new(1, 20));
    let current = output.iter().find(|depth| !depth.time_averaged).unwrap();
    assert_eq!(current.run.quantity, Decimal::ONE);
    assert_eq!(current.run.start_ms, 900);
}

#[test]
fn averaging_never_bridges_a_gap_generation_or_side() {
    let input = vec![
        run(1, RestingSide::Bid, 100, 10, 0, 100),
        run(1, RestingSide::Bid, 100, 20, 100, 200),
        run(2, RestingSide::Ask, 100, 30, 800, 900),
        run(2, RestingSide::Ask, 100, 40, 900, 1_000),
    ];
    let output = project(
        input,
        [
            CoverageSegment {
                generation: 1,
                start_ms: 0,
                end_ms: Some(200),
            },
            CoverageSegment {
                generation: 2,
                start_ms: 800,
                end_ms: Some(1_000),
            },
        ]
        .into_iter(),
        0,
        1_100,
        1_100,
        2,
    );
    assert_eq!(output.len(), 2);
    assert!(output.iter().all(|depth| depth.time_averaged));
    assert_eq!(output[0].run, run(1, RestingSide::Bid, 100, 15, 0, 200));
    assert_eq!(output[1].run, run(2, RestingSide::Ask, 100, 35, 800, 1_000));
}

#[test]
fn under_budget_is_exact_and_excess_rows_remain_honest_for_the_final_cap() {
    let input = vec![
        run(1, RestingSide::Bid, 99, 1, 0, 100),
        run(1, RestingSide::Ask, 101, 2, 0, 100),
    ];
    for cap in [0, 1, 2, usize::MAX] {
        let output = project(
            input.clone(),
            [CoverageSegment {
                generation: 1,
                start_ms: 0,
                end_ms: None,
            }]
            .into_iter(),
            0,
            100,
            100,
            cap,
        );
        assert_eq!(
            output
                .iter()
                .map(|depth| depth.run.clone())
                .collect::<Vec<_>>(),
            input
        );
        assert!(output.iter().all(|depth| !depth.time_averaged));
    }
}

#[test]
fn deleted_rows_never_average_into_unobserved_carry() {
    let output = project(
        vec![
            run(1, RestingSide::Bid, 99, 10, 0, 100),
            run(1, RestingSide::Bid, 99, 20, 100, 200),
            run(1, RestingSide::Ask, 101, 1, 0, 1_100),
        ],
        [CoverageSegment {
            generation: 1,
            start_ms: 0,
            end_ms: None,
        }]
        .into_iter(),
        0,
        1_100,
        1_000,
        2,
    );
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
