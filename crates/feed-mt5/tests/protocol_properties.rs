//! Properties of the MT5 bridge line protocol over untrusted input: no line
//! makes the decoder or the mappers behind it panic, the line reader never
//! hands over more than [`MAX_LINE_BYTES`] and frames exactly the lines that
//! fit, and every message either side writes decodes to what it was built
//! from.

use proptest::prelude::*;
use quantick_feed_mt5::{
    BookMapper, BoundedLine, BoundedLineReader, BridgeMsg, FeedMsg, MAX_LINE_BYTES, RateMapper,
    SettlementReadings, SideMode, TickMapper, encode_line, parse_line, report_for_line,
};

/// Text a numeric string field might carry, valid or not.
fn number_text() -> impl Strategy<Value = String> {
    prop_oneof![
        "-?[0-9]{1,30}(\\.[0-9]{0,30})?",
        "[0-9]{0,3}[.eE+-]{1,3}[0-9]{0,3}",
        Just("79228162514264337593543950336".to_owned()),
        "\\PC{0,6}",
    ]
}

fn extreme_i64() -> impl Strategy<Value = i64> {
    prop_oneof![
        Just(i64::MIN),
        Just(i64::MAX),
        Just(i64::MAX - 1),
        Just(0),
        any::<i64>(),
    ]
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

fn wire_level() -> impl Strategy<Value = String> {
    (number_text(), number_text())
        .prop_map(|(price, size)| format!("[{},{}]", json_string(&price), json_string(&size)))
}

/// Lines shaped like every bridge message, with untrusted field values, so
/// decoding gets past the tag and the mappers see hostile numbers.
fn near_valid_line() -> impl Strategy<Value = String> {
    let tick = (
        any::<u64>(),
        extreme_i64(),
        number_text(),
        number_text(),
        number_text(),
        any::<u64>(),
        any::<u32>(),
    )
        .prop_map(|(seq, time, bid, ask, last, volume, flags)| {
            format!(
                r#"{{"type":"tick","seq":{seq},"time_ms":{time},"bid":{},"ask":{},"last":{},"volume":{volume},"flags":{flags}}}"#,
                json_string(&bid),
                json_string(&ask),
                json_string(&last)
            )
        });
    let book = (
        any::<u64>(),
        extreme_i64(),
        prop::collection::vec(wire_level(), 0..6),
        prop::collection::vec(wire_level(), 0..6),
    )
        .prop_map(|(seq, time, bids, asks)| {
            format!(
                r#"{{"type":"book","seq":{seq},"time_ms":{time},"bids":[{}],"asks":[{}]}}"#,
                bids.join(","),
                asks.join(",")
            )
        });
    let row = (
        extreme_i64(),
        number_text(),
        number_text(),
        number_text(),
        number_text(),
        number_text(),
    )
        .prop_map(|(time, open, high, low, close, volume)| {
            format!(
                "[{time},{},{},{},{},{}]",
                json_string(&open),
                json_string(&high),
                json_string(&low),
                json_string(&close),
                json_string(&volume)
            )
        });
    let rate = prop::collection::vec(row, 0..6)
        .prop_map(|rows| format!(r#"{{"type":"rate","bars":[{}]}}"#, rows.join(",")));
    let settlement = (prop::option::of(number_text()), extreme_i64()).prop_map(|(price, time)| {
        let price = price.map_or("null".to_owned(), |price| json_string(&price));
        format!(r#"{{"type":"settlement","price":{price},"time_ms":{time}}}"#)
    });
    let hello = (extreme_i64(), prop::option::of(number_text()), any::<Option<u32>>()).prop_map(
        |(offset, tick_size, levels)| {
            let tick_size = tick_size.map_or("null".to_owned(), |size| json_string(&size));
            let levels = levels.map_or("null".to_owned(), |levels| levels.to_string());
            format!(
                r#"{{"type":"hello","schema":1,"bridge":"b","bridge_version":"1","symbol":"WIN","broker_symbol":"WINZ26","digits":0,"server_utc_offset_s":{offset},"tick_size":{tick_size},"book_levels":{levels}}}"#
            )
        },
    );
    let log = ("\\PC{0,12}", "\\PC{0,12}").prop_map(|(code, message)| {
        format!(
            r#"{{"event_code":{},"message":{},"severity":"error"}}"#,
            json_string(&code),
            json_string(&message)
        )
    });
    prop_oneof![tick, book, rate, settlement, hello, log]
}

/// Decode `line` and push what it holds through the mapper that would see it,
/// under a server offset and rate interval the bridge also declares.
fn exercise_line(line: &str, offset_s: i64, interval_ms: i64) {
    let _ = report_for_line(line);
    let Ok(message) = parse_line(line) else {
        return;
    };
    let mut ticks = TickMapper::new(SideMode::TickRule, offset_s);
    let mut flagged = TickMapper::new(SideMode::Flags, offset_s);
    match message {
        BridgeMsg::Hello(hello) => {
            let mut book = BookMapper::new(
                hello.symbol,
                1,
                hello.book_levels,
                hello.tick_size.as_deref(),
                hello.server_utc_offset_s,
            );
            book.set_server_utc_offset_s(offset_s);
            let _ =
                TickMapper::new(SideMode::TickRule, hello.server_utc_offset_s).to_utc_ms(i64::MAX);
        }
        BridgeMsg::Tick(tick) => {
            let _ = ticks.map(&tick);
            let _ = ticks.map(&tick);
            let _ = flagged.map(&tick);
        }
        BridgeMsg::Book(image) => {
            let mut book = BookMapper::new("WIN", 1, Some(5), Some("5"), offset_s);
            let _ = book.map(&image);
            let _ = book.map(&image);
        }
        BridgeMsg::Rate(chunk) => {
            let mut rates = RateMapper::new(interval_ms, offset_s);
            for row in &chunk.bars {
                let _ = rates.map(row);
            }
        }
        BridgeMsg::Settlement(reading) => {
            let mut readings = SettlementReadings::default();
            let to_utc = |server_ms| ticks.to_utc_ms(server_ms);
            let _ = readings.read(&reading, to_utc);
            let _ = readings.read(&reading, to_utc);
        }
        _ => {}
    }
}

/// Every line a [`BoundedLineReader`] yields from `bytes`, read to the end.
fn read_all(bytes: Vec<u8>) -> Vec<BoundedLine> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut reader = BoundedLineReader::new(std::io::Cursor::new(bytes));
        let mut lines = Vec::new();
        loop {
            match reader.next_line().await.unwrap() {
                BoundedLine::Eof => return lines,
                line => lines.push(line),
            }
        }
    })
}

/// What the reader promises for `lines` joined by `\n`: each line that fits
/// arrives whole, without a trailing `\r`; each that does not is one
/// `TooLong`, and nothing of it leaks into the lines after it.
fn expected_lines(lines: &[Vec<u8>], terminated: bool) -> Vec<BoundedLine> {
    let last = lines.len().saturating_sub(1);
    lines
        .iter()
        .enumerate()
        .filter(|(index, line)| *index != last || terminated || !line.is_empty())
        .map(|(_, line)| {
            if line.len() > MAX_LINE_BYTES {
                return BoundedLine::TooLong;
            }
            let mut line = line.clone();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            match String::from_utf8(line) {
                Ok(text) => BoundedLine::Line(text),
                Err(error) => BoundedLine::NotUtf8 {
                    len: error.as_bytes().len(),
                },
            }
        })
        .collect()
}

/// A line body: short text, invalid UTF-8, or a run sized around the cap.
fn line_body() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![
        4 => "[a-z{}\":,\r]{0,24}".prop_map(String::into_bytes),
        1 => prop::collection::vec(any::<u8>(), 0..24)
            .prop_map(|mut bytes| { bytes.retain(|&byte| byte != b'\n'); bytes }),
        1 => (MAX_LINE_BYTES - 2..=MAX_LINE_BYTES + 2).prop_map(|len| vec![b'x'; len]),
        1 => (MAX_LINE_BYTES..=4 * MAX_LINE_BYTES).prop_map(|len| vec![b'y'; len]),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_text_never_panics_the_decoder(
        line in "\\PC{0,256}",
        offset_s in extreme_i64(),
        interval_ms in extreme_i64(),
    ) {
        exercise_line(&line, offset_s, interval_ms);
    }

    #[test]
    fn near_valid_lines_never_panic_the_decoder_or_a_mapper(
        line in near_valid_line(),
        offset_s in extreme_i64(),
        interval_ms in extreme_i64(),
    ) {
        exercise_line(&line, offset_s, interval_ms);
    }

    #[test]
    fn a_load_older_request_encodes_as_one_line_holding_its_fields(
        count in any::<u64>(),
        before_ms in any::<i64>(),
    ) {
        let line = encode_line(&FeedMsg::LoadOlder { count, before_ms });
        prop_assert!(line.ends_with('\n'));
        prop_assert_eq!(line.matches('\n').count(), 1);
        prop_assert!(line.len() <= MAX_LINE_BYTES);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        prop_assert_eq!(value["type"].as_str(), Some("load_older"));
        prop_assert_eq!(value["count"].as_u64(), Some(count));
        prop_assert_eq!(value["before_ms"].as_i64(), Some(before_ms));
    }

    #[test]
    fn a_history_block_decodes_to_the_fields_it_was_written_from(
        count_hint in any::<Option<u64>>(),
        opening in any::<bool>(),
        exhausted in any::<bool>(),
        remaining in any::<Option<u64>>(),
        scanned_to_ms in any::<Option<i64>>(),
    ) {
        let json = |value: Option<String>| value.unwrap_or_else(|| "null".to_owned());
        let start = format!(
            r#"{{"type":"history_start","count_hint":{},"opening":{opening}}}"#,
            json(count_hint.map(|hint| hint.to_string()))
        );
        let end = format!(
            r#"{{"type":"history_end","exhausted":{exhausted},"remaining":{},"scanned_to_ms":{}}}"#,
            json(remaining.map(|left| left.to_string())),
            json(scanned_to_ms.map(|ms| ms.to_string()))
        );
        prop_assert_eq!(parse_line(&start).unwrap(), BridgeMsg::HistoryStart { count_hint, opening });
        prop_assert_eq!(
            parse_line(&end).unwrap(),
            BridgeMsg::HistoryEnd { exhausted, remaining, scanned_to_ms }
        );
    }
}

proptest! {
    // Each case may carry several lines of up to four times the cap.
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_line_reader_frames_exactly_the_lines_that_fit(
        lines in prop::collection::vec(line_body(), 1..5),
        terminated in any::<bool>(),
    ) {
        let mut bytes = lines.join(&b'\n');
        if terminated {
            bytes.push(b'\n');
        }
        let read = read_all(bytes);
        for line in &read {
            match line {
                BoundedLine::Line(text) => prop_assert!(text.len() <= MAX_LINE_BYTES),
                BoundedLine::NotUtf8 { len } => prop_assert!(*len <= MAX_LINE_BYTES),
                BoundedLine::TooLong | BoundedLine::Eof => {}
            }
        }
        prop_assert_eq!(read, expected_lines(&lines, terminated));
    }
}
