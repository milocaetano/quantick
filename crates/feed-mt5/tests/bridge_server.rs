//! Integration tests for the bridge server: a fake bridge over real TCP.
//!
//! These prove the whole in-process path — accept → hello → backfill → live →
//! goodbye — without MetaTrader, using the exact wire format the EA emits.

use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use quantick_engine::Side;
use quantick_feed_mt5::{
    BookCaptureSwitch, HistoryPager, Mt5Event, Mt5Status, ServerConfig, SideMode, TapeKind,
    run_bridge_server,
};
use quantick_orderbook::{DepthEvent, DepthStatus};

/// A config bound to an ephemeral port with tight test timeouts.
fn test_config(symbol: &str) -> ServerConfig {
    ServerConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        symbol: symbol.to_string(),
        side_mode: SideMode::TickRule,
        hello_timeout: Duration::from_millis(500),
        read_timeout: Duration::from_millis(1000),
        book_capture: BookCaptureSwitch::new(),
        history_pager: HistoryPager::new(),
    }
}

/// Receive the next event or panic after 2 s (keeps failures readable).
async fn next_event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for an event")
        .expect("server closed the event channel")
}

/// Start a server and return (its bound addr, the event receiver).
async fn start_server(symbol: &str) -> (String, mpsc::Receiver<Mt5Event>) {
    start_server_from(test_config(symbol)).await
}

/// Same, keeping the depth switch so a test can turn capture on and off.
async fn start_server_with_capture(
    symbol: &str,
) -> (String, mpsc::Receiver<Mt5Event>, BookCaptureSwitch) {
    let config = test_config(symbol);
    let capture = config.book_capture.clone();
    let (addr, rx) = start_server_from(config).await;
    (addr, rx, capture)
}

/// Start a server from an explicit config — for tests whose timing needs
/// differ from the tight defaults.
async fn start_server_from(config: ServerConfig) -> (String, mpsc::Receiver<Mt5Event>) {
    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move {
        let _ = run_bridge_server(config, tx).await;
    });
    let Mt5Event::Status(Mt5Status::Waiting { addr }) = next_event(&mut rx).await else {
        panic!("expected the initial waiting status");
    };
    (addr, rx)
}

fn hello(symbol: &str) -> String {
    format!(
        "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\
         \"symbol\":\"{symbol}\",\"broker_symbol\":\"WINQ26\",\"digits\":0,\
         \"server_utc_offset_s\":-10800}}\n"
    )
}

/// A hello from a bridge that streams Depth of Market.
fn hello_with_book(symbol: &str) -> String {
    format!(
        "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",         \"symbol\":\"{symbol}\",\"broker_symbol\":\"WINQ26\",\"digits\":0,         \"server_utc_offset_s\":-10800,\"book_levels\":5,\"tick_size\":\"5\"}}
"
    )
}

fn book(seq: u64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> String {
    fn side(levels: &[(&str, &str)]) -> String {
        levels
            .iter()
            .map(|(price, quantity)| format!("[\"{price}\",\"{quantity}\"]"))
            .collect::<Vec<_>>()
            .join(",")
    }
    format!(
        "{{\"type\":\"book\",\"seq\":{seq},\"time_ms\":{},\"bids\":[{}],\"asks\":[{}]}}
",
        1_784_824_300_000_i64 + seq as i64,
        side(bids),
        side(asks)
    )
}

fn tick(seq: u64, last: &str, volume: u64) -> String {
    format!(
        "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{},\"bid\":\"0\",\"ask\":\"0\",\
         \"last\":\"{last}\",\"volume\":{volume},\"flags\":1080}}\n",
        1_784_824_300_000_i64 + seq as i64
    )
}

/// A hello from a bridge that reads its socket and answers `load_older`.
fn hello_that_pages(symbol: &str) -> String {
    format!(
        "{{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\
         \"symbol\":\"{symbol}\",\"broker_symbol\":\"WINQ26\",\"digits\":0,\
         \"server_utc_offset_s\":-10800,\"history_paging\":true}}\n"
    )
}

/// One tick at an explicit server-time millisecond, for the paged blocks whose
/// whole point is landing before what the chart holds.
fn tick_at(seq: u64, time_ms: i64, last: &str, volume: u64) -> String {
    format!(
        "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time_ms},\"bid\":\"0\",\"ask\":\"0\",\
         \"last\":\"{last}\",\"volume\":{volume},\"flags\":1080}}\n"
    )
}

/// Read one NDJSON line the feed wrote back to the bridge.
async fn read_line(sock: &mut TcpStream) -> String {
    let mut line = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        let read = tokio::time::timeout(Duration::from_secs(2), sock.read(&mut byte))
            .await
            .expect("timed out waiting for a request from the feed")
            .expect("socket error reading the request");
        assert_eq!(read, 1, "the feed closed the socket mid-request");
        if byte[0] == b'\n' {
            return String::from_utf8(line).expect("the feed writes UTF-8");
        }
        line.push(byte[0]);
    }
}

