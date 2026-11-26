//! Properties of the execution channel's lines over untrusted input: no line
//! the bridge sends makes the decoder or the venue panic, none makes the
//! venue write an order of its own accord, and every request this side
//! writes is one line that decodes back to the values it was built from.

mod support;

use proptest::prelude::*;
use quantick_broker_mt5::wire::{Request, Stamp, decimal, decode, replay_only, side};
use quantick_engine::Side;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use support::{SYMBOL, Snapshot, connected};

/// The request types that place, change or remove an order or a position.
const TRADING_TYPES: [&str; 5] = ["submit", "cancel", "modify", "sltp", "close"];

/// Text a decimal or enum string field might carry, valid or not.
fn field_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "-?[0-9]{1,30}(\\.[0-9]{0,30})?",
        " [0-9]{1,3} ",
        "[0-9]{0,3}[.eE+-]{1,3}[0-9]{0,3}",
        Just("79228162514264337593543950336".to_owned()),
        Just("buy".to_owned()),
        Just("sell".to_owned()),
        Just("limit".to_owned()),
        Just("stop".to_owned()),
        Just("in".to_owned()),
        Just("out".to_owned()),
        "\\PC{0,6}",
    ]
}

fn ticket() -> impl Strategy<Value = u64> {
    prop_oneof![Just(0), Just(1), Just(u64::MAX), any::<u64>()]
}

fn position_line() -> impl Strategy<Value = Value> {
    (
        ticket(),
        field_text(),
        field_text(),
        field_text(),
        any::<u64>(),
    )
        .prop_map(|(ticket, side, volume, price, magic)| {
            json!({"ticket": ticket, "side": side, "volume": volume, "price_open": price,
                   "sl": null, "tp": price, "magic": magic, "time_msc": 0})
        })
}

fn order_line() -> impl Strategy<Value = Value> {
    (
        ticket(),
        field_text(),
        field_text(),
        field_text(),
        field_text(),
    )
        .prop_map(|(ticket, side, kind, volume, price)| {
            json!({"ticket": ticket, "side": side, "kind": kind, "volume": volume, "price": price,
                   "sl": price, "tp": null, "magic": support::MAGIC, "time_setup_msc": 0})
        })
}

fn deal_line() -> impl Strategy<Value = Value> {
    (
        ticket(),
        ticket(),
        field_text(),
        field_text(),
        field_text(),
        field_text(),
    )
        .prop_map(|(ticket, order, side, entry, volume, price)| {
            json!({"ticket": ticket, "order": order, "position_id": order, "side": side,
                   "entry": entry, "reason": "expert", "volume": volume, "price": price,
                   "magic": support::MAGIC, "time_msc": 0})
        })
}

fn state_line() -> impl Strategy<Value = Value> {
    (
        prop::collection::vec(position_line(), 0..3),
        prop::collection::vec(order_line(), 0..3),
        prop::collection::vec(deal_line(), 0..3),
        prop::option::of((field_text(), field_text(), field_text())),
    )
        .prop_map(|(positions, orders, deals, account)| {
            let mut line = Snapshot::new().line();
            line["positions"] = Value::Array(positions);
            line["orders"] = Value::Array(orders);
            line["deals"] = Value::Array(deals);
            if let Some((trade_mode, margin_mode, balance)) = account {
                line["account"]["trade_mode"] = json!(trade_mode);
                line["account"]["margin_mode"] = json!(margin_mode);
                line["account"]["balance"] = json!(balance);
            }
            line
        })
}

fn reply_line() -> impl Strategy<Value = Value> {
    (
        prop::option::of("\\PC{0,8}"),
        prop::option::of(any::<i64>()),
        ticket(),
        any::<bool>(),
    )
        .prop_map(|(id, retcode, order, unknown)| {
            json!({"type": "reply", "id": id, "retcode": retcode, "comment": "c",
                   "order": order, "deal": order, "unknown": unknown})
        })
}

