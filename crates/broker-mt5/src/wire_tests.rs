use super::*;

#[test]
fn a_submit_line_carries_decimals_as_strings_and_the_stamp() {
    let line = Request::Submit {
        symbol: "WINZ26".into(),
        side: Side::Buy,
        kind: "limit",
        volume: Decimal::from(2),
        price: Some(Decimal::new(1_249_000, 1)),
        sl: None,
        tp: Some(Decimal::from(125_300)),
    }
    .encode(
        "ab12-7",
        Stamp {
            magic: 7_710_001,
            deviation_points: 10,
            expires_ms: Some(1_763_380_806_000),
        },
    );
    let value: Value = serde_json::from_str(&line).expect("json");
    assert_eq!(value["type"], "submit");
    assert_eq!(value["id"], "ab12-7");
    assert_eq!(value["price"], "124900");
    assert_eq!(value["volume"], "2");
    assert_eq!(value["sl"], Value::Null);
    assert_eq!(value["tp"], "125300");
    assert_eq!(value["magic"], 7_710_001);
    assert_eq!(value["deviation"], 10);
    assert_eq!(value["comment"], "ab12-7");
    assert_eq!(value["expires_ms"], 1_763_380_806_000_i64);
}

/// The example lines in PROTOCOL.md's execution-channel section: every
/// bridge line decodes, and every request line is exactly what this side
/// encodes. A doc that drifts from the code fails here.
#[test]
fn protocol_md_examples_match_the_codec() {
    let doc = include_str!("../../../bridge/mt5/PROTOCOL.md");
    let section = doc
        .split("## Execution channel")
        .nth(1)
        .expect("the execution channel section");
    let lines: Vec<&str> = section
        .lines()
        .filter(|line| line.starts_with("{\"type\":"))
        .collect();
    let stamp = Stamp {
        magic: 7_710_001,
        deviation_points: 10,
        expires_ms: Some(1_763_380_806_000),
    };
    let symbol = || "WINZ26".to_owned();
    let expected = [
        Request::Watch { symbol: symbol() }.encode("a1b2-1", stamp),
        Request::Submit {
            symbol: symbol(),
            side: Side::Buy,
            kind: "market",
            volume: Decimal::from(2),
            price: None,
            sl: Some(Decimal::from(124_800)),
            tp: None,
        }
        .encode("a1b2-2", stamp),
        Request::Submit {
            symbol: symbol(),
            side: Side::Sell,
            kind: "limit",
            volume: Decimal::ONE,
            price: Some(Decimal::from(125_300)),
            sl: None,
            tp: None,
        }
        .encode("a1b2-3", stamp),
        Request::Cancel { order: 601 }.encode("a1b2-4", stamp),
        Request::Modify {
            order: 601,
            price: Decimal::from(125_250),
            sl: None,
            tp: Some(Decimal::from(124_900)),
        }
        .encode("a1b2-5", stamp),
        Request::PositionStops {
            symbol: symbol(),
            position: 77,
            sl: Some(Decimal::from(124_800)),
            tp: Some(Decimal::from(125_400)),
        }
        .encode("a1b2-6", stamp),
        Request::Close {
            position: 77,
            volume: Decimal::ONE,
        }
        .encode("a1b2-7", stamp),
        replay_only(
            &Request::Submit {
                symbol: symbol(),
                side: Side::Buy,
                kind: "market",
                volume: Decimal::from(2),
                price: None,
                sl: Some(Decimal::from(124_800)),
                tp: None,
            }
            .encode("a1b2-2", stamp),
        ),
    ];
    let mut requests = Vec::new();
    for line in lines {
        let value: Value = serde_json::from_str(line).expect("doc line is json");
        match value["type"].as_str() {
            Some("exec_hello" | "reply" | "state") => {
                assert!(decode(line).is_some(), "doc line decodes: {line}");
            }
            _ => requests.push(value),
        }
    }
    let encoded: Vec<Value> = expected
        .iter()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect();
    assert_eq!(requests, encoded);
}

#[test]
fn a_replay_only_line_is_the_line_plus_the_flag() {
    let line = Request::Cancel { order: 601 }.encode(
        "a1b2-4",
        Stamp {
            magic: 1,
            deviation_points: 1,
            expires_ms: None,
        },
    );
    let replay: Value = serde_json::from_str(&replay_only(&line)).expect("json");
    assert_eq!(replay["replay_only"], true);
    assert_eq!(replay["id"], "a1b2-4");
    assert_eq!(replay["order"], 601);
    let unknown = r#"{"type":"reply","id":"a1b2-4","retcode":null,"unknown":true,"error":"never received: not executed","cached":false}"#;
    let Some(Inbound::Reply(reply)) = decode(unknown) else {
        panic!("a reply");
    };
    assert!(reply.unknown);
}

#[test]
fn the_protocol_examples_decode() {
    let hello = r#"{"type":"exec_hello","schema":2,"bridge":"quantick-mt5-exec-py","bridge_version":"0.1.0","instance":"9f1c"}"#;
    assert_eq!(
        decode(hello),
        Some(Inbound::ExecHello {
            schema: 2,
            instance: "9f1c".into()
        })
    );
    let reply = r#"{"type":"reply","id":"ab12-7","retcode":10016,"comment":"Invalid stops","order":0,"deal":0,"volume":"0","price":"0","cached":false}"#;
    let Some(Inbound::Reply(reply)) = decode(reply) else {
        panic!("a reply");
    };
    assert_eq!(reply.retcode, Some(10016));
    assert_eq!(reply.comment.as_deref(), Some("Invalid stops"));
    let refused = r#"{"type":"reply","id":"ab12-8","retcode":null,"error":"position 77 is not open: nothing to close","cached":false}"#;
    let Some(Inbound::Reply(refused)) = decode(refused) else {
        panic!("a refusal");
    };
    assert_eq!(refused.retcode, None);
    assert!(refused.error.is_some());
    let state = r#"{"type":"state","symbol":"WINZ26","account":{"login":1234567,"server":"Broker-DEMO","company":"Broker Ltd","trade_mode":"demo","margin_mode":"exchange","balance":"100000.00","currency":"BRL"},"positions":[{"ticket":77,"side":"buy","volume":"2","price_open":"125000","sl":"124800","tp":null,"magic":7710001,"time_msc":1000}],"orders":[],"deals":[],"closed_orders":[]}"#;
    let Some(Inbound::State(state)) = decode(state) else {
        panic!("a state");
    };
    assert_eq!(state.positions[0].sl.as_deref(), Some("124800"));
    assert_eq!(state.account.map(|a| a.login), Some(1_234_567));
    assert_eq!(decode("{\"type\":\"shout\"}"), None);
    assert_eq!(decode("not json"), None);
}
