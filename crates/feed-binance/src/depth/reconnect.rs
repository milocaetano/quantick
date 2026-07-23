//! Persistent depth feed with deterministic jittered reconnect backoff.

use std::time::Duration;

use tokio::sync::mpsc::Sender;
use tracing::{info, warn};

use crate::reconnect::Backoff;

use super::snapshot::DepthSnapshotSource;
use super::stream::{
    DepthEvent, DepthSessionConfig, DepthSessionError, DepthStatus, depth_stream_url,
    run_depth_session,
};

/// Run synchronized depth generations until the consumer is dropped.
///
/// `config.initial_generation` is used verbatim for the first connection and
/// incremented for every reconnect. A UI controller should seed it with its own
/// monotonic capture epoch when depth is disabled and later re-enabled.
pub async fn run_depth_with_reconnect<S: DepthSnapshotSource>(
    ws_base_url: &str,
    symbol: &str,
    source: &S,
    events: &Sender<DepthEvent>,
    config: DepthSessionConfig,
    mut backoff: Backoff,
) {
    let symbol = symbol.to_uppercase();
    let url = depth_stream_url(ws_base_url, &symbol);
    let mut generation = config.initial_generation;

    loop {
        let result = run_depth_session(&url, &symbol, source, events, config, generation).await;
        let error = match result {
            Ok(()) => {
                // The current session runner is intentionally open-ended, but
                // retain a defensive reconnect path if that changes.
                DepthSessionError::Transport("depth session ended unexpectedly".to_string())
            }
            Err(DepthSessionError::ConsumerClosed) => {
                info!(
                    target: "quantick::depth",
                    schema_version = 1_u8,
                    event_code = "depth_feed_stopped",
                    symbol = symbol.as_str(),
                    generation,
                    action = "stop",
                    "depth consumer closed; stopping reconnect loop"
                );
                return;
            }
            Err(error) => error,
        };

        let error_class = error.error_class();
        warn!(
            target: "quantick::depth",
            schema_version = 1_u8,
            event_code = "depth_session_disconnected",
            symbol = symbol.as_str(),
            generation,
            error_class,
            error = %error,
            action = "reconnect",
            "depth generation ended"
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
            schema_version = 1_u8,
            event_code = "depth_reconnect_backoff",
            symbol = symbol.as_str(),
            generation,
            attempt = backoff.attempt(),
            delay_ms = delay.as_millis() as u64,
            action = "wait",
            "backing off before depth reconnect"
        );
        tokio::select! {
            () = events.closed() => return,
            () = tokio::time::sleep(delay.max(Duration::from_millis(1))) => {}
        }
        generation = generation.saturating_add(1);
    }
}
