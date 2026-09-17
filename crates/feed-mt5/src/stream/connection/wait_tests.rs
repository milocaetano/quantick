//! Actual driver wait tested with deterministic ready/pending input, not TCP timing.
use super::*;
use std::{
    future::{pending, ready},
    time::Duration,
};

struct PartialThenPending {
    bytes: &'static [u8],
}
impl tokio::io::AsyncRead for PartialThenPending {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.bytes.is_empty() {
            return std::task::Poll::Pending;
        }
        buf.put_slice(self.bytes);
        self.bytes = &[];
        std::task::Poll::Ready(Ok(()))
    }
}
#[tokio::test]
async fn a_ready_line_beats_an_expired_timer_and_a_pending_line_does_not() {
    let deadline = tokio::time::Instant::now() - Duration::from_secs(1);
    let mut reader = BoundedLineReader::new(&b"ready\n"[..]);
    let pager = pending();
    tokio::pin!(pager);
    assert!(
        matches!(tokio::time::timeout_at(deadline,select_input(&mut reader,pager.as_mut())).await,
        Ok(Wake::Line(Ok(BoundedLine::Line(line)))) if line=="ready")
    );
    let mut reader = BoundedLineReader::new(PartialThenPending {
        bytes: b"unfinished",
    });
    assert!(
        tokio::time::timeout_at(deadline, select_input(&mut reader, pager.as_mut()))
            .await
            .is_err()
    );
    // The reader really consumed the fragment; it did not mistake readiness for a complete line.
    assert!(
        tokio::time::timeout(Duration::ZERO, reader.next_line())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn a_ready_pager_beats_ready_eof_and_the_expired_timer() {
    let deadline = tokio::time::Instant::now() - Duration::from_secs(1);
    let mut reader = BoundedLineReader::new(&b""[..]);
    let pager = ready((7, 100));
    tokio::pin!(pager);
    assert!(matches!(
        tokio::time::timeout_at(deadline, select_input(&mut reader, pager.as_mut())).await,
        Ok(Wake::Request {
            count: 7,
            before_utc_ms: 100
        })
    ));
    assert!(matches!(
        reader.next_line().await.unwrap(),
        BoundedLine::Eof
    ));
}
#[tokio::test]
async fn every_complete_ignored_line_returns_a_new_wait_without_sampling_time() {
    let mut machine = SessionMachine::new("TEST", crate::map::SideMode::TickRule, 0, 10, 30);
    let hello = r#"{"type":"hello","schema":1,"bridge":"fixture","bridge_version":"1","symbol":"TEST","broker_symbol":"TEST","digits":0,"server_utc_offset_s":0}"#;
    let mut effect = machine
        .input(decode(Ok(BoundedLine::Line(hello.into()))))
        .unwrap();
    loop {
        effect = match effect {
            Effect::Wait(Phase::Connected) => break,
            Effect::Diagnostic => {
                let _ = machine.take_diagnostic().unwrap();
                machine.resume(Ack::Diagnostic).unwrap()
            }
            Effect::Publish => {
                let _ = machine.take_publication().unwrap();
                machine.resume(Ack::Published(true)).unwrap()
            }
            _ => panic!("admission effects"),
        };
    }
    for line in ["", "  ", "not-json", hello] {
        let mut effect = machine
            .input(decode(Ok(BoundedLine::Line(line.into()))))
            .unwrap();
        while matches!(effect, Effect::Diagnostic) {
            let _ = machine.take_diagnostic().unwrap();
            effect = machine.resume(Ack::Diagnostic).unwrap();
        }
        assert!(matches!(effect, Effect::Wait(Phase::Connected)));
    }
}

async fn sockets() -> (TcpStream, TcpStream) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (server, _) = listener.accept().await.unwrap();
    (peer, server)
}
async fn received(rx: &mut mpsc::Receiver<Mt5Event>) -> Mt5Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("event deadline")
        .expect("event channel")
}

#[tokio::test]
async fn queued_page_during_live_backpressure_settles_once_at_eof_without_replay() {
    use tokio::io::AsyncBufReadExt as _;
    let (mut peer, socket) = sockets().await;
    let mut config = ServerConfig::new("TEST");
    config.read_timeout = Duration::from_millis(100);
    let pager = config.history_pager.clone();
    let next_config = config.clone();
    let (tx, mut rx) = mpsc::channel(1);
    let queue = tx.clone();
    let session = tokio::spawn(async move {
        let mut generation = 0;
        let end = serve_connection(socket, &config, &tx, &mut generation).await;
        (end, generation)
    });
    let hello = concat!(
        r#"{"type":"hello","schema":1,"bridge":"fixture","bridge_version":"1","symbol":"TEST","broker_symbol":"TEST","digits":0,"server_utc_offset_s":0,"history_paging":true}"#,
        "\n"
    );
    peer.write_all(hello.as_bytes()).await.unwrap();
    assert!(matches!(
        received(&mut rx).await,
        Mt5Event::Status(super::super::events::Mt5Status::Connected { .. })
    ));
    for seq in 1..=3 {
        peer.write_all(format!(r#"{{"type":"tick","seq":{seq},"time_ms":10000,"bid":"0","ask":"0","last":"{}","volume":1,"flags":56}}"#,100+seq).as_bytes()).await.unwrap();
        peer.write_all(b"\n").await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while queue.capacity() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first live event fills the queue");
    assert!(pager.request(7, 123));
    peer.shutdown().await.unwrap(); // read half stays available for the queued request
    // More than read_timeout elapses while publication is blocked. It must not count as silence.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(matches!(received(&mut rx).await,Mt5Event::Live(trade) if trade.agg_id==2));
    assert!(matches!(received(&mut rx).await,Mt5Event::Live(trade) if trade.agg_id==3));
    let mut line = String::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        tokio::io::BufReader::new(&mut peer).read_line(&mut line),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        line,
        "{\"type\":\"load_older\",\"count\":7,\"before_ms\":123}\n"
    );
    assert!(
        matches!(received(&mut rx).await,Mt5Event::HistoryPage{trades,exhausted:false,scanned_to_utc_ms:None} if trades.is_empty())
    );
    let (end, generation) = tokio::time::timeout(Duration::from_secs(5), session)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(end,ConnEnd::BridgeGone(reason) if reason=="eof"));
    assert_eq!(generation, 0);
    assert!(!pager.is_in_flight());
    assert!(rx.try_recv().is_err());

    let (mut peer, socket) = sockets().await;
    let (tx, mut rx) = mpsc::channel(8);
    let next = tokio::spawn(async move {
        let mut generation = 0;
        serve_connection(socket, &next_config, &tx, &mut generation).await
    });
    peer.write_all(format!("{hello}{{\"type\":\"bye\",\"reason\":\"next\"}}\n").as_bytes())
        .await
        .unwrap();
    assert!(matches!(
        received(&mut rx).await,
        Mt5Event::Status(super::super::events::Mt5Status::Connected { .. })
    ));
    assert!(matches!(next.await.unwrap(),ConnEnd::BridgeGone(reason) if reason=="bye: next"));
    assert!(
        rx.recv().await.is_none(),
        "no page request or reply leaks into the next session"
    );
}
