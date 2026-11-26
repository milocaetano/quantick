//! Reconnecting live feed: exponential backoff + continuity tracking.
//!
//! Wraps the single-connection [`run_agg_trade_stream`](crate::stream) in a loop
//! that reconnects on drop or error, backing off exponentially (with jitter) so
//! a persistent outage doesn't hammer Binance. A single [`ContinuityTracker`]
//! lives across reconnects, so a gap that opens *during* a disconnect is
//! detected when the stream resumes.

use std::time::Duration;

use tokio::sync::{mpsc::Sender, watch};
use tracing::{info, warn};

use quantick_engine::Trade;
use quantick_sources::reconnect::Backoff;

use crate::continuity::ContinuityTracker;
use crate::stream::run_agg_trade_stream;

/// Run the live aggTrade feed forever, reconnecting with backoff.
///
/// Forwards trades to `tx`, tracking continuity across reconnects. A healthy
/// connection (one that forwarded at least one trade) resets the backoff, so
/// only sustained failure escalates the delay. A connection silent for
/// `idle_timeout` ends with an error and reconnects like any other. Returns
/// when the consumer drops the receiver.
pub async fn run_with_reconnect(
    url: &str,
    tx: &Sender<Trade>,
    connected: &watch::Sender<bool>,
    mut backoff: Backoff,
    idle_timeout: Duration,
) {
    let mut tracker = ContinuityTracker::new();
    loop {
        match run_agg_trade_stream(url, tx, &mut tracker, connected, idle_timeout).await {
            Ok(forwarded) => {
                if forwarded > 0 {
                    backoff.reset();
                }
                if tx.is_closed() {
                    info!(target: "quantick::feed", "consumer gone; stopping reconnect loop");
                    return;
                }
                info!(target: "quantick::feed", forwarded, "stream ended cleanly; reconnecting");
            }
            Err(error) => {
                warn!(target: "quantick::feed", %error, "stream error; reconnecting");
            }
        }
        if tx.is_closed() {
            return;
        }
        // The websocket ended; publish that fact before waiting or attempting
        // another connection. Trade silence is never used as a proxy.
        let _ = connected.send(false);
        let delay = backoff.next_delay();
        info!(
            target: "quantick::feed",
            attempt = backoff.attempt(),
            delay_ms = delay.as_millis() as u64,
            "backing off before reconnect"
        );
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::agg_trade_url;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::{Notify, watch};

    async fn wait_for_status(status: &mut watch::Receiver<bool>, expected: bool) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                status.changed().await.expect("connection status closed");
                if *status.borrow_and_update() == expected {
                    return;
                }
            }
        })
        .await
        .expect("timed out waiting for connection status");
    }

    /// A socket that falls silent after the handshake is dropped once the
    /// idle timeout passes and dialled again.
    #[tokio::test]
    async fn a_silent_socket_is_dropped_and_dialled_again() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let silent = tokio_tungstenite::accept_async(stream).await.unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let _second = tokio_tungstenite::accept_async(stream).await.unwrap();
            drop(silent);
        });

        let (trades_tx, _trades_rx) = tokio::sync::mpsc::channel(8);
        let (connected_tx, mut connected_rx) = watch::channel(false);
        let url = agg_trade_url(&format!("ws://{address}"), "BTCUSDT");
        let feed = tokio::spawn(async move {
            run_with_reconnect(
                &url,
                &trades_tx,
                &connected_tx,
                Backoff::new(Duration::from_millis(2), Duration::from_millis(2), 7),
                Duration::from_millis(100),
            )
            .await;
        });

        wait_for_status(&mut connected_rx, true).await;
        wait_for_status(&mut connected_rx, false).await;
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("the feed never dialled a second connection")
            .unwrap();
        feed.abort();
    }

    #[tokio::test]
    async fn connection_status_tracks_disconnect_and_reconnect() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let close_first = Arc::new(Notify::new());
        let allow_second_handshake = Arc::new(Notify::new());
        let close_second = Arc::new(Notify::new());
        let server = {
            let close_first = Arc::clone(&close_first);
            let allow_second_handshake = Arc::clone(&allow_second_handshake);
            let close_second = Arc::clone(&close_second);
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                close_first.notified().await;
                socket.close(None).await.unwrap();

                let (stream, _) = listener.accept().await.unwrap();
                allow_second_handshake.notified().await;
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                close_second.notified().await;
                socket.close(None).await.unwrap();
            })
        };

        let (trades_tx, trades_rx) = tokio::sync::mpsc::channel(8);
        let (connected_tx, mut connected_rx) = watch::channel(false);
        let url = agg_trade_url(&format!("ws://{address}"), "BTCUSDT");
        let feed = tokio::spawn(async move {
            run_with_reconnect(
                &url,
                &trades_tx,
                &connected_tx,
                Backoff::new(Duration::from_millis(2), Duration::from_millis(2), 7),
                Duration::from_secs(30),
            )
            .await;
        });

        wait_for_status(&mut connected_rx, true).await;
        close_first.notify_one();
        wait_for_status(&mut connected_rx, false).await;
        allow_second_handshake.notify_one();
        wait_for_status(&mut connected_rx, true).await;

        drop(trades_rx);
        close_second.notify_one();
        tokio::time::timeout(Duration::from_secs(2), feed)
            .await
            .expect("feed did not stop")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("server did not stop")
            .unwrap();
    }
}
