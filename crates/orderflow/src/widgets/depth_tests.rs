use quantick_engine::Side;
use quantick_orderbook::BookLevel;
use rust_decimal::Decimal;

use super::depth::{DepthInput, DepthRow, UNATTRIBUTED_L2_REDUCTION, build_depth};
use crate::engine::BookLadder;
use crate::history::Aggression;

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn level(price: &str, quantity: &str) -> BookLevel {
    BookLevel::new(d(price), d(quantity)).expect("fixture level")
}

fn ladder(bids: &[(&str, &str)], asks: &[(&str, &str)]) -> BookLadder {
    let bids: Vec<BookLevel> = bids.iter().map(|(p, q)| level(p, q)).collect();
    let asks: Vec<BookLevel> = asks.iter().map(|(p, q)| level(p, q)).collect();
    BookLadder {
        best_bid: bids.first().copied(),
        best_ask: asks.first().copied(),
        bids,
        asks,
    }
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

fn row(price: &str, sold: &str, bid: &str, ask: &str, bought: &str) -> DepthRow {
    DepthRow {
        price: d(price),
        sold: d(sold),
        bid: d(bid),
        ask: d(ask),
        bought: d(bought),
        last: false,
        wall: false,
        bid_unattributed: Decimal::ZERO,
        ask_unattributed: Decimal::ZERO,
    }
}

/// The earlier book, observed at `EARLIER_AT_MS`.
fn earlier() -> BookLadder {
    ladder(
        &[
            ("1000.0", "2"),
            ("999.5", "3"),
            ("999.0", "1"),
            ("998.5", "4"),
        ],
        &[
            ("1000.5", "1"),
            ("1001.0", "2"),
            ("1001.5", "5"),
            ("1002.0", "1"),
        ],
    )
}

/// The later book: 1000.5's ask was lifted by trades, 1001.5's ask and
/// 999.5's bid shrank with no trade there, 999.0's bid grew into a wall and
/// 1001.3 lands in the 1001.0 row.
fn later() -> BookLadder {
    ladder(
        &[
            ("1000.0", "1.5"),
            ("999.5", "1"),
            ("999.0", "20"),
            ("998.5", "4"),
        ],
        &[
            ("1001.0", "2"),
            ("1001.3", "0.5"),
            ("1001.5", "3"),
            ("1002.0", "1"),
        ],
    )
}

const EARLIER_AT_MS: i64 = 1_000;
const LATER_AT_MS: i64 = 2_000;

fn prints() -> Vec<Aggression> {
    vec![
        // Before the earlier book: counted as volume, never as evidence.
        print(10, 900, Side::Sell, "999.5", "1"),
        print(11, 1_100, Side::Buy, "1000.5", "0.6"),
        print(12, 1_200, Side::Sell, "1000.0", "0.5"),
        print(13, 1_300, Side::Buy, "1000.5", "0.4"),
    ]
}

#[test]
fn ladder_rows_match_golden() {
    let earlier = earlier();
    let later = later();
    let prints = prints();
    let model = build_depth(&DepthInput {
        earlier: Some((&earlier, EARLIER_AT_MS)),
        ladder: &later,
        ladder_at_ms: LATER_AT_MS,
        prints: &prints,
        tick: d("0.5"),
        rows_per_side: 4,
    })
    .expect("a two-sided book builds a model");

    let mut golden = vec![
        row("1002.5", "0", "0", "0", "0"),
        row("1002.0", "0", "0", "1", "0"),
        row("1001.5", "0", "0", "3", "0"),
        row("1001.0", "0", "0", "2.5", "0"),
        row("1000.5", "0", "0", "0", "1.0"),
        row("1000.0", "0.5", "1.5", "0", "0"),
        row("999.5", "1", "1", "0", "0"),
        row("999.0", "0", "20", "0", "0"),
        row("998.5", "0", "4", "0", "0"),
    ];
    golden[2].ask_unattributed = d("2");
    golden[4].last = true;
    golden[6].bid_unattributed = d("2");
    golden[7].wall = true;
    assert_eq!(model.rows, golden);

    assert_eq!(model.last_price, d("1000.5"));
    assert_eq!(model.spread, Some(d("1.0")));
    // Mid 1000.5, band 1.0005: bids from 999.4995, asks to 1001.5005.
    assert_eq!(model.near_touch, Some((d("2.5"), d("5.5"))));

    let labels: Vec<(Decimal, &str)> = model
        .rows
        .iter()
        .filter_map(|row| row.reduction_label().map(|label| (row.price, label)))
        .collect();
    assert_eq!(
        labels,
        vec![
            (d("1001.5"), UNATTRIBUTED_L2_REDUCTION),
            (d("999.5"), UNATTRIBUTED_L2_REDUCTION),
        ]
    );
    assert_eq!(UNATTRIBUTED_L2_REDUCTION, "unattributed L2 reduction");
}

#[test]
fn without_an_earlier_book_nothing_is_called_a_reduction() {
    let later = later();
    let prints = prints();
    let model = build_depth(&DepthInput {
        earlier: None,
        ladder: &later,
        ladder_at_ms: LATER_AT_MS,
        prints: &prints,
        tick: d("0.5"),
        rows_per_side: 4,
    })
    .expect("model");
    assert!(model.rows.iter().all(|row| row.reduction_label().is_none()));
}

#[test]
fn an_empty_book_without_prints_has_no_model() {
    let empty = ladder(&[], &[]);
    let model = build_depth(&DepthInput {
        earlier: None,
        ladder: &empty,
        ladder_at_ms: LATER_AT_MS,
        prints: &[],
        tick: d("0.5"),
        rows_per_side: 4,
    });
    assert!(model.is_none());
}

#[test]
fn without_prints_the_ladder_centres_on_the_mid() {
    let later = later();
    let model = build_depth(&DepthInput {
        earlier: None,
        ladder: &later,
        ladder_at_ms: LATER_AT_MS,
        prints: &[],
        tick: d("0.5"),
        rows_per_side: 1,
    })
    .expect("model");
    let prices: Vec<Decimal> = model.rows.iter().map(|row| row.price).collect();
    assert_eq!(prices, vec![d("1001.0"), d("1000.5"), d("1000.0")]);
    assert_eq!(model.last_price, d("1000.5"));
}

#[test]
fn a_print_after_the_current_book_explains_nothing() {
    let earlier = earlier();
    let later = later();
    let mut prints = prints();
    // A sell at 999.5 after the current book cannot explain its drop.
    prints.push(print(14, LATER_AT_MS + 500, Side::Sell, "999.5", "2"));
    let model = build_depth(&DepthInput {
        earlier: Some((&earlier, EARLIER_AT_MS)),
        ladder: &later,
        ladder_at_ms: LATER_AT_MS,
        prints: &prints,
        tick: d("0.5"),
        rows_per_side: 4,
    })
    .expect("model");
    let row = model
        .rows
        .iter()
        .find(|row| row.price == d("999.5"))
        .expect("row");
    assert_eq!(row.bid_unattributed, d("2"));
}

#[test]
fn a_repeated_id_does_not_make_the_centre_order_dependent() {
    let later = later();
    let forward = vec![
        print(5, 1_500, Side::Buy, "1000.0", "1"),
        print(5, 1_500, Side::Buy, "1001.0", "1"),
    ];
    let mut backward = forward.clone();
    backward.reverse();
    let build = |prints: &[Aggression]| {
        build_depth(&DepthInput {
            earlier: None,
            ladder: &later,
            ladder_at_ms: LATER_AT_MS,
            prints,
            tick: d("0.5"),
            rows_per_side: 1,
        })
        .expect("model")
        .last_price
    };
    assert_eq!(build(&forward), build(&backward));
}
