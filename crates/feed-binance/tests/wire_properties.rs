//! Properties of the Binance wire decoders over untrusted text: no message
//! makes a parser or the mapping behind it panic, what is accepted keeps the
//! documented invariants, and every message Binance could send decodes back
//! to the values it was written from.

use proptest::prelude::*;
use quantick_engine::Side;
use quantick_feed_binance::depth::wire::{DepthLevel, parse_snapshot, parse_update};
use quantick_feed_binance::wire::{parse_rest, parse_ws_message};
use rust_decimal::Decimal;

/// Strings a decimal field might carry: honest decimals, near misses, and
/// the overflow and exponent shapes `Decimal::from_str` has to refuse.
fn decimal_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "-?[0-9]{1,40}(\\.[0-9]{0,40})?",
        "[0-9]{0,3}[.eE+-]{1,3}[0-9]{0,3}",
        Just("79228162514264337593543950336".to_owned()),
        Just("0.0000000000000000000000000001".to_owned()),
        Just("0.00000000000000000000000000001".to_owned()),
        Just("NaN".to_owned()),
        "\\PC{0,8}",
    ]
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

fn levels_json(levels: &[(String, String)]) -> String {
    let levels: Vec<String> = levels
        .iter()
        .map(|(price, quantity)| format!("[{},{}]", json_string(price), json_string(quantity)))
        .collect();
    format!("[{}]", levels.join(","))
}

/// Shapes close enough to a real message to reach the decimal parsing.
fn agg_trade_json() -> impl Strategy<Value = String> {
    (
        any::<u64>(),
        decimal_text(),
        decimal_text(),
        any::<i64>(),
        any::<bool>(),
    )
        .prop_map(|(id, price, quantity, time, maker)| {
            format!(
                r#"{{"e":"aggTrade","a":{id},"p":{},"q":{},"T":{time},"m":{maker}}}"#,
                json_string(&price),
                json_string(&quantity)
            )
        })
}

fn depth_side() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec((decimal_text(), decimal_text()), 0..6)
}

fn depth_update_json() -> impl Strategy<Value = String> {
    (
        any::<u64>(),
        any::<u64>(),
        prop::option::of(any::<u64>()),
        depth_side(),
        depth_side(),
    )
        .prop_map(|(first, last, previous, bids, asks)| {
            let previous = previous.map_or(String::new(), |id| format!(r#","pu":{id}"#));
            format!(
                r#"{{"E":1,"s":"BTCUSDT","U":{first},"u":{last}{previous},"b":{},"a":{}}}"#,
                levels_json(&bids),
                levels_json(&asks)
            )
        })
}

fn depth_snapshot_json() -> impl Strategy<Value = String> {
    (any::<u64>(), depth_side(), depth_side()).prop_map(|(id, bids, asks)| {
        format!(
            r#"{{"lastUpdateId":{id},"bids":{},"asks":{}}}"#,
            levels_json(&bids),
            levels_json(&asks)
        )
    })
}

/// A decimal as Binance writes it, from its mantissa and scale.
fn decimal(mantissa: i64, scale: u32) -> Decimal {
    Decimal::new(mantissa, scale)
}

fn exercise_text(text: &str) {
    if let Ok(trades) = parse_rest(text) {
        for trade in trades {
            let _ = trade.to_trade();
        }
    }
    if let Ok(trade) = parse_ws_message(text) {
        let _ = trade.to_trade();
    }
    if let Ok(update) = parse_update(text) {
        prop_assert_levels(&update.bids);
        prop_assert_levels(&update.asks);
        assert!(update.first_update_id <= update.final_update_id);
        let _ = update.to_book_delta();
    }
    if let Ok(snapshot) = parse_snapshot(text) {
        prop_assert_levels(&snapshot.bids);
        prop_assert_levels(&snapshot.asks);
        let _ = snapshot.to_book_snapshot(snapshot.bids.len().max(snapshot.asks.len()));
    }
}

/// What `parse_levels` promises of every level it accepts.
fn prop_assert_levels(levels: &[DepthLevel]) {
    for level in levels {
        assert!(level.price > Decimal::ZERO, "{level:?}");
        assert!(level.quantity >= Decimal::ZERO, "{level:?}");
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
        text in prop_oneof![agg_trade_json(), depth_update_json(), depth_snapshot_json()],
    ) {
        exercise_text(&text);
        exercise_text(&format!("[{text}]"));
    }

    #[test]
    fn an_agg_trade_decodes_to_the_values_it_was_written_from(
        id in any::<u64>(),
        price in 1_i64..=i64::MAX,
        quantity in 0_i64..=i64::MAX,
        scale in 0_u32..=18,
        time in any::<i64>(),
        maker in any::<bool>(),
    ) {
        let (price, quantity) = (decimal(price, scale), decimal(quantity, scale));
        let text = format!(
            r#"{{"e":"aggTrade","E":1,"s":"BTCUSDT","a":{id},"p":"{price}","q":"{quantity}","T":{time},"m":{maker}}}"#
        );
        let trade = parse_ws_message(&text).unwrap().to_trade().unwrap();
        prop_assert_eq!(trade.agg_id, id);
        prop_assert_eq!(trade.price, price);
        prop_assert_eq!(trade.quantity, quantity);
        prop_assert_eq!(trade.timestamp_ms, time);
        prop_assert_eq!(trade.side, if maker { Side::Sell } else { Side::Buy });
        let rest = parse_rest(&format!("[{text},{text}]")).unwrap();
        prop_assert_eq!(rest.len(), 2);
        prop_assert_eq!(rest[1].to_trade().unwrap(), trade);
    }

    #[test]
    fn a_depth_update_decodes_to_the_levels_it_was_written_from(
        first in any::<u64>(),
        span in any::<u32>(),
        bids in prop::collection::vec((1_i64..=i64::MAX, 0_i64..=i64::MAX, 0_u32..=18), 0..8),
        asks in prop::collection::vec((1_i64..=i64::MAX, 0_i64..=i64::MAX, 0_u32..=18), 0..8),
    ) {
        let last = first.saturating_add(u64::from(span));
        let side = |levels: &[(i64, i64, u32)]| -> Vec<(Decimal, Decimal)> {
            levels
                .iter()
                .map(|&(price, quantity, scale)| (decimal(price, scale), decimal(quantity, scale)))
                .collect()
        };
        let (bids, asks) = (side(&bids), side(&asks));
        let text = |levels: &[(Decimal, Decimal)]| {
            levels_json(
                &levels
                    .iter()
                    .map(|(price, quantity)| (price.to_string(), quantity.to_string()))
                    .collect::<Vec<_>>(),
            )
        };
        let update = parse_update(&format!(
            r#"{{"E":1,"s":"BTCUSDT","U":{first},"u":{last},"b":{},"a":{}}}"#,
            text(&bids),
            text(&asks)
        ))
        .unwrap();
        let decoded = |levels: &[DepthLevel]| -> Vec<(Decimal, Decimal)> {
            levels.iter().map(|level| (level.price, level.quantity)).collect()
        };
        prop_assert_eq!((update.first_update_id, update.final_update_id), (first, last));
        prop_assert_eq!(decoded(&update.bids), bids);
        prop_assert_eq!(decoded(&update.asks), asks);
    }
}
