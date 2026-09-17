//! Owned loopback producer for explicit native evidence, never a replayed event vector.

use futures_util::SinkExt as _;
use tokio::{
    net::TcpListener,
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::tungstenite::Message;

use crate::{ObservedFeedHandle, config::ProviderKind};

pub(crate) struct FixtureLifetime {
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for FixtureLifetime {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().expect("local fixture runtime failed");
        }
    }
}

/// Run literal local WebSocket sessions through the actual observed host.
/// The receiver owns the complete runtime lifetime. Dropping it cancels and
/// joins only that fixture's server and host. No global endpoint is enabled.
#[must_use]
pub fn spawn_local_fixture() -> ObservedFeedHandle {
    let (events, receiver) = mpsc::channel(4096);
    let (book, book_events) = mpsc::channel(8);
    let (notice, notices) = mpsc::channel(32);
    let (commands, command_receiver) = mpsc::channel(16);
    let (stop, stopped) = oneshot::channel();
    let thread = std::thread::Builder::new().name("quantick-local-integrity-fixture".into()).spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("ws://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                for frames in [&[super::ACK][..], &[super::ACK, super::EMPTY, super::MIXED], &[super::ACK, super::OVERLAP, super::STALE]] {
                    let mut socket = super::accept_subscription(&listener).await;
                    for frame in frames { socket.send(Message::Text((*frame).into())).await.unwrap(); }
                    socket.close(None).await.unwrap();
                }
                // A live fourth connection prevents fabricated extra outages.
                // The three earlier closes were processed before this subscribe.
                let mut socket = super::accept_subscription(&listener).await;
                socket.send(Message::Text(super::ACK.into())).await.unwrap();
                std::future::pending::<()>().await;
            });
            let mut host = tokio::spawn(crate::hyperliquid::feed_task_with(
                "BTC".into(), crate::hyperliquid::ObservedOutput(events), book, notice, command_receiver,
                crate::hyperliquid::HyperliquidSource {
                    local_fixture: true, url,
                    backoff: quantick_feed_hyperliquid::Backoff::new(std::time::Duration::from_millis(1), std::time::Duration::from_millis(1), 7),
                },
            ));
            tokio::select! { _ = stopped => { host.abort(); let _ = host.await; }, result = &mut host => { result.unwrap(); } }
            server.abort();
            assert!(server.await.unwrap_err().is_cancelled());
        });
    }).expect("spawn local integrity fixture");
    let mut capabilities = ProviderKind::Hyperliquid.capabilities();
    capabilities.book_capture = false;
    capabilities.ohlcv_history = false;
    ObservedFeedHandle {
        events: crate::ObservedReceiver::from(receiver).with_fixture(FixtureLifetime {
            stop: Some(stop),
            thread: Some(thread),
        }),
        book_events,
        notices,
        capabilities: crate::fixed_capabilities(capabilities),
        latency: crate::unsplit_latency(),
        commands,
        replay: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn owned_fixture_runs_actual_trade_path_refuses_secondary_io_and_joins_on_drop() {
        let mut handle = spawn_local_fixture();
        assert!(handle.events.is_local_fixture());
        assert!(!handle.capabilities.borrow().book_capture);
        assert!(!handle.capabilities.borrow().ohlcv_history);
        handle
            .commands
            .send(crate::FeedCommand::FetchOhlcv {
                span_ms: 1000,
                slice_ms: None,
                before_ms: None,
            })
            .await
            .unwrap();
        handle
            .commands
            .send(crate::FeedCommand::SetBookCapture {
                enabled: true,
                initial_generation: 1,
            })
            .await
            .unwrap();
        handle
            .commands
            .send(crate::FeedCommand::RestartBookCapture {
                initial_generation: 2,
            })
            .await
            .unwrap();
        let mut integrity = crate::FeedIntegrity::default();
        let mut exclusions = crate::FeedExclusions::default();
        let (mut trades, mut refused) = (0, false);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while integrity.unknown_loss != 3 || !refused {
                match handle.events.recv().await.unwrap() {
                    crate::ObservedFeedEvent::Excluded(event) => exclusions.observe(event),
                    crate::ObservedFeedEvent::Feed(crate::FeedEvent::Continuity(event)) => {
                        integrity.observe(event)
                    }
                    crate::ObservedFeedEvent::Feed(
                        crate::FeedEvent::Backfilled(batch) | crate::FeedEvent::LiveBatch(batch),
                    ) => trades += batch.len(),
                    crate::ObservedFeedEvent::Feed(crate::FeedEvent::OhlcvHistory {
                        bars,
                        slice,
                        ..
                    }) => {
                        assert!(bars.is_empty());
                        assert_eq!(slice, crate::OhlcvSlice::Refused);
                        refused = true;
                    }
                    _ => panic!("unexpected fixture output"),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(
            (integrity.missing_messages, integrity.non_monotonic, trades),
            (0, 0, 1)
        );
        assert_eq!((exclusions.malformed_rows, exclusions.stale_rows), (1, 1));
        assert!(handle.book_events.try_recv().is_err());
        drop(handle); // Synchronously joins its one owned runtime and all tasks.
    }
}
