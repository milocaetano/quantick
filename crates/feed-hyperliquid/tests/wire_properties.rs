//! Properties of the Hyperliquid wire decoders over untrusted text: no message
//! makes a parser or the mapper behind it panic, a book image past the
//! per-side level cap is refused, and every trade or book the venue could
//! send decodes back to the values it was written from.

use proptest::prelude::*;
use quantick_engine::Side;
use quantick_feed_hyperliquid::depth::{
    BookMapError, BookMapper, BookMessage, DepthEvent, HYPERLIQUID_LEVELS_PER_SIDE,
    parse_book_message,
};
use quantick_feed_hyperliquid::{
    TradeMapper, TradeMessage, parse_rest_trades, parse_trade_message,
};
use rust_decimal::Decimal;

/// Strings a price or size field might carry: honest decimals, near misses
/// and the overflow shapes `Decimal::from_str` has to refuse.
fn decimal_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "-?[0-9]{1,40}(\\.[0-9]{0,40})?",
        "[0-9]{0,3}[.eE+-]{1,3}[0-9]{0,3}",
        Just("79228162514264337593543950336".to_owned()),
        "\\PC{0,8}",
    ]
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

fn trade_json(coin: &str, side: &str, price: &str, size: &str, time: i64, tid: u64) -> String {
    format!(
        r#"{{"coin":{},"side":{},"px":{},"sz":{},"time":{time},"tid":{tid},"hash":"0x0"}}"#,
        json_string(coin),
        json_string(side),
        json_string(price),
        json_string(size)
    )
}

fn level_json(price: &str, size: &str, orders: u64) -> String {
    format!(
        r#"{{"px":{},"sz":{},"n":{orders}}}"#,
        json_string(price),
        json_string(size)
    )
}

fn book_json(coin: &str, time: i64, bids: &[String], asks: &[String]) -> String {
    format!(
        r#"{{"channel":"l2Book","data":{{"coin":{},"time":{time},"levels":[[{}],[{}]]}}}}"#,
        json_string(coin),
        bids.join(","),
        asks.join(",")
    )
}