#[tokio::test]
async fn a_page_of_older_ticks_comes_back_for_the_asking() {
    // The whole round trip: the consumer asks, the request crosses the socket
    // in the terminal's own clock, and the block that comes back is delivered
    // as one page rather than as live prints.
    let config = test_config("WIN$N");
    let pager = config.history_pager.clone();
    let (addr, mut rx) = start_server_from(config).await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello_that_pages("WIN$N").as_bytes())
        .await
        .unwrap();

    let Mt5Event::Status(Mt5Status::Connected { history_paging, .. }) = next_event(&mut rx).await
    else {
        panic!("expected a connected status");
    };
    assert!(history_paging, "the hello declared it");

    // The consumer names its cursor in UTC. A B3 server stamps ticks in its own
    // wall time encoded as epoch (UTC−3), so the same instant is three hours
    // *behind* on the wire — and the paged ticks below are stamped in that
    // clock too, which is the whole reason the conversion happens here and not
    // in the consumer.
    assert!(pager.request(2_000, 1_784_835_100_000));
    let request = read_line(&mut sock).await;
    assert_eq!(
        request, "{\"type\":\"load_older\",\"count\":2000,\"before_ms\":1784824300000}",
        "the cursor crosses in server time, converted once, by the session"
    );

    let mut script = String::new();
    script.push_str("{\"type\":\"history_start\",\"count_hint\":2}\n");
    // Two ticks well before the cursor. The first has no tick-rule context and
    // is dropped by the mapper, exactly as in any other block.
    script.push_str(&tick_at(1, 1_784_824_290_000, "177795", 3));
    script.push_str(&tick_at(2, 1_784_824_291_000, "177800", 1));
    script.push_str(&tick_at(3, 1_784_824_292_000, "177790", 2));
    script.push_str("{\"type\":\"history_end\",\"exhausted\":false}\n");
    sock.write_all(script.as_bytes()).await.unwrap();

    let Mt5Event::HistoryPage {
        trades, exhausted, ..
    } = next_event(&mut rx).await
    else {
        panic!("expected the page, not a live print");
    };
    assert_eq!(trades.len(), 2, "the first tick has no side context");
    assert_eq!(trades[0].timestamp_ms, 1_784_824_291_000 + 10_800_000);
    assert_eq!(trades[0].side, Side::Buy, "uptick");
    assert_eq!(trades[1].side, Side::Sell, "downtick");
    assert!(!exhausted, "the bridge did not claim the end of the tape");
    assert!(
        !pager.is_in_flight(),
        "the answer settles the pager, so the next click is heard"
    );
}

#[tokio::test]
async fn a_page_is_classified_against_itself_and_the_live_tape_resumes_unchanged() {
    // The tick rule reads a side out of the price *before* this one. A page is
    // a run of prints from hours ago arriving after the live tape, so both
    // seams need care: its first print has no predecessor in hand, and the live
    // print after the block must be compared against the live price, not
    // against whatever the page happened to end on.
    let config = test_config("WIN$N");
    let pager = config.history_pager.clone();
    let (addr, mut rx) = start_server_from(config).await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = hello_that_pages("WIN$N");
    // Live tape climbing: 100 → 101. The chart's newest price is 101.
    script.push_str(&tick_at(1, 1_784_824_300_000, "100", 1));
    script.push_str(&tick_at(2, 1_784_824_301_000, "101", 1));
    sock.write_all(script.as_bytes()).await.unwrap();

    let _connected = next_event(&mut rx).await;
    let Mt5Event::Live(_first) = next_event(&mut rx).await else {
        panic!("expected the first mapped live print");
    };

    assert!(pager.request(10, 1_784_835_100_000));
    let _request = read_line(&mut sock).await;

    // The page sits far below the live tape. Were its first print compared
    // against the live 101 it would read as a heavy sell; it has no
    // predecessor at all, and is dropped and counted instead.
    let mut script = String::from("{\"type\":\"history_start\"}\n");
    script.push_str(&tick_at(3, 1_784_824_200_000, "90", 1));
    script.push_str(&tick_at(4, 1_784_824_201_000, "91", 1));
    script.push_str("{\"type\":\"history_end\",\"exhausted\":false}\n");
    sock.write_all(script.as_bytes()).await.unwrap();

    let Mt5Event::HistoryPage { trades, .. } = next_event(&mut rx).await else {
        panic!("expected the page");
    };
    assert_eq!(trades.len(), 1, "the page's first print has no predecessor");
    assert_eq!(
        trades[0].side,
        Side::Buy,
        "90 → 91 inside the page is an uptick, whatever the live tape is doing"
    );

    // And the live tape carries on from 101, not from the page's 91. A print
    // at 100 is a downtick; had the context leaked it would read as a buy.
    sock.write_all(tick_at(5, 1_784_824_302_000, "100", 1).as_bytes())
        .await
        .unwrap();
    let Mt5Event::Live(resumed) = next_event(&mut rx).await else {
        panic!("expected the live print after the page");
    };
    assert_eq!(
        resumed.side,
        Side::Sell,
        "the live tape resumed from 101, so 100 is a downtick"
    );
}