fn bridge_line() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => state_line().prop_map(|line| line.to_string()),
        2 => reply_line().prop_map(|line| line.to_string()),
        1 => (any::<u32>(), "\\PC{0,8}").prop_map(|(schema, instance)| {
            json!({"type": "exec_hello", "schema": schema, "instance": instance}).to_string()
        }),
        1 => "\\PC{0,8}".prop_map(|reason| json!({"type": "bye", "reason": reason}).to_string()),
        1 => "\\PC{0,64}",
    ]
}

fn positive_decimal() -> impl Strategy<Value = Decimal> {
    (1_i64..=i64::MAX, 0_u32..=18).prop_map(|(mantissa, scale)| Decimal::new(mantissa, scale))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_never_panics_a_decoder(text in "\\PC{0,256}") {
        let _ = decode(&text);
        let _ = decimal(&text);
        let _ = side(&text);
        let replayed = replay_only(&text);
        if !replayed.is_empty() {
            prop_assert!(!replayed.contains('\n'));
            let value: Value = serde_json::from_str(&replayed).unwrap();
            prop_assert_eq!(&value["replay_only"], &Value::Bool(true));
        }
    }

    #[test]
    fn a_decimal_field_reads_exactly_what_was_written(value in positive_decimal(), negate in any::<bool>()) {
        let value = if negate { -value } else { value };
        prop_assert_eq!(decimal(&value.to_string()), Some(value));
        prop_assert_eq!(decimal(&format!(" {value} ")), Some(value));
    }

    #[test]
    fn a_submit_encodes_as_one_line_holding_its_values(
        id in "[ -~]{1,24}",
        buy in any::<bool>(),
        volume in positive_decimal(),
        price in prop::option::of(positive_decimal()),
        sl in prop::option::of(positive_decimal()),
        magic in any::<u64>(),
        deviation_points in any::<u32>(),
        expires_ms in prop::option::of(any::<i64>()),
    ) {
        let side_wanted = if buy { Side::Buy } else { Side::Sell };
        let request = Request::Submit {
            symbol: SYMBOL.into(),
            side: side_wanted,
            kind: "limit",
            volume,
            price,
            sl,
            tp: None,
        };
        let line = request.encode(&id, Stamp { magic, deviation_points, expires_ms });
        prop_assert!(!line.contains('\n'));
        let value: Value = serde_json::from_str(&line).unwrap();
        let text = |key: &str| value[key].as_str();
        prop_assert_eq!(text("id"), Some(id.as_str()));
        prop_assert_eq!(text("type"), Some("submit"));
        prop_assert_eq!(text("side").and_then(side), Some(side_wanted));
        prop_assert_eq!(text("volume").and_then(decimal), Some(volume));
        prop_assert_eq!(text("price").and_then(decimal), price);
        prop_assert_eq!(text("sl").and_then(decimal), sl);
        prop_assert_eq!(&value["tp"], &Value::Null);
        prop_assert_eq!(value["magic"].as_u64(), Some(magic));
        prop_assert_eq!(value["deviation"].as_u64(), Some(u64::from(deviation_points)));
        prop_assert_eq!(value["expires_ms"].as_i64(), expires_ms);
        // Marked for a replay, the same request keeps every field.
        let mut replayed: Value = serde_json::from_str(&replay_only(&line)).unwrap();
        prop_assert_eq!(replayed["replay_only"].take(), Value::Bool(true));
        replayed.as_object_mut().unwrap().remove("replay_only");
        prop_assert_eq!(replayed, value);
    }
}

proptest! {
    // Each case drives a whole venue session.
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn no_bridge_line_panics_the_venue_or_writes_an_order(
        lines in prop::collection::vec(bridge_line(), 1..6),
        now_ms in prop_oneof![Just(0_i64), Just(i64::MAX), any::<i64>()],
    ) {
        let (mut venue, handle) = connected();
        for line in &lines {
            handle.push_raw(line);
            venue.poll();
            venue.tick(now_ms);
        }
        let written: Vec<Value> = handle
            .take_sent()
            .into_iter()
            .filter(|line| TRADING_TYPES.contains(&line["type"].as_str().unwrap_or_default()))
            .collect();
        prop_assert!(written.is_empty(), "{written:?} after {lines:?}");
    }
}
