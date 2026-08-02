//! Persistent Hyperliquid depth generations.

use tokio::sync::mpsc::Sender;
use tracing::{info, warn};

use quantick_orderbook::{DepthEvent, DepthStatus};

use crate::reconnect::Backoff;

use super::mapper::BookMapper;
use super::stream::{DepthSessionError, run_depth_session};

/// Reconnect forever, advancing the consumer generation every time continuity
/// ends. Returns only when the receiver is dropped.
pub async fn run_depth_with_reconnect(
    url: &str,
    symbol: &str,
    events: &Sender<DepthEvent>,
    initial_generation: u64,
    mut backoff: Backoff,
) {
    let symbol = symbol.to_uppercase();
    let mut generation = initial_generation;
    let mut mapper = BookMapper::new(&symbol, generation);

    loop {
        let result = run_depth_session(url, &symbol, events, &mut mapper, generation).await;
        if mapper.is_synchronized() {
            backoff.reset();
        }
        let error = match result {
            Ok(()) => DepthSessionError::Transport("depth session ended unexpectedly".to_owned()),
            Err(DepthSessionError::ConsumerClosed) => {
                info!(target: "quantick::depth", symbol, generation, "depth consumer gone; stopping Hyperliquid feed");
                return;
            }
            Err(error) => error,
        };
        let error_class = error.error_class();
        warn!(
            target: "quantick::depth",
            schema_version = 1_u8,
            event_code = "hyperliquid_depth_disconnected",
            symbol,
            generation,
            error_class,
            %error,
            action = "reconnect",
            "Hyperliquid L2 generation ended"
        );
        if events
            .send(DepthEvent::Status {
                symbol: symbol.clone(),
                generation,
                status: DepthStatus::Disconnected { error_class },
            })
            .await
            .is_err()
        {
            return;
        }
        let delay = backoff.next_delay();
        info!(
            target: "quantick::depth",
            symbol,
            generation,
            attempt = backoff.attempt(),
            delay_ms = delay.as_millis() as u64,
            "backing off before Hyperliquid L2 reconnect"
        );
        tokio::select! {
            () = events.closed() => return,
            () = tokio::time::sleep(delay) => {}
        }
        generation = generation.saturating_add(1);
    }
}
