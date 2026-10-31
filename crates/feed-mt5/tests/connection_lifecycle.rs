//! One bridge connection's lifecycle, pinned literally through the public TCP port.
//!
//! Written against `serve_connection` before it was split into phases, so a
//! failure here means the split changed an event, an order or an end reason.

use quantick_engine::Side;
use quantick_feed_mt5::{
    BookCaptureSwitch, HistoryPager, Mt5Event, Mt5Status, ServerConfig, run_bridge_server,
};
use quantick_orderbook::{DepthEvent, DepthStatus};
use std::time::Duration;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

struct Bridge {
    socket: TcpStream,
    rx: mpsc::Receiver<Mt5Event>,
    pager: HistoryPager,
    capture: BookCaptureSwitch,
    server: tokio::task::JoinHandle<Result<(), quantick_feed_mt5::Mt5Error>>,
    addr: String,
}

async fn event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(15), rx.recv())
        .await
        .expect("event deadline")
        .expect("event channel")
}

impl Bridge {
    async fn new() -> Self {
        let mut config = ServerConfig::new("TEST");
        config.listen_addr = "127.0.0.1:0".into();
        config.hello_timeout = Duration::from_millis(300);
        config.read_timeout = Duration::from_secs(5);
        let pager = config.history_pager.clone();
        let capture = config.book_capture.clone();
        let (tx, mut rx) = mpsc::channel(1024);
        let server = tokio::spawn(run_bridge_server(config, tx));
        let Mt5Event::Status(Mt5Status::Waiting { addr }) = event(&mut rx).await else {
            panic!("waiting")
        };
        let socket = TcpStream::connect(&addr).await.unwrap();
        Self {
            socket,
            rx,
            pager,
            capture,
            server,
            addr,
        }
    }
    async fn send(&mut self, bytes: &str) {
        self.socket.write_all(bytes.as_bytes()).await.unwrap();
    }
    async fn connect(&mut self) {
        self.send(HELLO).await;
        assert!(matches!(
            event(&mut self.rx).await,
            Mt5Event::Status(Mt5Status::Connected {
                history_paging: true,
                rates: true,
                deal_counter: true,
                ..
            })
        ));
    }
    async fn request(&mut self) {
        assert!(self.pager.request(7, 100));
        let mut bytes = Vec::new();
        loop {
            let b = self.socket.read_u8().await.unwrap();
            bytes.push(b);
            if b == b'\n' {
                break;
            }
        }
        assert_eq!(
            bytes,
            b"{\"type\":\"load_older\",\"count\":7,\"before_ms\":2100}\n"
        );
    }
    async fn lost(&mut self, expected: &str) {
        let Mt5Event::Status(Mt5Status::Lost { reason }) = event(&mut self.rx).await else {
            panic!("lost")
        };
        assert_eq!(reason, expected);
        assert!(matches!(
            event(&mut self.rx).await,
            Mt5Event::Status(Mt5Status::Waiting { .. })
        ));
    }
}

const HELLO: &str = "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"fixture\",\"bridge_version\":\"0\",\"symbol\":\"TEST\",\"broker_symbol\":\"TEST.raw\",\"digits\":0,\"server_utc_offset_s\":2,\"history_paging\":true,\"rates\":true,\"deal_counter\":true,\"book_levels\":5,\"tick_size\":\"1\"}\n";
fn tick(seq: u64, time: i64, price: u64) -> String {
    format!(
        "{{\"type\":\"tick\",\"seq\":{seq},\"time_ms\":{time},\"bid\":\"0\",\"ask\":\"0\",\"last\":\"{price}\",\"volume\":1,\"flags\":56}}\n"
    )
}
fn book(seq: u64) -> String {
    format!(
        "{{\"type\":\"book\",\"seq\":{seq},\"time_ms\":10000,\"bids\":[[\"100\",\"2\"]],\"asks\":[[\"102\",\"3\"]]}}\n"
    )
}
fn deal_tick(seq: u64, time: i64, price: u64, deals: u64, sent: i64) -> String {
    tick(seq, time, price).replace(
        "\"flags\":56",
        &format!("\"flags\":56,\"deals\":{deals},\"sent_ms\":{sent}"),
    )
}

