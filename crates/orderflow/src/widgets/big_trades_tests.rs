use quantick_engine::Side;
use rust_decimal::Decimal;

use super::big_trades::{BIG_TRADES_WINDOW_MS, BigTrade, BigTradesParams, big_trades};
use crate::history::Aggression;

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn print(agg_id: u64, timestamp_ms: i64, side: Side, price: &str, quantity: &str) -> Aggression {
    Aggression {
        agg_id,
        timestamp_ms,
        price: d(price),
        quantity: d(quantity),
        side,
        generation: None,
    }
}

fn prints() -> Vec<Aggression> {
    vec![
        // Outside the five-minute window the clock below opens.
        print(7, 700_000, Side::Sell, "50000", "10"),
        // One millisecond, one side, two prices: a sweep only merging lifts
        // over the threshold. The sell in the same millisecond stays apart.
        print(1, 1_000_000, Side::Buy, "50000", "3"),
        print(2, 1_000_000, Side::Buy, "50010", "2"),
        print(3, 1_000_000, Side::Sell, "49990", "1"),
        print(4, 1_100_000, Side::Sell, "50000", "6"),
        print(5, 1_200_000, Side::Buy, "50000", "20"),
        print(6, 1_200_001, Side::Buy, "50000", "1"),
    ]
}

fn big(
    agg_id: u64,
    timestamp_ms: i64,
    side: Side,
    price: &str,
    quantity: &str,
    notional: &str,
    fill_count: usize,
) -> BigTrade {
    BigTrade {
        agg_id,
        timestamp_ms,
        side,
        price: d(price),
        quantity: d(quantity),
        notional: d(notional),
        fill_count,
        merged: fill_count > 1,
    }
}

const NOW_MS: i64 = 1_250_000;

#[test]
fn merge_and_filter_match_golden() {
    assert_eq!(BIG_TRADES_WINDOW_MS, 300_000);
    let prints = prints();
    let merged = big_trades(
        &prints,
        &BigTradesParams {
            min_notional: d("250000"),
            merge_fills: true,
            max_rows: 10,
        },
        NOW_MS,
    );
    assert_eq!(
        merged.rows,
        vec![
            big(5, 1_200_000, Side::Buy, "50000", "20", "1000000", 1),
            big(4, 1_100_000, Side::Sell, "50000", "6", "300000", 1),
            big(1, 1_000_000, Side::Buy, "50004", "5", "250020", 2),
            big(7, 700_000, Side::Sell, "50000", "10", "500000", 1),
        ]
    );
    assert!(merged.rows[2].merged);
    // The window is (now - 5 min, now]: print 7 is listed but not totalled.
    assert_eq!(merged.window_buy_notional, d("1250020"));
    assert_eq!(merged.window_sell_notional, d("300000"));

    let unmerged = big_trades(
        &prints,
        &BigTradesParams {
            min_notional: d("250000"),
            merge_fills: false,
            max_rows: 10,
        },
        NOW_MS,
    );
    assert_eq!(
        unmerged.rows,
        vec![
            big(5, 1_200_000, Side::Buy, "50000", "20", "1000000", 1),
            big(4, 1_100_000, Side::Sell, "50000", "6", "300000", 1),
            big(7, 700_000, Side::Sell, "50000", "10", "500000", 1),
        ]
    );
    assert!(unmerged.rows.iter().all(|row| !row.merged));
    assert_eq!(unmerged.window_buy_notional, d("1000000"));
    assert_eq!(unmerged.window_sell_notional, d("300000"));

    // The same tape read a minute later: the clock alone moves the window.
    let later = big_trades(
        &prints,
        &BigTradesParams {
            min_notional: d("250000"),
            merge_fills: true,
            max_rows: 10,
        },
        NOW_MS + 60_000,
    );
    assert_eq!(later.window_buy_notional, d("1000000"));
    assert_eq!(later.window_sell_notional, d("300000"));
}

#[test]
fn the_row_cap_keeps_the_newest_and_never_the_totals() {
    let prints = prints();
    let capped = big_trades(
        &prints,
        &BigTradesParams {
            min_notional: d("250000"),
            merge_fills: true,
            max_rows: 2,
        },
        NOW_MS,
    );
    let ids: Vec<u64> = capped.rows.iter().map(|row| row.agg_id).collect();
    assert_eq!(ids, vec![5, 4]);
    assert_eq!(capped.window_buy_notional, d("1250020"));
}

#[test]
fn input_order_does_not_change_the_rows() {
    let mut reversed = prints();
    reversed.reverse();
    let params = BigTradesParams {
        min_notional: d("50000"),
        merge_fills: true,
        max_rows: 10,
    };
    assert_eq!(
        big_trades(&reversed, &params, NOW_MS),
        big_trades(&prints(), &params, NOW_MS)
    );
}

#[test]
fn a_repeated_id_is_still_two_prints() {
    // An MT5 bridge restarts its sequence per session: the same id twice.
    let prints = vec![
        print(5, 2_000_000, Side::Buy, "76200", "10"),
        print(5, 2_100_000, Side::Buy, "76300", "1"),
    ];
    let rows = big_trades(
        &prints,
        &BigTradesParams {
            min_notional: d("250000"),
            merge_fills: true,
            max_rows: 10,
        },
        2_100_000,
    )
    .rows;
    assert_eq!(
        rows,
        vec![big(5, 2_000_000, Side::Buy, "76200", "10", "762000", 1)]
    );

    let same_ms = vec![
        print(5, 2_000_000, Side::Sell, "100", "2"),
        print(5, 2_000_000, Side::Sell, "100", "3"),
    ];
    let merged = big_trades(
        &same_ms,
        &BigTradesParams {
            min_notional: Decimal::ZERO,
            merge_fills: true,
            max_rows: 10,
        },
        2_000_000,
    );
    assert_eq!(
        merged.rows,
        vec![big(5, 2_000_000, Side::Sell, "100", "5", "500", 2)]
    );
}

#[test]
fn a_print_ahead_of_the_clock_is_still_in_the_totals() {
    let prints = vec![print(1, NOW_MS + 200, Side::Buy, "50000", "40")];
    let model = big_trades(
        &prints,
        &BigTradesParams {
            min_notional: d("250000"),
            merge_fills: true,
            max_rows: 10,
        },
        NOW_MS,
    );
    assert_eq!(model.rows.len(), 1);
    assert_eq!(model.window_buy_notional, d("2000000"));
}

#[test]
fn a_print_delivered_twice_counts_once() {
    let once = prints();
    let mut twice = prints();
    twice.push(once[4].clone());
    for merge_fills in [true, false] {
        let params = BigTradesParams {
            min_notional: d("250000"),
            merge_fills,
            max_rows: 10,
        };
        assert_eq!(
            big_trades(&twice, &params, NOW_MS),
            big_trades(&once, &params, NOW_MS)
        );
    }
}