#[tokio::test]
async fn an_empty_page_still_answers_and_can_report_the_end_of_the_tape() {
    // The terminal has nothing older. The block is empty and the markers still
    // frame it — an unanswered request is a spinner that never stops — and
    // `exhausted` distinguishes "nothing left" from "nothing this time".
    let config = test_config("WIN$N");
    let pager = config.history_pager.clone();
    let (addr, mut rx) = start_server_from(config).await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello_that_pages("WIN$N").as_bytes())
        .await
        .unwrap();
    let _connected = next_event(&mut rx).await;

    assert!(pager.request(500, 1_784_835_100_000));
    let _request = read_line(&mut sock).await;
    sock.write_all(
        b"{\"type\":\"history_start\",\"count_hint\":0}\n{\"type\":\"history_end\",\"exhausted\":true}\n",
    )
    .await
    .unwrap();

    let Mt5Event::HistoryPage {
        trades, exhausted, ..
    } = next_event(&mut rx).await
    else {
        panic!("an empty page is still a page");
    };
    assert!(trades.is_empty());
    assert!(exhausted, "the terminal reported nothing older exists");
}

#[tokio::test]
async fn a_bridge_that_cannot_page_is_never_written_to() {
    // The compatibility case. A bridge whose hello predates the back-channel
    // must not receive a byte: it never reads, so the request would sit in its
    // receive buffer and eventually block the thread sending ticks. The
    // consumer is still answered, because a request that goes unanswered is a
    // spinner that outlives the click.
    let config = test_config("WIN$N");
    let pager = config.history_pager.clone();
    let (addr, mut rx) = start_server_from(config).await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("WIN$N").as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected { history_paging, .. }) = next_event(&mut rx).await
    else {
        panic!("expected a connected status");
    };
    assert!(!history_paging, "an absent field is a no, never a maybe");

    assert!(pager.request(2_000, 1_784_835_100_000));
    let Mt5Event::HistoryPage {
        trades, exhausted, ..
    } = next_event(&mut rx).await
    else {
        panic!("the request must be answered even though nothing was asked");
    };
    assert!(trades.is_empty());
    assert!(
        !exhausted,
        "refusing to ask says nothing about whether history exists"
    );

    // And nothing crossed the socket. The bridge proves it by streaming on:
    // a session that had been written to would still work, but a session that
    // *was not* is what this asserts, so the read below must find its own
    // request-shaped silence.
    let mut script = String::new();
    script.push_str(&tick(1, "177795", 3));
    script.push_str(&tick(2, "177800", 1));
    sock.write_all(script.as_bytes()).await.unwrap();
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the live print");
    };
    assert_eq!(trade.side, Side::Buy);

    let mut byte = [0_u8; 1];
    match tokio::time::timeout(Duration::from_millis(200), sock.read(&mut byte)).await {
        // Nothing arrived within the window: the feed stayed silent, which is
        // the whole claim.
        Err(_elapsed) => {}
        // A clean close would also read as "no bytes", so it is named rather
        // than folded into the silence: the session ending is a different
        // failure from the feed writing, and they must not look alike.
        Ok(Ok(0)) => panic!("the feed closed the session instead of leaving it alone"),
        Ok(Ok(_)) => panic!("the feed wrote to a bridge that never said it reads"),
        Ok(Err(e)) => panic!("socket error while proving the feed stayed silent: {e}"),
    }
}

#[tokio::test]
async fn a_session_that_dies_holding_a_request_still_answers_it() {
    // The spinner-forever case. The bridge takes the request and disappears
    // without a block; the consumer must still get its one reply.
    let config = test_config("WIN$N");
    let pager = config.history_pager.clone();
    let (addr, mut rx) = start_server_from(config).await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello_that_pages("WIN$N").as_bytes())
        .await
        .unwrap();
    let _connected = next_event(&mut rx).await;

    assert!(pager.request(2_000, 1_784_835_100_000));
    let _request = read_line(&mut sock).await;
    drop(sock); // the terminal closed, mid-answer

    let Mt5Event::HistoryPage {
        trades, exhausted, ..
    } = next_event(&mut rx).await
    else {
        panic!("a session that dies owing an answer still owes it");
    };
    assert!(trades.is_empty());
    assert!(!exhausted);
    assert!(
        !pager.is_in_flight(),
        "the next session must find the pager clear, not stuck mid-fetch"
    );
}

#[tokio::test]
async fn one_request_at_a_time_and_a_page_nobody_asked_for_is_dropped() {
    let config = test_config("WIN$N");
    let pager = config.history_pager.clone();
    let (addr, mut rx) = start_server_from(config).await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello_that_pages("WIN$N").as_bytes())
        .await
        .unwrap();
    let _connected = next_event(&mut rx).await;

    assert!(pager.request(2_000, 1_784_835_100_000));
    let _request = read_line(&mut sock).await;
    // A trader leaning on the button. The second click is dropped rather than
    // queued: pages answered minutes apart would land in front of bars the
    // earlier ones already drew.
    assert!(
        !pager.request(2_000, 1_784_835_100_000),
        "a second request while one is in flight is refused"
    );

    sock.write_all(
        b"{\"type\":\"history_start\"}\n{\"type\":\"history_end\",\"exhausted\":false}\n",
    )
    .await
    .unwrap();
    let Mt5Event::HistoryPage { .. } = next_event(&mut rx).await else {
        panic!("expected the answer to the first request");
    };

    // Now a block nobody asked for. Its ticks are history; charting them at the
    // front of the live tape is the one outcome worse than dropping them.
    let mut script = String::new();
    script.push_str("{\"type\":\"history_start\"}\n");
    script.push_str(&tick_at(9, 1_784_824_200_000, "177700", 5));
    script.push_str(&tick_at(10, 1_784_824_201_000, "177710", 5));
    script.push_str("{\"type\":\"history_end\",\"exhausted\":false}\n");
    // Followed by a live print, so the test has something to wait for that
    // proves the unsolicited block produced nothing of its own.
    script.push_str(&tick(1, "177795", 3));
    script.push_str(&tick(2, "177800", 1));
    sock.write_all(script.as_bytes()).await.unwrap();

    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("the unsolicited page must produce no event at all");
    };
    assert_eq!(trade.side, Side::Buy);
}