#[tokio::test]
async fn hello_refusals_have_exact_reasons_and_never_connect() {
    let cases = [
        (
            b"{\"type\":\"bye\",\"reason\":\"x\"}\n".to_vec(),
            "no hello",
        ),
        (b"not-json\n".to_vec(), "undecodable hello"),
        (vec![255, 254, b'\n'], "undecodable hello"),
        (vec![b'x'; 65_537], "oversized hello"),
        (
            HELLO.replace("\"schema\":1", "\"schema\":2").into_bytes(),
            "schema mismatch (bridge 2)",
        ),
        (
            HELLO
                .replace("\"symbol\":\"TEST\"", "\"symbol\":\"OTHER\"")
                .into_bytes(),
            "symbol mismatch (OTHER)",
        ),
    ];
    for (bytes, reason) in cases {
        let mut bridge = Bridge::new().await;
        bridge.socket.write_all(&bytes).await.unwrap();
        bridge.lost(reason).await;
        bridge.server.abort();
    }
    let mut eof = Bridge::new().await;
    eof.socket.shutdown().await.unwrap();
    eof.lost("closed before hello").await;
    eof.server.abort();
    let mut silent = Bridge::new().await;
    silent.lost("hello timeout").await;
    silent.server.abort();
}

#[tokio::test]
async fn a_partial_tick_survives_a_pager_wake_and_startless_end_settles_once() {
    let mut b = Bridge::new().await;
    b.connect().await;
    b.send(&tick(1, 10000, 100)).await;
    let line = tick(2, 10001, 101);
    b.socket.write_all(&line.as_bytes()[..32]).await.unwrap();
    b.request().await;
    b.socket.write_all(&line.as_bytes()[32..]).await.unwrap();
    let Mt5Event::Live(t) = event(&mut b.rx).await else {
        panic!("live")
    };
    assert_eq!((t.agg_id, t.timestamp_ms, t.side), (2, 8001, Side::Buy));
    b.send("{\"type\":\"history_end\",\"exhausted\":true}\n")
        .await;
    assert!(
        matches!(event(&mut b.rx).await, Mt5Event::HistoryPage { trades, exhausted:false, scanned_to_utc_ms:None } if trades.is_empty())
    );
    assert!(!b.pager.is_in_flight());
    b.send("{\"type\":\"history_end\"}\n{\"type\":\"bye\",\"reason\":\"fixture\"}\n")
        .await;
    b.lost("bye: fixture").await;
    b.server.abort();
}

#[tokio::test]
async fn repeated_history_restores_live_context_and_current_offset_cursor() {
    let mut b = Bridge::new().await;
    b.connect().await;
    b.send(&(tick(1, 10000, 100) + &tick(2, 10001, 101))).await;
    assert!(matches!(event(&mut b.rx).await, Mt5Event::Live(_)));
    b.request().await;
    b.send(&(String::from("{\"type\":\"backfill_start\"}\n{\"type\":\"history_start\"}\n")+&tick(3,9000,90)+&tick(4,9001,91)+"{\"type\":\"history_start\"}\n"+&tick(5,8000,80)+&tick(6,8001,81)+"{\"type\":\"heartbeat\",\"time_ms\":12000,\"seq_last\":6,\"ticks_sent\":6,\"server_utc_offset_s\":3}\n{\"type\":\"history_end\",\"scanned_to_ms\":7000}\n{\"type\":\"backfill_end\"}\n")).await;
    assert!(matches!(event(&mut b.rx).await, Mt5Event::Latency(_)));
    let Mt5Event::HistoryPage {
        trades,
        exhausted,
        scanned_to_utc_ms,
    } = event(&mut b.rx).await
    else {
        panic!("page")
    };
    assert_eq!(trades.len(), 1);
    assert_eq!(
        (trades[0].agg_id, trades[0].timestamp_ms, trades[0].side),
        (6, 6001, Side::Buy)
    );
    assert!(!exhausted);
    assert_eq!(scanned_to_utc_ms, Some(4000));
    assert!(matches!(event(&mut b.rx).await,Mt5Event::Backfilled(t) if t.is_empty()));
    b.send(&tick(7, 10002, 100)).await;
    let Mt5Event::Live(t) = event(&mut b.rx).await else {
        panic!("resumed")
    };
    assert_eq!((t.agg_id, t.timestamp_ms, t.side), (7, 7002, Side::Sell));
    b.server.abort();
}

