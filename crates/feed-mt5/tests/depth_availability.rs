//! A quiet but connected bridge must stop book loading and recover in place.

use std::time::Duration;

use quantick_feed_mt5::{Mt5Event, Mt5Status, ServerConfig, SideMode, run_bridge_server};
use quantick_orderbook::{DepthEvent, DepthStatus};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

async fn next(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(14), rx.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn empty_depth_goes_offline_while_trades_history_and_recovery_keep_the_socket() {
    let mut config = ServerConfig::new("WIN");
    config.listen_addr = "127.0.0.1:0".to_owned();
    config.side_mode = SideMode::Flags;
    config.read_timeout = Duration::from_secs(5);
    config.book_capture.enable(100);
    let pager = config.history_pager.clone();
    let (tx, mut rx) = mpsc::channel(128);
    let server = tokio::spawn(run_bridge_server(config, tx));
    let Mt5Event::Status(Mt5Status::Waiting { addr }) = next(&mut rx).await else {
        panic!("expected ephemeral listener");
    };
    let socket = TcpStream::connect(addr).await.unwrap();
    let (reader, mut writer) = socket.into_split();
    writer.write_all(b"{\"type\":\"hello\",\"schema\":1,\"bridge\":\"test\",\"bridge_version\":\"0\",\"symbol\":\"WIN\",\"broker_symbol\":\"WIN\",\"digits\":0,\"server_utc_offset_s\":0,\"book_levels\":5,\"history_paging\":true}\n").await.unwrap();
    assert!(matches!(
        next(&mut rx).await,
        Mt5Event::Status(Mt5Status::Connected { .. })
    ));
    // Heartbeats and empty images prove socket liveness, not DOM availability.
    let writer_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        for seq in 1..=13 {
            interval.tick().await;
            writer.write_all(format!("{{\"type\":\"book\",\"seq\":{seq},\"time_ms\":1,\"bids\":[],\"asks\":[]}}\n{{\"type\":\"heartbeat\",\"seq_last\":0,\"time_ms\":1,\"ticks_sent\":0}}\n").as_bytes()).await.unwrap();
        }
        writer
    });
    assert!(matches!(
        next(&mut rx).await,
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Connecting,
            ..
        })
    ));
    let Mt5Event::Depth(DepthEvent::Status {
        generation,
        status: DepthStatus::Disconnected {
            error_class: "no_depth_signal",
        },
        ..
    }) = next(&mut rx).await
    else {
        panic!("empty DOM must stop loading despite heartbeats");
    };
    let mut writer = writer_task.await.unwrap();
    assert!(
        rx.try_recv().is_err(),
        "offline wait emits no repeated retries or loading statuses"
    );
    assert!(pager.request(10, 1000));
    let mut lines = BufReader::new(reader).lines();
    let request = tokio::time::timeout(Duration::from_secs(2), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(request.contains("load_older"));
    writer.write_all(b"{\"type\":\"history_start\"}\n{\"type\":\"tick\",\"seq\":1,\"time_ms\":900,\"bid\":\"100\",\"ask\":\"105\",\"last\":\"100\",\"volume\":3,\"flags\":1080}\n{\"type\":\"history_end\"}\n{\"type\":\"tick\",\"seq\":2,\"time_ms\":1100,\"bid\":\"100\",\"ask\":\"105\",\"last\":\"105\",\"volume\":1,\"flags\":1080}\n{\"type\":\"book\",\"seq\":14,\"time_ms\":1200,\"bids\":[[\"100\",\"3\"]],\"asks\":[[\"105\",\"4\"]]}\n").await.unwrap();
    assert!(
        matches!(next(&mut rx).await, Mt5Event::HistoryPage { trades, .. } if trades.len() == 1)
    );
    assert!(matches!(next(&mut rx).await, Mt5Event::Live(trade) if trade.agg_id == 2));
    assert!(
        matches!(next(&mut rx).await, Mt5Event::Depth(DepthEvent::Snapshot { generation: recovered, .. }) if recovered == generation)
    );
    assert!(matches!(
        next(&mut rx).await,
        Mt5Event::Depth(DepthEvent::Status {
            status: DepthStatus::Synchronized { .. },
            ..
        })
    ));
    server.abort();
}