#[tokio::test]
async fn full_session_backfill_then_live() {
    let (addr, mut rx) = start_server("WIN$N").await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str("{\"type\":\"backfill_start\",\"count_hint\":3}\n");
    script.push_str(&tick(1, "177795", 3)); // no context → dropped
    script.push_str(&tick(2, "177800", 1)); // uptick → buy
    script.push_str(&tick(3, "177790", 2)); // downtick → sell
    script.push_str("{\"type\":\"backfill_end\"}\n");
    script.push_str(&tick(4, "177795", 1)); // live uptick → buy
    script.push_str("{\"type\":\"bye\",\"reason\":\"test_done\"}\n");
    sock.write_all(script.as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected {
        symbol,
        broker_symbol,
        tape,
        ..
    }) = next_event(&mut rx).await
    else {
        panic!("expected connected");
    };
    assert_eq!(symbol, "WIN$N");
    assert_eq!(broker_symbol, "WINQ26");
    assert_eq!(
        tape,
        TapeKind::Trades,
        "a hello without the field means the venue prints trades"
    );

    let Mt5Event::Backfilled(batch) = next_event(&mut rx).await else {
        panic!("expected the backfill block");
    };
    assert_eq!(batch.len(), 2, "first tick honestly dropped (no context)");
    assert_eq!(batch[0].side, Side::Buy);
    assert_eq!(batch[1].side, Side::Sell);
    assert_eq!(batch[0].agg_id, 2, "agg_id is the bridge seq");
    // Server-time 16:31 BRT surfaces as UTC (+3 h).
    assert_eq!(batch[0].timestamp_ms, 1_784_824_300_002 + 10_800_000);

    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected a live trade");
    };
    assert_eq!(trade.side, Side::Buy);
    assert_eq!(trade.agg_id, 4);

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected the session to end");
    };
    assert_eq!(reason, "bye: test_done");

    // And the server is accepting again.
    let Mt5Event::Status(Mt5Status::Waiting { .. }) = next_event(&mut rx).await else {
        panic!("expected the server back in waiting");
    };
}

#[tokio::test]
async fn symbol_mismatch_is_refused() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("EURUSD").as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected refusal");
    };
    assert!(reason.contains("symbol mismatch"), "{reason}");
}

#[tokio::test]
async fn schema_mismatch_is_refused() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let old = hello("WIN$N").replace("\"schema\":1", "\"schema\":99");
    sock.write_all(old.as_bytes()).await.unwrap();

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected refusal");
    };
    assert!(reason.contains("schema mismatch"), "{reason}");
}

#[tokio::test]
async fn garbage_lines_are_skipped_not_fatal() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str(&tick(1, "100", 1));
    script.push_str("this is not json\n");
    script.push_str("{\"type\":\"unknown_message\"}\n");
    script.push_str(&tick(2, "101", 1)); // still classified: buy
    sock.write_all(script.as_bytes()).await.unwrap();

    // Skip the Connected status.
    let _ = next_event(&mut rx).await;
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the trade after the garbage");
    };
    assert_eq!(trade.agg_id, 2);
    assert_eq!(trade.side, Side::Buy);
}

#[tokio::test]
async fn multibyte_garbage_is_skipped_without_panicking() {
    // Regression: `snippet` used to slice at byte 120, panicking (and killing
    // the whole listener) when that byte fell inside a multibyte char.
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str(&tick(1, "100", 1));
    script.push_str(&format!("x{}\n", "é".repeat(200))); // byte 120 mid-char
    script.push_str(&tick(2, "101", 1)); // still classified: buy
    sock.write_all(script.as_bytes()).await.unwrap();

    let _ = next_event(&mut rx).await; // Connected
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the trade after the multibyte garbage");
    };
    assert_eq!(trade.agg_id, 2);
}

#[tokio::test]
async fn an_invalid_utf8_line_is_skipped_not_fatal() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = Vec::new();
    script.extend_from_slice(hello("WIN$N").as_bytes());
    script.extend_from_slice(tick(1, "100", 1).as_bytes());
    script.extend_from_slice(&[0xff, 0xfe, 0xfd, b'\n']); // not UTF-8
    script.extend_from_slice(tick(2, "101", 1).as_bytes());
    sock.write_all(&script).await.unwrap();

    let _ = next_event(&mut rx).await; // Connected
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the trade after the invalid utf-8 line");
    };
    assert_eq!(trade.agg_id, 2);
}