#[tokio::test]
async fn opening_disconnect_keeps_the_click_owed_and_discards_partial_blocks() {
    let mut b = Bridge::new().await;
    b.connect().await;
    b.request().await;
    b.send(
        &(String::from("{\"type\":\"history_start\",\"opening\":true}\n")
            + &tick(1, 1000, 100)
            + &tick(2, 1001, 101)
            + "{\"type\":\"history_end\",\"remaining\":2}\n"),
    )
    .await;
    assert!(
        matches!(event(&mut b.rx).await,Mt5Event::OpeningPage { trades, remaining:Some(2) } if trades.len()==1)
    );
    assert!(b.pager.is_in_flight());
    b.send("{\"type\":\"backfill_start\"}\n{\"type\":\"rates_start\",\"interval_ms\":60000}\n{\"type\":\"history_start\",\"opening\":true}\n{\"type\":\"bye\",\"reason\":\"partial\"}\n").await;
    assert!(
        matches!(event(&mut b.rx).await,Mt5Event::HistoryPage { trades, exhausted:false, scanned_to_utc_ms:None } if trades.is_empty())
    );
    b.lost("bye: partial").await;
    assert!(!b.pager.is_in_flight());
    let socket = TcpStream::connect(&b.addr).await.unwrap();
    b.socket = socket;
    b.connect().await;
    b.send("{\"type\":\"bye\",\"reason\":\"next\"}\n").await;
    b.lost("bye: next").await;
    b.server.abort();
}

#[tokio::test]
async fn sequence_deals_prints_offsets_and_terminal_flush_keep_their_order() {
    let mut b = Bridge::new().await;
    b.connect().await;
    b.send(&(deal_tick(1, 10000, 100, 10, 11000) + &deal_tick(4, 10001, 101, 20, 11001)))
        .await;
    let Mt5Event::SequenceAnomaly { from_ms, to_ms, .. } = event(&mut b.rx).await else {
        panic!("anomaly first")
    };
    assert_eq!((from_ms, to_ms), (8000, 8001));
    let Mt5Event::DealCounter(d) = event(&mut b.rx).await else {
        panic!("deal second")
    };
    assert_eq!((d.time_ms, d.session_deals), (8000, 10));
    assert!(matches!(event(&mut b.rx).await,Mt5Event::Live(t) if t.agg_id==4));
    b.send("{\"type\":\"heartbeat\",\"time_ms\":12000,\"seq_last\":4,\"ticks_sent\":2,\"server_utc_offset_s\":3}\n")
        .await;
    let Mt5Event::DealCounter(d) = event(&mut b.rx).await else {
        panic!("old-clock heartbeat flush")
    };
    assert_eq!((d.time_ms, d.session_deals), (8001, 20));
    assert!(matches!(event(&mut b.rx).await, Mt5Event::Latency(_)));
    let quote = deal_tick(6, 10002, 0, 30, 11002).replace("\"volume\":1", "\"volume\":0");
    b.send(&quote).await;
    assert!(matches!(
        event(&mut b.rx).await,
        Mt5Event::SequenceAnomaly {
            from_ms: 8001,
            to_ms: 7002,
            ..
        }
    ));
    b.socket.shutdown().await.unwrap();
    let Mt5Event::DealCounter(d) = event(&mut b.rx).await else {
        panic!("EOF deal flush")
    };
    assert_eq!((d.time_ms, d.session_deals), (7002, 30));
    b.lost("eof").await;
    b.server.abort();
}

