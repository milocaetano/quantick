//! Handing finished things to the consumer, and saying so when it cannot keep
//! up.
//!
//! Every send out of a session goes through here, so "the consumer is gone"
//! and "the consumer is late" are decided in one place instead of at each of
//! the dozen call sites that publish something.

use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tracing::{info, warn};

use quantick_engine::Trade;
use quantick_orderbook::{DepthEvent, DepthStatus};

use crate::latency::LatencyTracker;
use crate::map::TickMapper;

use super::LAG_REPORT_MS;
use super::events::Mt5Event;

/// Publish one live trade, saying so when the consumer's queue is full.
///
/// `Err(())` means the consumer is gone.
///
/// A full queue here is not a harmless wait. This task is the one reading the
/// socket, so a blocked publish stops the read, the receive buffer fills, and
/// the bridge's next `SocketSend` blocks *the terminal's main thread* — the
/// thread that would otherwise be collecting ticks. One slow consumer therefore
/// becomes a late tape at the source, and until now it did that in silence:
/// every hop looked healthy and the chart simply ran behind.
///
/// `try_send` first costs nothing on the happy path and is the only way to tell
/// "the queue was full" from "the send took a while", so the wait is still
/// taken — never a dropped print — but it is a wait with a name on it.
pub(super) async fn send_live(
    tx: &mpsc::Sender<Mt5Event>,
    trade: Trade,
    symbol: &str,
    reported: &mut bool,
) -> Result<(), ()> {
    match tx.try_send(Mt5Event::Live(trade)) {
        Ok(()) => {
            if *reported {
                *reported = false;
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_CONSUMER_KEEPING_UP",
                    symbol = %symbol,
                    "the consumer's queue drained; the socket is being read freely again"
                );
            }
            Ok(())
        }
        Err(mpsc::error::TrySendError::Full(event)) => {
            if !*reported {
                *reported = true;
                warn!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_CONSUMER_BACKPRESSURE",
                    symbol = %symbol,
                    action = "wait_for_room",
                    "the consumer is not draining live trades fast enough; \
                     this stops the socket read, which stalls the bridge's sends"
                );
            }
            tx.send(event).await.map_err(|_| ())
        }
        Err(mpsc::error::TrySendError::Closed(_)) => Err(()),
    }
}

/// Draw a latency sample, log the diagnosis when it crosses, and publish it.
///
/// `Err(())` means the consumer is gone, the same signal every other publish in
/// this module returns.
///
/// This is the one place in the read path that reads a system clock. It is
/// called at most once every
/// [`SAMPLE_EVERY_PRINTS`](crate::latency::SAMPLE_EVERY_PRINTS) prints and once
/// per heartbeat, so a tape printing a thousand times a second pays for
/// roughly sixteen clock reads to be measurable — not a thousand.
pub(super) async fn publish_latency(
    latency: &mut LatencyTracker,
    mapper: &TickMapper,
    symbol: &str,
    lag_reported: &mut bool,
    tx: &mpsc::Sender<Mt5Event>,
) -> Result<(), ()> {
    let Some(sample) = latency.sample(wall_clock_ms(), mapper.server_utc_offset_ms()) else {
        // No live print since the last sample: nothing happened to measure, and
        // republishing the previous window would let a wedged socket show a
        // healthy split forever.
        return Ok(());
    };
    let late = sample.arrival_lag_ms >= LAG_REPORT_MS;
    if late != *lag_reported {
        *lag_reported = late;
        // Named, not absorbed. A tape that falls behind used to be visible only
        // as a number drifting up in the corner of the chart, with nothing
        // anywhere saying which hop was spending the time.
        if late {
            warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_TAPE_LATE",
                symbol = %symbol,
                arrival_lag_ms = sample.arrival_lag_ms,
                terminal_lag_ms = sample.terminal_lag_ms,
                terminal_lag_peak_ms = sample.terminal_lag_peak_ms,
                transport_lag_ms = sample.transport_lag_ms,
                prints = sample.prints,
                hop = sample.dominant().map(crate::latency::LatencyHop::label),
                "the tape is running behind; the hop field says where the time went"
            );
        } else {
            info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_TAPE_CAUGHT_UP",
                symbol = %symbol,
                arrival_lag_ms = sample.arrival_lag_ms,
                "the tape is current again"
            );
        }
    }
    tx.send(Mt5Event::Latency(sample)).await.map_err(|_| ())
}

/// UTC epoch milliseconds.
///
/// The only wall-clock read in this crate, and it exists so a delay can be
/// named. Everything that turns bridge lines into trades stays a pure function
/// of what arrived.
pub(super) fn wall_clock_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Publish one depth status. `Err(())` means the consumer is gone.
pub(super) async fn send_depth_status(
    tx: &mpsc::Sender<Mt5Event>,
    symbol: &str,
    generation: u64,
    status: DepthStatus,
) -> Result<(), ()> {
    tx.send(Mt5Event::Depth(DepthEvent::Status {
        symbol: symbol.to_string(),
        generation,
        status,
    }))
    .await
    .map_err(|_| ())
}