#[tokio::test]
async fn an_oversized_line_ends_the_session_but_not_the_server() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    let _ = next_event(&mut rx).await; // Connected

    // 65 KiB without a newline: crosses the 64 KiB cap, yet small enough to
    // fit in the OS socket buffers so this write never races the server
    // closing the connection.
    let flood = vec![b'a'; 65 * 1024];
    sock.write_all(&flood).await.unwrap();

    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected the oversized line to end the session");
    };
    assert_eq!(reason, "oversized line");
    let Mt5Event::Status(Mt5Status::Waiting { .. }) = next_event(&mut rx).await else {
        panic!("expected the server back in waiting");
    };
}

#[tokio::test]
async fn a_silent_bridge_is_dropped_and_the_server_waits_again() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    let _ = next_event(&mut rx).await; // Connected

    // Send nothing further: the 1 s read timeout must end the session.
    let Mt5Event::Status(Mt5Status::Lost { reason }) = next_event(&mut rx).await else {
        panic!("expected the silent bridge to be dropped");
    };
    assert_eq!(reason, "silent");
    let Mt5Event::Status(Mt5Status::Waiting { .. }) = next_event(&mut rx).await else {
        panic!("expected the server back in waiting");
    };
}

/// Drain events until the first depth one, ignoring trades and statuses.
async fn next_depth(rx: &mut mpsc::Receiver<Mt5Event>) -> DepthEvent {
    loop {
        match next_event(rx).await {
            Mt5Event::Depth(event) => return event,
            other => {
                if let Mt5Event::Status(Mt5Status::Lost { reason }) = other {
                    panic!("session ended before any depth event: {reason}");
                }
            }
        }
    }
}

#[tokio::test]
async fn depth_images_become_a_snapshot_then_deltas_when_capture_is_on() {
    let (addr, mut rx, capture) = start_server_with_capture("WIN$N").await;
    capture.enable(1_000_000);

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello_with_book("WIN$N"));
    script.push_str(&book(
        1,
        &[("177790", "12"), ("177795", "3")],
        &[("177800", "5")],
    ));
    // One bid shrank; nothing else moved.
    script.push_str(&book(
        2,
        &[("177790", "12"), ("177795", "1")],
        &[("177800", "5")],
    ));
    sock.write_all(script.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    // "Connecting" precedes the first image so a chart can say what it waits for.
    let DepthEvent::Status {
        generation,
        status: DepthStatus::Connecting,
        ..
    } = next_depth(&mut rx).await
    else {
        panic!("expected the connecting status first");
    };
    // Above the consumer's base: a reconnect can never reuse a generation.
    assert!(generation > 1_000_000, "generation was {generation}");

    let DepthEvent::Snapshot {
        symbol,
        observed_at_ms,
        price_step,
        snapshot,
        generation: snapshot_generation,
        ..
    } = next_depth(&mut rx).await
    else {
        panic!("expected the opening snapshot");
    };
    assert_eq!(symbol, "WIN$N");
    assert_eq!(snapshot_generation, generation);
    // Server time converted to UTC with the hello's −3 h offset.
    assert_eq!(observed_at_ms, 1_784_824_300_001 + 10_800_000);
    assert_eq!(price_step, Some(rust_decimal::Decimal::from(5)));
    assert_eq!(snapshot.bids().len(), 2);
    assert_eq!(snapshot.asks().len(), 1);
    // The terminal's DOM is truncated by definition, and the hello said how far
    // it goes — the chart must never present it as the whole exchange book.
    assert_eq!(
        snapshot.coverage(),
        quantick_orderbook::BookCoverage::Limited { levels_per_side: 5 }
    );

    assert!(matches!(
        next_depth(&mut rx).await,
        DepthEvent::Status {
            status: DepthStatus::Synchronized { .. },
            ..
        }
    ));

    let DepthEvent::Update { delta, .. } = next_depth(&mut rx).await else {
        panic!("expected a delta for the changed level");
    };
    assert_eq!(delta.bids().len(), 1, "only the level that moved travels");
    assert!(delta.asks().is_empty());
}

#[tokio::test]
async fn depth_stays_silent_while_capture_is_off() {
    let (addr, mut rx, _capture) = start_server_with_capture("WIN$N").await;

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello_with_book("WIN$N"));
    script.push_str(&book(1, &[("177795", "3")], &[("177800", "5")]));
    script.push_str(&tick(1, "177795", 3));
    script.push_str(&tick(2, "177800", 1)); // uptick → a buy reaches the UI
    script.push_str("{\"type\":\"bye\",\"reason\":\"test_done\"}\n");
    sock.write_all(script.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    // Trades still flow; not one depth event is published.
    let mut trades = 0;
    loop {
        match next_event(&mut rx).await {
            Mt5Event::Live(_) => trades += 1,
            Mt5Event::Depth(event) => panic!("capture is off but got {event:?}"),
            Mt5Event::Status(Mt5Status::Lost { .. }) => break,
            _ => {}
        }
    }
    assert_eq!(trades, 1);
}

