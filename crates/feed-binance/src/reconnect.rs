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

use crate::continuity::ContinuityTracker;
use crate::stream::run_agg_trade_stream;

/// Exponential backoff with equal-jitter, deterministic given a seed.
///
/// Delay for attempt `n` is `min(base * 2^n, max)`, then jittered into
/// `[capped/2, capped]`. The jitter comes from a seeded xorshift, so the delay
/// sequence is reproducible in tests (no rng dependency, and the feed's timing
/// need not be — and is not — engine-deterministic; the seed just makes tests
/// stable). Jitter avoids many clients reconnecting in lockstep.
#[derive(Debug, Clone)]
pub struct Backoff {
    base_ms: u64,
    max_ms: u64,
    attempt: u32,
    rng: u64,
}

impl Backoff {
    /// A backoff between `base` and `max`, seeded for reproducible jitter.
    #[must_use]
    pub fn new(base: Duration, max: Duration, seed: u64) -> Self {
        Self {
            base_ms: base.as_millis().max(1) as u64,
            max_ms: max.as_millis().max(1) as u64,
            attempt: 0,
            rng: seed | 1, // xorshift must not be seeded with 0
        }
    }

    /// Sensible defaults for a live market-data feed: 500 ms base, 30 s ceiling.
    #[must_use]
    pub fn for_feed(seed: u64) -> Self {
        Self::new(Duration::from_millis(500), Duration::from_secs(30), seed)
    }

    fn next_rand(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }

    /// The delay before the next reconnect attempt, advancing the counter.
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1u64.checked_shl(self.attempt.min(31)).unwrap_or(u64::MAX);
        let capped = self.base_ms.saturating_mul(factor).min(self.max_ms);
        let half = capped / 2;
        let jitter = (half as f64 * self.next_rand()) as u64;
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(half + jitter)
    }

    /// Reset the attempt counter (call after a healthy connection).
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// The number of delays handed out since the last reset.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// Run the live aggTrade feed forever, reconnecting with backoff.
///
/// Forwards trades to `tx`, tracking continuity across reconnects. A healthy
/// connection (one that forwarded at least one trade) resets the backoff, so
/// only sustained failure escalates the delay. Returns when the consumer drops
/// the receiver.
pub async fn run_with_reconnect(
    url: &str,
    tx: &Sender<Trade>,
    connected: &watch::Sender<bool>,
    mut backoff: Backoff,
) {
    let mut tracker = ContinuityTracker::new();
    loop {
        match run_agg_trade_stream(url, tx, &mut tracker, connected).await {
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

    #[test]
    fn backoff_grows_and_is_capped() {
        let base = Duration::from_millis(100);
        let max = Duration::from_millis(3200);
        let mut b = Backoff::new(base, max, 12345);

        // Attempt 0: capped = 100, delay in [50, 100].
        let d0 = b.next_delay().as_millis() as u64;
        assert!((50..=100).contains(&d0), "d0 = {d0}");

        // Collect a run of delays; each stays within its capped bound and the
        // ceiling is respected once 2^n * base exceeds max.
        let mut delays = vec![d0];
        for _ in 0..9 {
            delays.push(b.next_delay().as_millis() as u64);
        }
        for d in &delays {
            assert!(*d <= max.as_millis() as u64, "delay {d} exceeds max");
        }
        // By later attempts the cap (3200) is hit, so delay is in [1600, 3200].
        let late = *delays.last().unwrap();
        assert!((1600..=3200).contains(&late), "late = {late}");
    }

    #[test]
    fn reset_returns_to_the_base_delay() {
        let mut b = Backoff::new(Duration::from_millis(100), Duration::from_secs(30), 7);
        for _ in 0..5 {
            let _ = b.next_delay();
        }
        assert_eq!(b.attempt(), 5);
        b.reset();
        assert_eq!(b.attempt(), 0);
        let d = b.next_delay().as_millis() as u64;
        assert!((50..=100).contains(&d), "post-reset delay = {d}");
    }

    #[test]
    fn jitter_is_deterministic_for_a_given_seed() {
        let seq = |seed| {
            let mut b = Backoff::new(Duration::from_millis(100), Duration::from_secs(30), seed);
            (0..5).map(|_| b.next_delay()).collect::<Vec<_>>()
        };
        assert_eq!(seq(42), seq(42), "same seed => same delays");
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