#[tokio::test]
async fn rates_repeated_startless_and_partial_keep_the_original_hello_offset() {
    let mut b = Bridge::new().await;
    b.connect().await;
    b.send(concat!(
        "{\"type\":\"rates_end\"}\n",
        "{\"type\":\"rate\",\"bars\":[[60000,\"10\",\"12\",\"9\",\"11\",\"3\"]]}\n",
        "{\"type\":\"rates_start\",\"interval_ms\":60000}\n",
        "{\"type\":\"rate\",\"bars\":[[60000,\"10\",\"12\",\"9\",\"11\",\"3\"]]}\n",
        "{\"type\":\"heartbeat\",\"time_ms\":12000,\"seq_last\":0,\"ticks_sent\":0,\"server_utc_offset_s\":3}\n",
        "{\"type\":\"rates_start\",\"interval_ms\":60000}\n",
        "{\"type\":\"rate\",\"bars\":[[120000,\"20\",\"22\",\"19\",\"21\",\"4\"]]}\n",
        "{\"type\":\"rates_end\",\"partial\":true}\n",
        "{\"type\":\"rates_start\",\"interval_ms\":60000}\n",
        "{\"type\":\"rate\",\"bars\":[[180000,\"30\",\"32\",\"29\",\"31\",\"5\"]]}\n",
        "{\"type\":\"bye\",\"reason\":\"rates\"}\n"
    ))
    .await;
    let Mt5Event::Rates {
        interval_ms,
        bars,
        partial,
    } = event(&mut b.rx).await
    else {
        panic!("rates")
    };
    assert_eq!(interval_ms, 60000);
    assert!(partial);
    assert_eq!(bars.len(), 1);
    assert_eq!((bars[0].open_time, bars[0].close_time), (118000, 177999));
    assert_eq!(bars[0].close, rust_decimal::Decimal::from(21));
    b.lost("bye: rates").await;
    b.server.abort();
}

async fn depth_status(b: &mut Bridge, generation: u64, expected: DepthStatus) {
    let Mt5Event::Depth(DepthEvent::Status {
        generation: actual,
        status,
        ..
    }) = event(&mut b.rx).await
    else {
        panic!("depth status")
    };
    assert_eq!(actual, generation);
    assert_eq!(status, expected);
}
async fn snapshot(b: &mut Bridge, generation: u64) {
    depth_status(b, generation, DepthStatus::Connecting).await;
    assert!(
        matches!(event(&mut b.rx).await,Mt5Event::Depth(DepthEvent::Snapshot { generation:g,.. }) if g==generation)
    );
    assert!(
        matches!(event(&mut b.rx).await,Mt5Event::Depth(DepthEvent::Status { generation:g,status:DepthStatus::Synchronized { .. },.. }) if g==generation)
    );
}

#[tokio::test]
async fn depth_capture_loss_reconnect_and_terminal_deal_flush_are_ordered() {
    let mut b = Bridge::new().await;
    b.capture.enable(100);
    b.connect().await;
    b.send(&book(1)).await;
    snapshot(&mut b, 101).await;
    b.capture.disable();
    b.send(&book(2)).await;
    depth_status(&mut b, 101, DepthStatus::Stopped).await;
    b.capture.enable(200);
    b.send(&book(3)).await;
    snapshot(&mut b, 202).await;
    b.send(&book(5)).await;
    assert!(matches!(
        event(&mut b.rx).await,
        Mt5Event::Depth(DepthEvent::Status {
            generation: 202,
            status: DepthStatus::Resyncing { .. },
            ..
        })
    ));
    snapshot(&mut b, 203).await;
    b.send(&(deal_tick(1, 10000, 100, 7, 11000) + "{\"type\":\"bye\",\"reason\":\"depth\"}\n"))
        .await;
    assert!(
        matches!(event(&mut b.rx).await,Mt5Event::DealCounter(d) if d.time_ms==8000 && d.session_deals==7)
    );
    depth_status(
        &mut b,
        203,
        DepthStatus::Disconnected {
            error_class: "bridge_lost",
        },
    )
    .await;
    b.lost("bye: depth").await;
    b.socket = TcpStream::connect(&b.addr).await.unwrap();
    b.connect().await;
    b.send(&book(1)).await;
    snapshot(&mut b, 204).await;
    b.server.abort();
}

