//! Integration test: recorded Binance JSON maps to engine trades.
//!
//! Reads the real recorded fixtures from disk so the on-the-wire JSON shape is
//! covered, not just inline strings.

use quantick_engine::Side;
use quantick_feed_binance::wire;

const REST: &str = include_str!("fixtures/rest_aggtrades.json");
const WS: &str = include_str!("fixtures/ws_aggtrade.json");

#[test]
fn recorded_rest_batch_maps_to_ordered_trades() {
    let raw = wire::parse_rest(REST).expect("REST fixture parses");
    let trades: Vec<_> = raw.iter().map(|r| r.to_trade().expect("maps")).collect();

    assert_eq!(trades.len(), 3);
    // agg_ids are strictly increasing, as Binance guarantees.
    for pair in trades.windows(2) {
        assert!(pair[1].agg_id > pair[0].agg_id);
    }
    // Aggressor inversion: m=true (rows 0, 2) => Sell; m=false (row 1) => Buy.
    assert_eq!(trades[0].side, Side::Sell);
    assert_eq!(trades[1].side, Side::Buy);
    assert_eq!(trades[2].side, Side::Sell);
}

#[test]
fn recorded_ws_message_maps() {
    let raw = wire::parse_ws_message(WS).expect("WS fixture parses");
    let trade = raw.to_trade().expect("maps");
    assert_eq!(trade.agg_id, 26131);
    assert_eq!(trade.side, Side::Buy);
    assert_eq!(trade.timestamp_ms, 1_700_000_000_450);
}