fn near_valid_trades() -> impl Strategy<Value = String> {
    let trade = (
        prop_oneof![Just("BTC".to_owned()), "\\PC{0,4}"],
        prop_oneof![Just("B".to_owned()), Just("A".to_owned()), "\\PC{0,2}"],
        decimal_text(),
        decimal_text(),
        any::<i64>(),
        any::<u64>(),
    )
        .prop_map(|(coin, side, price, size, time, tid)| {
            trade_json(&coin, &side, &price, &size, time, tid)
        });
    prop::collection::vec(trade, 0..6)
        .prop_map(|trades| format!(r#"{{"channel":"trades","data":[{}]}}"#, trades.join(",")))
}

fn book_side() -> impl Strategy<Value = Vec<String>> {
    let level = (decimal_text(), decimal_text(), any::<u64>())
        .prop_map(|(price, size, orders)| level_json(&price, &size, orders));
    prop::collection::vec(level, 0..=HYPERLIQUID_LEVELS_PER_SIDE + 3)
}

fn near_valid_book() -> impl Strategy<Value = String> {
    (
        prop_oneof![Just("BTC".to_owned()), "\\PC{0,4}"],
        any::<i64>(),
        book_side(),
        book_side(),
    )
        .prop_map(|(coin, time, bids, asks)| book_json(&coin, time, &bids, &asks))
}

fn event_levels(event: &DepthEvent) -> (usize, usize) {
    match event {
        DepthEvent::Snapshot { snapshot, .. } => (snapshot.bids().len(), snapshot.asks().len()),
        DepthEvent::Update { delta, .. } => (delta.bids().len(), delta.asks().len()),
        _ => (0, 0),
    }
}

/// Every decoder and the mapper behind it, fed the same text; none may panic,
/// and the book mapper's level cap must hold on what it accepts.
fn exercise_text(text: &str) {
    if let Ok(TradeMessage::Trades(trades)) = parse_trade_message(text) {
        let _ = TradeMapper::new("BTC").map_batch(trades);
    }
    if let Ok(trades) = parse_rest_trades(text) {
        let _ = TradeMapper::new("BTC").map_batch(trades);
    }
    if let Ok(BookMessage::Book(image)) = parse_book_message(text) {
        let mut mapper = BookMapper::new("BTC", 1);
        let too_many = image
            .levels
            .iter()
            .any(|side| side.len() > HYPERLIQUID_LEVELS_PER_SIDE);
        match mapper.map(&image) {
            Ok(Some(event)) => {
                let (bids, asks) = event_levels(&event);
                assert!(bids <= HYPERLIQUID_LEVELS_PER_SIDE && asks <= HYPERLIQUID_LEVELS_PER_SIDE);
                assert!(!too_many);
            }
            Ok(None) => assert!(!too_many),
            Err(BookMapError::LevelCount { got, max, .. }) => assert!(got > max),
            Err(_) => {}
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_never_panics_a_decoder(text in "\\PC{0,256}") {
        exercise_text(&text);
    }

    #[test]
    fn near_valid_messages_never_panic_a_decoder(
        text in prop_oneof![near_valid_trades(), near_valid_book()],
    ) {
        exercise_text(&text);
    }

    #[test]
    fn a_trade_decodes_to_the_values_it_was_written_from(
        price in 1_i64..=i64::MAX,
        size in 1_i64..=i64::MAX,
        scale in 0_u32..=18,
        time in 0_i64..=i64::MAX,
        tid in any::<u64>(),
        buy in any::<bool>(),
    ) {
        let (price, size) = (Decimal::new(price, scale), Decimal::new(size, scale));
        let side = if buy { "B" } else { "A" };
        let trade = trade_json("BTC", side, &price.to_string(), &size.to_string(), time, tid);
        let message = format!(r#"{{"channel":"trades","data":[{trade}]}}"#);
        let TradeMessage::Trades(raw) = parse_trade_message(&message).unwrap() else {
            panic!("a trades envelope decodes as trades");
        };
        prop_assert_eq!(&parse_rest_trades(&format!("[{trade}]")).unwrap(), &raw);
        let mapped = TradeMapper::new("btc").map_batch(raw);
        prop_assert!(mapped.errors.is_empty(), "{:?}", mapped.errors);
        prop_assert_eq!(mapped.trades.len(), 1);
        let mapped = &mapped.trades[0];
        prop_assert_eq!(mapped.price, price);
        prop_assert_eq!(mapped.quantity, size);
        prop_assert_eq!(mapped.timestamp_ms, time);
        prop_assert_eq!(mapped.side, if buy { Side::Buy } else { Side::Sell });
    }

    #[test]
    fn a_book_decodes_to_the_levels_it_was_written_from(
        bids in prop::collection::btree_set(1_i64..1_000, 0..=HYPERLIQUID_LEVELS_PER_SIDE),
        asks in prop::collection::btree_set(1_000_i64..2_000, 0..=HYPERLIQUID_LEVELS_PER_SIDE),
        size in 1_i64..1_000_000,
        time in 0_i64..=i64::MAX,
    ) {
        // Bids best first (descending), asks best first (ascending), as sent.
        let bids: Vec<i64> = bids.into_iter().rev().collect();
        let asks: Vec<i64> = asks.into_iter().collect();
        let json = |prices: &[i64]| -> Vec<String> {
            prices
                .iter()
                .map(|&price| level_json(&Decimal::new(price, 1).to_string(), &Decimal::new(size, 3).to_string(), 1))
                .collect()
        };
        let message = book_json("BTC", time, &json(&bids), &json(&asks));
        let BookMessage::Book(image) = parse_book_message(&message).unwrap() else {
            panic!("an l2Book envelope decodes as a book");
        };
        let event = BookMapper::new("BTC", 1).map(&image).unwrap();
        let Some(DepthEvent::Snapshot { snapshot, .. }) = event else {
            // An empty first image is the only one that publishes nothing.
            prop_assert!(event.is_none() && bids.is_empty() && asks.is_empty());
            return Ok(());
        };
        let prices = |levels: &[quantick_orderbook::BookLevel]| -> Vec<Decimal> {
            levels.iter().map(|level| level.price()).collect()
        };
        let mut sent_bids: Vec<Decimal> = bids.iter().map(|&price| Decimal::new(price, 1)).collect();
        let mut sent_asks: Vec<Decimal> = asks.iter().map(|&price| Decimal::new(price, 1)).collect();
        let (mut got_bids, mut got_asks) = (prices(snapshot.bids()), prices(snapshot.asks()));
        for side in [&mut sent_bids, &mut sent_asks, &mut got_bids, &mut got_asks] {
            side.sort();
        }
        prop_assert_eq!(got_bids, sent_bids);
        prop_assert_eq!(got_asks, sent_asks);
    }
}