#[tokio::test]
async fn a_bridge_without_depth_says_so_instead_of_staying_silent() {
    let (addr, mut rx, capture) = start_server_with_capture("WIN$N").await;
    capture.enable(2_000_000);

    let mut sock = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    // The plain hello declares no book_levels: an older bridge, or a symbol
    // with no DOM on this account.
    script.push_str(&hello("WIN$N"));
    script.push_str("{\"type\":\"heartbeat\",\"seq_last\":0,\"time_ms\":1,\"ticks_sent\":0}\n");
    sock.write_all(script.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    let DepthEvent::Status {
        generation,
        status:
            DepthStatus::Disconnected {
                error_class: "bridge_without_depth",
            },
        ..
    } = next_depth(&mut rx).await
    else {
        panic!("expected an honest 'this bridge has no depth' status");
    };
    // At the consumer's own generation floor, so it is not discarded as stale.
    assert_eq!(generation, 2_000_000);
}

#[tokio::test]
async fn two_ports_stream_two_symbols_at_once() {
    // How quantick charts more than one MetaTrader symbol: one listener per
    // symbol, each a fully independent stack. Two EAs, two ports, two charts —
    // and neither stream can reach the other's consumer.
    let (gold_addr, mut gold_rx) = start_server("XAUUSD").await;
    let (index_addr, mut index_rx) = start_server("US500").await;
    assert_ne!(gold_addr, index_addr, "each symbol owns its own port");

    let mut gold = TcpStream::connect(&gold_addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("XAUUSD"));
    script.push_str(&tick(1, "4000", 1)); // no context → dropped
    script.push_str(&tick(2, "4001", 2)); // uptick → buy
    gold.write_all(script.as_bytes()).await.unwrap();
    gold.flush().await.unwrap();

    let mut index = TcpStream::connect(&index_addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("US500"));
    script.push_str(&tick(1, "7000", 1)); // no context → dropped
    script.push_str(&tick(2, "6999", 5)); // downtick → sell
    index.write_all(script.as_bytes()).await.unwrap();
    index.flush().await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected { symbol, .. }) = next_event(&mut gold_rx).await
    else {
        panic!("expected the gold session to connect");
    };
    assert_eq!(symbol, "XAUUSD");
    let Mt5Event::Status(Mt5Status::Connected { symbol, .. }) = next_event(&mut index_rx).await
    else {
        panic!("expected the index session to connect");
    };
    assert_eq!(symbol, "US500");

    let Mt5Event::Live(trade) = next_event(&mut gold_rx).await else {
        panic!("expected a live trade on the gold port");
    };
    assert_eq!(trade.agg_id, 2);
    assert_eq!(trade.side, Side::Buy);
    assert_eq!(trade.quantity, rust_decimal::Decimal::from(2));

    let Mt5Event::Live(trade) = next_event(&mut index_rx).await else {
        panic!("expected a live trade on the index port");
    };
    assert_eq!(trade.agg_id, 2);
    assert_eq!(trade.side, Side::Sell);
    assert_eq!(trade.quantity, rust_decimal::Decimal::from(5));

    // Neither consumer saw anything that was not its own.
    assert!(
        gold_rx.try_recv().is_err(),
        "the gold feed got a stray event"
    );
    assert!(
        index_rx.try_recv().is_err(),
        "the index feed got a stray event"
    );
}

/// Read until the peer closes, or panic after `patience`. Returns how many
/// bytes arrived first (the protocol is one-way, so the answer should be 0).
async fn expect_closed(sock: &mut TcpStream, patience: Duration) -> usize {
    let mut received = 0;
    let mut buf = [0_u8; 256];
    tokio::time::timeout(patience, async {
        loop {
            match sock.read(&mut buf).await {
                Ok(0) => return,
                Ok(n) => received += n,
                // A reset counts as closed: the peer is done with us either way.
                Err(_) => return,
            }
        }
    })
    .await
    .expect("the connection was left hanging instead of being refused");
    received
}

#[tokio::test]
async fn a_second_bridge_on_a_busy_port_is_refused_promptly() {
    // Two EAs pointed at one port is the setup mistake this makes loud. The
    // intruder used to sit in the accept backlog until its own send timeout —
    // invisible here, and a hang over there.
    let (addr, mut rx) = start_server("WIN$N").await;

    let mut first = TcpStream::connect(&addr).await.unwrap();
    let mut script = String::new();
    script.push_str(&hello("WIN$N"));
    script.push_str(&tick(1, "177795", 3)); // no context → dropped
    script.push_str(&tick(2, "177800", 1)); // uptick → buy
    first.write_all(script.as_bytes()).await.unwrap();
    first.flush().await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected { .. }) = next_event(&mut rx).await else {
        panic!("expected the first session to connect");
    };
    let Mt5Event::Live(trade) = next_event(&mut rx).await else {
        panic!("expected the first session's trade");
    };
    assert_eq!(trade.agg_id, 2);

    let mut second = TcpStream::connect(&addr).await.unwrap();
    second.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    second.flush().await.unwrap();
    assert_eq!(
        expect_closed(&mut second, Duration::from_secs(2)).await,
        0,
        "the protocol is one-way; a refusal is a close, not a reply"
    );

    // And the established session never noticed: same connection, still live.
    // The refusal itself now reaches the chart as an event, emitted from the
    // refusal task — so it and the tick race across tasks. Demand exactly one
    // of each, in whichever order they land.
    first
        .write_all(tick(3, "177810", 1).as_bytes())
        .await
        .unwrap();
    first.flush().await.unwrap();
    let mut busy = None;
    let mut live = None;
    for _ in 0..2 {
        match next_event(&mut rx).await {
            Mt5Event::SessionBusy {
                peer_symbol,
                diagnosis,
                ..
            } => busy = Some((peer_symbol, diagnosis)),
            Mt5Event::Live(trade) => live = Some(trade),
            other => panic!("unexpected event around the refusal: {other:?}"),
        }
    }
    let (peer_symbol, diagnosis) = busy.expect("the refusal must reach the event channel");
    assert_eq!(peer_symbol.as_deref(), Some("WIN$N"));
    assert_eq!(diagnosis, "same_symbol");
    let trade = live.expect("the served session must stream straight through a refusal");
    assert_eq!(trade.agg_id, 3);
}

