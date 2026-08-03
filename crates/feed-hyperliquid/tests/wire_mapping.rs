//! Recorded public payloads cover the real wire shapes without network access.

use quantick_engine::Side;
use quantick_feed_hyperliquid::{
    TradeMapper, TradeMessage,
    depth::{BookMapper, BookMessage, HYPERLIQUID_LEVELS_PER_SIDE, parse_book_message},
    parse_rest_trades, parse_trade_message,
};
use quantick_orderbook::{BookCoverage, DepthEvent};
use rust_decimal::Decimal;

const REST_TRADES: &str = include_str!("fixtures/rest_recent_trades.json");
const WS_TRADES: &str = include_str!("fixtures/ws_trades.json");
const WS_BOOK: &str = include_str!("fixtures/ws_l2_book.json");

#[test]
fn recorded_rest_window_maps_oldest_first_with_true_aggressor_sides() {
    let raw = parse_rest_trades(REST_TRADES).expect("recorded REST trades decode");
    let mut mapper = TradeMapper::new("BTC");
    let batch = mapper.map_batch(raw);

    assert!(batch.errors.is_empty());
    assert_eq!(batch.trades.len(), 2);
    assert!(batch.trades[0].timestamp_ms < batch.trades[1].timestamp_ms);
    assert_eq!(batch.trades[0].side, Side::Buy);
    assert_eq!(batch.trades[1].side, Side::Sell);
    assert_eq!(batch.trades[1].quantity, Decimal::new(18, 4));
}

#[test]
fn recorded_websocket_trade_batch_is_sorted_by_the_mapper() {
    let TradeMessage::Trades(raw) =
        parse_trade_message(WS_TRADES).expect("recorded trade frame decodes")
    else {
        panic!("expected trades channel");
    };
    let mut mapper = TradeMapper::new("BTC");
    let batch = mapper.map_batch(raw);
    assert_eq!(batch.trades.len(), 2);
    assert_eq!(batch.trades[0].side, Side::Sell);
    assert_eq!(batch.trades[1].side, Side::Buy);
    assert_eq!(batch.trades[0].agg_id, 1);
    assert_eq!(batch.trades[1].agg_id, 2);
}

#[test]
fn recorded_l2_image_becomes_a_limited_neutral_snapshot() {
    let BookMessage::Book(image) = parse_book_message(WS_BOOK).expect("recorded book decodes")
    else {
        panic!("expected l2Book channel");
    };
    let mut mapper = BookMapper::new("BTC", 11);
    let Some(DepthEvent::Snapshot {
        generation,
        observed_at_ms,
        snapshot,
        ..
    }) = mapper.map(&image).expect("book maps")
    else {
        panic!("expected first image snapshot");
    };
    assert_eq!(generation, 11);
    assert_eq!(observed_at_ms, 1_785_648_733_832);
    assert_eq!(snapshot.bids().len(), 3);
    assert_eq!(snapshot.asks().len(), 3);
    assert_eq!(
        snapshot.coverage(),
        BookCoverage::Limited {
            levels_per_side: HYPERLIQUID_LEVELS_PER_SIDE
        }
    );
}