#[tokio::test]
async fn a_closed_consumer_completes_the_server_during_dispatch() {
    let mut b = Bridge::new().await;
    b.connect().await;
    drop(b.rx);
    b.socket
        .write_all((tick(1, 10000, 100) + &tick(2, 10001, 101)).as_bytes())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(5), b.server)
            .await
            .expect("server must finish, not time out")
            .unwrap()
            .is_ok()
    );
}

#[tokio::test]
async fn a_history_page_is_bounded_at_250000_mapped_trades() {
    let mut b = Bridge::new().await;
    b.connect().await;
    b.request().await;
    b.send("{\"type\":\"history_start\"}\n").await;
    // Batched writes keep the fixture bounded while exercising the actual append arm.
    for batch in 0..251 {
        let mut lines = String::new();
        for offset in 0..1000 {
            let seq = batch * 1000 + offset + 1;
            lines.push_str(&tick(seq, seq as i64, 100 + seq % 2));
        }
        b.send(&lines).await;
    }
    b.send("{\"type\":\"history_end\"}\n").await;
    let Mt5Event::HistoryPage {
        trades,
        exhausted,
        scanned_to_utc_ms,
    } = event(&mut b.rx).await
    else {
        panic!("bounded page")
    };
    assert_eq!(trades.len(), 250000);
    assert_eq!(
        (trades[0].agg_id, trades.last().unwrap().agg_id),
        (2, 250001)
    );
    assert!(!exhausted);
    assert_eq!(scanned_to_utc_ms, None);
    b.server.abort();
}

/// Silence and pager ordering.
mod deadline {
    // Public TCP silence and pager-order characterization.
    //
    // These tests do not assume a TCP write equals one reader fill. Controlled
    // private wait tests separately pin ready-input versus expired-timer priority.

    use std::time::Duration;

    use quantick_feed_mt5::{HistoryPager, Mt5Event, Mt5Status, ServerConfig, run_bridge_server};
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
    use tokio::net::TcpStream;
    use tokio::sync::mpsc;

    const HELLO: &str = concat!(
        "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"deadline-fixture\",",
        "\"bridge_version\":\"1\",\"symbol\":\"TEST\",\"broker_symbol\":\"TEST\",",
        "\"digits\":0,\"server_utc_offset_s\":0,\"history_paging\":true}\n",
    );
    const BYE: &str = "{\"type\":\"bye\",\"reason\":\"ready_line\"}\n";

