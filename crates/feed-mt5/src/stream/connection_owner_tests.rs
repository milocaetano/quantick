//! Baseline owner fixtures exercise the existing concrete socket and pager.

use super::*;
use tokio::net::TcpListener;

struct FailedRead;

impl tokio::io::AsyncRead for FailedRead {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        _: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "fixture read reset",
        )))
    }
}

#[tokio::test]
async fn the_admission_reader_keeps_the_original_socket_error() {
    let error = BoundedLineReader::new(FailedRead)
        .next_line()
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
    assert_eq!(error.to_string(), "fixture read reset");
}

#[tokio::test]
async fn extracted_admission_classifies_the_same_injected_read_error() {
    let mut reader = BoundedLineReader::new(FailedRead);
    let mut machine = SessionMachine::new("TEST", crate::map::SideMode::TickRule, 0, 10, 30);
    let mut effect = machine.input(decode(reader.next_line().await)).unwrap();
    while matches!(effect, Effect::Diagnostic) {
        let _ = machine.take_diagnostic().unwrap();
        effect = machine.resume(Ack::Diagnostic).unwrap();
    }
    assert!(matches!(effect, Effect::Finished));
    let ConnEnd::BridgeGone(reason) = machine.take_end() else {
        panic!("admission must refuse a failed read")
    };
    assert_eq!(reason, "socket error before hello: fixture read reset");
}

#[tokio::test]
async fn a_shut_down_request_writer_reports_write_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (socket, _) = listener.accept().await.unwrap();
    let (_reader, mut writer) = socket.into_split();
    writer.shutdown().await.unwrap();
    let mapper = crate::map::TickMapper::new(crate::map::SideMode::TickRule, 2);
    assert!(
        write_message(
            &mut writer,
            &FeedMsg::LoadOlder {
                count: 7,
                before_ms: mapper.to_server_ms(100)
            }
        )
        .await
        .is_err()
    );
    drop(peer);
}

#[tokio::test]
async fn queued_and_in_flight_debt_is_abandoned_once_without_replay() {
    let pager = super::super::HistoryPager::new();
    assert!(pager.request(7, 100));
    assert!(pager.abandon());
    assert!(!pager.abandon());
    assert!(pager.request(9, 200));
    assert_eq!(pager.take_request().await, (9, 200));
    assert!(pager.abandon());
    assert!(!pager.abandon());
    assert!(!pager.is_in_flight());
    assert!(pager.request(11, 300));
    assert_eq!(pager.take_request().await, (11, 300));
    assert!(pager.settle_owed());
    assert!(!pager.settle_owed());
}