#[tokio::test]
async fn an_intruder_that_says_nothing_is_refused_too() {
    // The window for the intruder to name itself is a courtesy to the log, not
    // a condition for refusing it: something that dials the port and stays mute
    // must still be closed, and quickly.
    //
    // The served session stays silent for the whole 250 ms refusal window, so
    // this test needs a read timeout that is not racing it. Ten seconds gives
    // the real-clock wait ~40x of margin; the default 1 s left only 4x, close
    // enough for a loaded CI box to drop the session first and fail this on
    // the wrong thing. A paused clock is not the alternative — it would
    // auto-advance the session's read timeout along with the window.
    let (addr, mut rx) = start_server_from(ServerConfig {
        read_timeout: Duration::from_secs(10),
        ..test_config("WIN$N")
    })
    .await;

    let mut first = TcpStream::connect(&addr).await.unwrap();
    first.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    first.flush().await.unwrap();
    let Mt5Event::Status(Mt5Status::Connected { .. }) = next_event(&mut rx).await else {
        panic!("expected the first session to connect");
    };

    let mut second = TcpStream::connect(&addr).await.unwrap();
    assert_eq!(
        expect_closed(&mut second, Duration::from_secs(2)).await,
        0,
        "a silent intruder is closed when its window runs out"
    );

    first
        .write_all(tick(1, "177795", 3).as_bytes())
        .await
        .unwrap();
    first
        .write_all(tick(2, "177800", 1).as_bytes())
        .await
        .unwrap();
    first.flush().await.unwrap();
    // The silent intruder's refusal is an event too, racing the tick across
    // tasks — accept both orders, demand both facts.
    let mut busy = None;
    let mut live = None;
    for _ in 0..2 {
        match next_event(&mut rx).await {
            Mt5Event::SessionBusy {
                peer_symbol,
                diagnosis,
                ..
            } => busy = Some((peer_symbol, diagnosis)),
            Mt5Event::Live(trade) => live = Some(trade),
            other => panic!("unexpected event around the refusal: {other:?}"),
        }
    }
    let (peer_symbol, diagnosis) = busy.expect("the refusal must reach the event channel");
    assert_eq!(peer_symbol, None);
    assert_eq!(diagnosis, "unidentified");
    let trade = live.expect("the served session must survive a silent intruder");
    assert_eq!(trade.agg_id, 2);
}

/// A hello from a bridge that also sends a historical candle block.
fn hello_with_rates(symbol: &str) -> String {
    hello(symbol).replace("\"digits\":0", "\"rates\":true,\"digits\":0")
}

/// One `rate` batch line carrying `bars` as the bridge formats them.
fn rate_chunk(bars: &[(i64, &str, &str, &str, &str, &str)]) -> String {
    let rows: Vec<String> = bars
        .iter()
        .map(|(t, o, h, l, c, v)| format!("[{t},\"{o}\",\"{h}\",\"{l}\",\"{c}\",\"{v}\"]"))
        .collect();
    format!("{{\"type\":\"rate\",\"bars\":[{}]}}\n", rows.join(","))
}