    async fn next_event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
        tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("server event deadline")
            .expect("server event channel")
    }

    async fn start(
        pager: HistoryPager,
        read_timeout: Duration,
    ) -> (
        TcpStream,
        mpsc::Receiver<Mt5Event>,
        tokio::task::JoinHandle<Result<(), quantick_feed_mt5::Mt5Error>>,
    ) {
        let mut config = ServerConfig::new("TEST");
        config.listen_addr = "127.0.0.1:0".into();
        config.read_timeout = read_timeout;
        config.history_pager = pager;
        let (tx, mut rx) = mpsc::channel(8);
        let server = tokio::spawn(run_bridge_server(config, tx));
        let Mt5Event::Status(Mt5Status::Waiting { addr }) = next_event(&mut rx).await else {
            panic!("initial waiting event")
        };
        (TcpStream::connect(addr).await.unwrap(), rx, server)
    }

    async fn connected(rx: &mut mpsc::Receiver<Mt5Event>) {
        assert!(matches!(
            next_event(rx).await,
            Mt5Event::Status(Mt5Status::Connected { .. })
        ));
    }

    async fn lost(rx: &mut mpsc::Receiver<Mt5Event>, reason: &str) {
        assert_eq!(
            next_event(rx).await,
            Mt5Event::Status(Mt5Status::Lost {
                reason: reason.into()
            })
        );
        assert!(matches!(
            next_event(rx).await,
            Mt5Event::Status(Mt5Status::Waiting { .. })
        ));
    }

    #[tokio::test]
    async fn coalesced_session_lines_preserve_the_bye_reason() {
        let (mut peer, mut rx, server) = start(HistoryPager::new(), Duration::from_secs(5)).await;
        // Coalesced sender input is not assumed to be a coalesced receiver read.
        peer.write_all(format!("{HELLO}{BYE}").as_bytes())
            .await
            .unwrap();
        connected(&mut rx).await;
        lost(&mut rx, "bye: ready_line").await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn an_unfinished_line_ends_with_the_silent_timeout_reason() {
        let (mut peer, mut rx, server) =
            start(HistoryPager::new(), Duration::from_millis(100)).await;
        peer.write_all(format!("{HELLO}{{\"type\":\"tick\"").as_bytes())
            .await
            .unwrap();
        connected(&mut rx).await;
        lost(&mut rx, "silent").await;
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn a_prequeued_pager_is_written_before_bye_and_settles_once() {
        let pager = HistoryPager::new();
        assert!(pager.request(7, 123));
        let (mut peer, mut rx, server) = start(pager.clone(), Duration::from_secs(5)).await;
        peer.write_all(format!("{HELLO}{BYE}").as_bytes())
            .await
            .unwrap();
        connected(&mut rx).await;
        let mut request = String::new();
        tokio::time::timeout(
            Duration::from_secs(10),
            BufReader::new(peer).read_line(&mut request),
        )
        .await
        .expect("pager wire deadline")
        .expect("pager wire read");
        assert_eq!(
            request,
            "{\"type\":\"load_older\",\"count\":7,\"before_ms\":123}\n"
        );
        assert_eq!(
            next_event(&mut rx).await,
            Mt5Event::HistoryPage {
                trades: Vec::new(),
                exhausted: false,
                scanned_to_utc_ms: None,
            }
        );
        lost(&mut rx, "bye: ready_line").await;
        assert!(!pager.is_in_flight());
        assert!(
            pager.request(9, 456),
            "the one request was settled and its gate cleared"
        );
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

/// The boundary before a session exists.
mod admission {
    // Public TCP characterization of the boundary before connected cleanup exists.

    use std::time::Duration;

    use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, run_bridge_server};
    use tokio::io::AsyncWriteExt as _;
    use tokio::net::TcpStream;
    use tokio::sync::mpsc;

    const HELLO: &[u8] = concat!(
        "{\"type\":\"hello\",\"schema\":1,\"bridge\":\"admission-fixture\",",
        "\"bridge_version\":\"1\",\"symbol\":\"TEST\",\"broker_symbol\":\"TEST\",",
        "\"digits\":0,\"server_utc_offset_s\":0,\"history_paging\":true}\n",
    )
    .as_bytes();

    async fn next_event(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
        tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("server event deadline")
            .expect("server event channel")
    }

    #[tokio::test]
    async fn rejected_connected_publication_never_takes_or_abandons_prequeued_debt() {
        let mut config = ServerConfig::new("TEST");
        config.listen_addr = "127.0.0.1:0".into();
        let pager = config.history_pager.clone();
        assert!(pager.request(7, 123));
        let (tx, mut rx) = mpsc::channel(1);
        let server = tokio::spawn(run_bridge_server(config, tx));
        let Mt5Event::Status(Mt5Status::Waiting { addr }) = next_event(&mut rx).await else {
            panic!("initial waiting event")
        };
        let mut peer = TcpStream::connect(addr).await.unwrap();
        // Close before hello: its Connected publication deterministically fails.
        // Admission itself still reads the socket, without polling the pager.
        rx.close();
        peer.write_all(HELLO).await.unwrap();
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("consumer closure must finish the server")
            .expect("server task")
            .expect("normal consumer closure");
        assert!(rx.recv().await.is_none());
        assert!(!pager.is_in_flight(), "request was never taken");
        assert!(
            !pager.request(9, 456),
            "original queued request was not abandoned"
        );
    }

    #[tokio::test]
    async fn eof_before_hello_reports_refusal_without_connected_page_cleanup() {
        let mut config = ServerConfig::new("TEST");
        config.listen_addr = "127.0.0.1:0".into();
        let pager = config.history_pager.clone();
        assert!(pager.request(7, 123));
        let (tx, mut rx) = mpsc::channel(1);
        let server = tokio::spawn(run_bridge_server(config, tx));
        let Mt5Event::Status(Mt5Status::Waiting { addr }) = next_event(&mut rx).await else {
            panic!("initial waiting event")
        };
        let mut peer = TcpStream::connect(addr).await.unwrap();
        peer.shutdown().await.unwrap();
        assert_eq!(
            next_event(&mut rx).await,
            Mt5Event::Status(Mt5Status::Lost {
                reason: "closed before hello".into()
            }),
        );
        assert!(matches!(
            next_event(&mut rx).await,
            Mt5Event::Status(Mt5Status::Waiting { .. })
        ));
        assert!(
            !pager.is_in_flight(),
            "unadmitted session must not take the request"
        );
        assert!(!pager.request(9, 456), "prequeued request remains queued");
        // No HistoryPage reply was interleaved before Lost or the next Waiting.
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

/// Paths nothing above reaches: what refreshes the silence deadline, a reset
/// socket on each side of the hello, and messages arriving out of place.
mod gaps {
    use std::time::Duration;

    use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, run_bridge_server};
    use tokio::io::AsyncWriteExt as _;
    use tokio::net::TcpStream;
    use tokio::sync::mpsc;

    use super::{HELLO, event, tick};

    /// Long enough that a contended host cannot fake a silence between sends.
    const READ_TIMEOUT: Duration = Duration::from_secs(2);
    const SEND_GAP: Duration = Duration::from_millis(250);
    /// Sends spanning well past [`READ_TIMEOUT`].
    const SENDS: usize = 12;

    async fn start(read_timeout: Duration) -> (TcpStream, mpsc::Receiver<Mt5Event>) {
        let mut config = ServerConfig::new("TEST");
        config.listen_addr = "127.0.0.1:0".into();
        config.read_timeout = read_timeout;
        let (tx, mut rx) = mpsc::channel(1024);
        tokio::spawn(run_bridge_server(config, tx));
        let Mt5Event::Status(Mt5Status::Waiting { addr }) = event(&mut rx).await else {
            panic!("initial waiting event")
        };
        (TcpStream::connect(addr).await.unwrap(), rx)
    }

    async fn lost_reason(rx: &mut mpsc::Receiver<Mt5Event>) -> String {
        let Mt5Event::Status(Mt5Status::Lost { reason }) = event(rx).await else {
            panic!("expected the session to end")
        };
        reason
    }

    async fn connected(sock: &mut TcpStream, rx: &mut mpsc::Receiver<Mt5Event>) {
        sock.write_all(HELLO.as_bytes()).await.unwrap();
        assert!(matches!(
            event(rx).await,
            Mt5Event::Status(Mt5Status::Connected { .. })
        ));
    }

    #[tokio::test]
    async fn invalid_utf8_lines_do_not_count_as_a_living_bridge() {
        let (mut sock, mut rx) = start(READ_TIMEOUT).await;
        connected(&mut sock, &mut rx).await;
        let started = tokio::time::Instant::now();
        for _ in 0..SENDS {
            // A peer that already closed the session may refuse the write.
            if sock.write_all(&[0xff, 0xfe, b'\n']).await.is_err() {
                break;
            }
            tokio::time::sleep(SEND_GAP).await;
        }
        assert_eq!(lost_reason(&mut rx).await, "silent");
        assert!(started.elapsed() >= READ_TIMEOUT);
    }

    #[tokio::test]
    async fn undecodable_and_blank_lines_do_count_as_a_living_bridge() {
        for filler in ["not-json\n", "\n"] {
            let (mut sock, mut rx) = start(READ_TIMEOUT).await;
            connected(&mut sock, &mut rx).await;
            for _ in 0..SENDS {
                sock.write_all(filler.as_bytes()).await.unwrap();
                tokio::time::sleep(SEND_GAP).await;
            }
            sock.write_all(b"{\"type\":\"bye\",\"reason\":\"alive\"}\n")
                .await
                .unwrap();
            assert_eq!(lost_reason(&mut rx).await, "bye: alive");
        }
    }

    #[tokio::test]
    async fn a_reset_socket_is_a_socket_error_on_either_side_of_the_hello() {
        let (sock, mut rx) = start(READ_TIMEOUT).await;
        sock.set_zero_linger().unwrap();
        drop(sock);
        let reason = lost_reason(&mut rx).await;
        assert!(
            reason.starts_with("socket error before hello: "),
            "got {reason:?}"
        );

        let (mut sock, mut rx) = start(READ_TIMEOUT).await;
        connected(&mut sock, &mut rx).await;
        sock.set_zero_linger().unwrap();
        drop(sock);
        let reason = lost_reason(&mut rx).await;
        assert!(reason.starts_with("socket error: "), "got {reason:?}");
    }

    #[tokio::test]
    async fn a_second_hello_and_a_startless_backfill_end_leave_the_session_running() {
        let (mut sock, mut rx) = start(READ_TIMEOUT).await;
        connected(&mut sock, &mut rx).await;
        let lines = format!(
            "{HELLO}{{\"type\":\"backfill_end\"}}\n{}{}",
            tick(1, 10000, 100),
            tick(2, 10001, 101)
        );
        sock.write_all(lines.as_bytes()).await.unwrap();
        assert!(matches!(event(&mut rx).await, Mt5Event::Backfilled(t) if t.is_empty()));
        assert!(matches!(event(&mut rx).await, Mt5Event::Live(t) if t.agg_id == 2));
    }

    #[tokio::test]
    async fn a_tick_inside_a_backfill_waits_for_its_end_and_the_live_tape_follows() {
        let (mut sock, mut rx) = start(READ_TIMEOUT).await;
        connected(&mut sock, &mut rx).await;
        let lines = format!(
            "{{\"type\":\"backfill_start\",\"count_hint\":2}}\n{}{}{{\"type\":\"backfill_end\"}}\n{}",
            tick(1, 10000, 100),
            tick(2, 10001, 101),
            tick(3, 10002, 102)
        );
        sock.write_all(lines.as_bytes()).await.unwrap();
        let Mt5Event::Backfilled(batch) = event(&mut rx).await else {
            panic!("the backfill comes out whole, ahead of the live print")
        };
        assert_eq!(batch.iter().map(|t| t.agg_id).collect::<Vec<_>>(), [2]);
        assert!(matches!(event(&mut rx).await, Mt5Event::Live(t) if t.agg_id == 3));
    }

    #[tokio::test]
    async fn a_session_that_ends_inside_a_backfill_delivers_none_of_it() {
        let (mut sock, mut rx) = start(READ_TIMEOUT).await;
        connected(&mut sock, &mut rx).await;
        let lines = format!(
            "{{\"type\":\"backfill_start\"}}\n{}{}",
            tick(1, 10000, 100),
            tick(2, 10001, 101)
        );
        sock.write_all(lines.as_bytes()).await.unwrap();
        sock.shutdown().await.unwrap();
        assert_eq!(lost_reason(&mut rx).await, "eof");
    }
}
