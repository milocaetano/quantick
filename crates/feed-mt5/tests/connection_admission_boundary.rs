//! Public TCP characterization of the boundary before connected cleanup exists.

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
