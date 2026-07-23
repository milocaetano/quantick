//! Recorded Binance depth payloads decode without network access.

use rust_decimal::Decimal;

use quantick_feed_binance::depth::{parse_snapshot, parse_update};
use quantick_orderbook::BookCoverage;

const SNAPSHOT: &str = include_str!("fixtures/rest_depth_snapshot.json");
const UPDATE: &str = include_str!("fixtures/ws_depth_update.json");

#[test]
fn recorded_snapshot_preserves_decimal_levels() {
    let snapshot = parse_snapshot(SNAPSHOT).expect("snapshot fixture parses");
    assert_eq!(snapshot.last_update_id, 1_027_024);
    assert_eq!(snapshot.bids.len(), 2);
    assert_eq!(snapshot.asks.len(), 2);
    assert_eq!(snapshot.bids[0].price, Decimal::new(3_600_010_000_000, 8));
    assert_eq!(snapshot.bids[0].quantity, Decimal::new(125_000_000, 8));

    let book = snapshot
        .to_book_snapshot(5_000)
        .expect("maps to order-book snapshot");
    assert_eq!(book.last_update_id(), 1_027_024);
    assert_eq!(
        book.coverage(),
        BookCoverage::Limited {
            levels_per_side: 5_000
        }
    );
}

#[test]
fn recorded_diff_preserves_ids_and_absolute_zero() {
    let update = parse_update(UPDATE).expect("update fixture parses");
    assert_eq!(update.symbol, "BTCUSDT");
    assert_eq!(update.event_time_ms, 1_700_000_000_500);
    assert_eq!(update.first_update_id, 1_027_025);
    assert_eq!(update.final_update_id, 1_027_027);
    assert_eq!(update.bids[1].quantity, Decimal::ZERO);

    let delta = update.to_book_delta().expect("maps to order-book delta");
    assert_eq!(delta.first_update_id(), 1_027_025);
    assert_eq!(delta.final_update_id(), 1_027_027);
    assert_eq!(delta.bids()[1].quantity(), Decimal::ZERO);
}
