//! Public TCP silence and pager-order characterization.
//!
//! These tests do not assume a TCP write equals one reader fill. Controlled
//! private wait tests separately pin ready-input versus expired-timer priority.

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
    let (mut peer, mut rx, server) = start(HistoryPager::new(), Duration::from_millis(100)).await;
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