/// Drain until the candle block arrives, or panic if the session ends first.
async fn next_rates(rx: &mut mpsc::Receiver<Mt5Event>) -> (i64, Vec<quantick_engine::Bar>, bool) {
    loop {
        match next_event(rx).await {
            Mt5Event::Rates {
                interval_ms,
                bars,
                partial,
            } => return (interval_ms, bars, partial),
            Mt5Event::Status(Mt5Status::Lost { reason }) => {
                panic!("session ended before the candle block: {reason}")
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_rates_block_arrives_as_one_ordered_series() {
    // The time pane's whole supply on MetaTrader: the bridge pushes candles
    // once, after the tick backfill, and the feed hands over one series.
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();

    let mut script = String::new();
    script.push_str(&hello_with_rates("WIN$N"));
    script.push_str("{\"type\":\"backfill_start\",\"count_hint\":0}\n");
    script.push_str("{\"type\":\"backfill_end\"}\n");
    script.push_str("{\"type\":\"rates_start\",\"interval_ms\":60000,\"count_hint\":4}\n");
    // Two chunks, deliberately out of order across the boundary, with the
    // first bucket repeated at a corrected close in the second chunk.
    script.push_str(&rate_chunk(&[
        (
            1_784_824_320_000,
            "177800",
            "177860",
            "177790",
            "177850",
            "20",
        ),
        (
            1_784_824_260_000,
            "177790",
            "177850",
            "177780",
            "177800",
            "10",
        ),
    ]));
    script.push_str(&rate_chunk(&[
        // A bucket the venue reported empty: dropped, never carried forward.
        (
            1_784_824_380_000,
            "177850",
            "177850",
            "177850",
            "177850",
            "0",
        ),
        (
            1_784_824_260_000,
            "177790",
            "177850",
            "177780",
            "177799",
            "10",
        ),
    ]));
    script.push_str("{\"type\":\"rates_end\"}\n");
    sock.write_all(script.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected { rates, .. }) = next_event(&mut rx).await else {
        panic!("expected the session to connect");
    };
    assert!(rates, "the hello declared a candle block");

    let (interval_ms, bars, partial) = next_rates(&mut rx).await;
    assert_eq!(interval_ms, 60_000);
    assert!(!partial, "the bridge declared nothing missing");
    assert_eq!(
        bars.len(),
        2,
        "the empty bucket is dropped, the repeat merged"
    );
    // Ascending by open_time whatever order the chunks arrived in, and in UTC
    // (the hello's server clock is three hours behind).
    assert_eq!(bars[0].open_time, 1_784_824_260_000 + 10_800_000);
    assert_eq!(bars[1].open_time, 1_784_824_320_000 + 10_800_000);
    assert_eq!(bars[0].close_time, bars[0].open_time + 59_999);
    // Last write wins: a repeated bucket is a correction, not a duplicate bar.
    assert_eq!(
        bars[0].close,
        rust_decimal::Decimal::from(177_799),
        "the corrected close replaced the first one"
    );
    assert_eq!(bars[0].volume(), rust_decimal::Decimal::from(10));
    assert_eq!(
        bars[0].delta(),
        rust_decimal::Decimal::ZERO,
        "MT5 publishes no aggressor split; delta reads as not measured"
    );
}

#[tokio::test]
async fn a_session_without_rates_says_so_instead_of_promising_candles() {
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();
    sock.write_all(hello("WIN$N").as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    let Mt5Event::Status(Mt5Status::Connected { rates, .. }) = next_event(&mut rx).await else {
        panic!("expected the session to connect");
    };
    assert!(
        !rates,
        "an Expert Advisor session, or any bridge predating candles"
    );
}

#[tokio::test]
async fn a_corrupt_candle_is_dropped_and_the_block_still_arrives() {
    // One bad row in ninety days is a gap, not a reason to lose the history.
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();

    let mut script = String::new();
    script.push_str(&hello_with_rates("WIN$N"));
    script.push_str("{\"type\":\"rates_start\",\"interval_ms\":60000}\n");
    script.push_str(&rate_chunk(&[
        (
            1_784_824_260_000,
            "177790",
            "177850",
            "177780",
            "177800",
            "10",
        ),
        // High below low: not a candle any market printed.
        (
            1_784_824_320_000,
            "177800",
            "177700",
            "177900",
            "177850",
            "20",
        ),
    ]));
    // A line that is not even a rate batch: skipped and counted, like any
    // other garbage, without ending the session.
    script.push_str("{\"type\":\"rate\",\"bars\":\"not-an-array\"}\n");
    script.push_str(&rate_chunk(&[(
        1_784_824_380_000,
        "177850",
        "177900",
        "177840",
        "177870",
        "30",
    )]));
    script.push_str("{\"type\":\"rates_end\"}\n");
    sock.write_all(script.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    let _ = next_event(&mut rx).await; // Connected
    let (_, bars, _) = next_rates(&mut rx).await;
    assert_eq!(bars.len(), 2, "only the incoherent candle was dropped");
    assert_eq!(bars[0].open_time, 1_784_824_260_000 + 10_800_000);
    assert_eq!(bars[1].open_time, 1_784_824_380_000 + 10_800_000);
}

#[tokio::test]
async fn a_block_that_never_ended_is_discarded_not_half_delivered() {
    // A candle series with a hole reads as a market that stopped trading, so
    // an interrupted block is dropped whole; the next session re-sends it.
    let (addr, mut rx) = start_server("WIN$N").await;
    let mut sock = TcpStream::connect(&addr).await.unwrap();

    let mut script = String::new();
    script.push_str(&hello_with_rates("WIN$N"));
    script.push_str("{\"type\":\"rates_start\",\"interval_ms\":60000}\n");
    script.push_str(&rate_chunk(&[(
        1_784_824_260_000,
        "177790",
        "177850",
        "177780",
        "177800",
        "10",
    )]));
    script.push_str("{\"type\":\"bye\",\"reason\":\"test_done\"}\n");
    sock.write_all(script.as_bytes()).await.unwrap();
    sock.flush().await.unwrap();

    let _ = next_event(&mut rx).await; // Connected
    loop {
        match next_event(&mut rx).await {
            Mt5Event::Rates { bars, .. } => {
                panic!(
                    "an unfinished block must not be delivered: {} bars",
                    bars.len()
                )
            }
            Mt5Event::Status(Mt5Status::Lost { reason }) => {
                assert_eq!(reason, "bye: test_done");
                break;
            }
            _ => {}
        }
    }
}
