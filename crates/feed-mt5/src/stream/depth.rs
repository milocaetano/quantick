//! Bounded Depth of Market availability for one bridge connection.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::Instant;
use tracing::{info, warn};

use quantick_orderbook::{DepthEvent, DepthResyncReason, DepthStatus};

use crate::depth::BookMapper;
use crate::protocol;

use super::events::Mt5Event;
use super::ports::BookCaptureSwitch;
use super::publish::send_depth_status;

/// A declared DOM without a usable image stops loading after this wait.
const INITIAL_DEPTH_WAIT: Duration = Duration::from_secs(10);
/// A previously live DOM without fresh usable images is no longer live.
const STALE_DEPTH_WAIT: Duration = Duration::from_secs(30);
/// Capture switches and silence are checked even when the socket stays quiet.
pub(super) const DEPTH_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Depth capture state for one bridge connection.
///
/// Split out because it is the only stateful thing in the message loop besides
/// tick mapping, and it must stay correct across three independent events: the
/// consumer toggling capture, the terminal losing images, and the session
/// ending.
pub(super) struct DepthSession {
    symbol: String,
    /// `None` when the bridge declared no Depth of Market support.
    mapper: Option<BookMapper>,
    /// Whether the consumer has been told a generation is open.
    publishing: bool,
    last_seq: Option<u64>,
    missing_capability_reported: Option<u64>,
    waiting_since: Option<Instant>,
    last_usable_at: Option<Instant>,
    offline: bool,
}

impl DepthSession {
    pub(super) fn new(hello: &protocol::Hello, symbol: String) -> Self {
        Self {
            mapper: hello.book_levels.map(|levels| {
                BookMapper::new(
                    symbol.clone(),
                    0,
                    Some(levels),
                    hello.tick_size.as_deref(),
                    hello.server_utc_offset_s,
                )
            }),
            symbol,
            publishing: false,
            last_seq: None,
            missing_capability_reported: None,
            waiting_since: None,
            last_usable_at: None,
            offline: false,
        }
    }

    pub(super) fn log_capability(&self) {
        match &self.mapper {
            Some(_) => info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_AVAILABLE",
                symbol = %self.symbol,
                "bridge declares Depth of Market support"
            ),
            None => info!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_BOOK_UNSUPPORTED_BY_BRIDGE",
                symbol = %self.symbol,
                action = "trades_only",
                "bridge declares no Depth of Market; the heatmap will stay empty \
                 (recompile bridge/mt5/QuantickBridge.mq5, or the terminal refused the DOM)"
            ),
        }
    }

    pub(super) fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        if let Some(mapper) = self.mapper.as_mut() {
            mapper.set_server_utc_offset_s(offset_s);
        }
    }

    /// The low-frequency driver stamps the deterministic availability check.
    pub(super) async fn check(
        &mut self,
        capture: &BookCaptureSwitch,
        generation_offset: &mut u64,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        self.poll(capture, generation_offset, tx, Instant::now())
            .await
    }

    /// Check capture changes and depth silence independently of socket traffic.
    pub(super) async fn poll(
        &mut self,
        capture: &BookCaptureSwitch,
        generation_offset: &mut u64,
        tx: &mpsc::Sender<Mt5Event>,
        now: Instant,
    ) -> Result<(), ()> {
        let (enabled, base_generation) = capture.state();
        if !enabled {
            self.missing_capability_reported = None;
            self.waiting_since = None;
            self.last_usable_at = None;
            self.offline = false;
            self.last_seq = None;
            if self.publishing {
                self.publishing = false;
                let mapper = self.mapper.as_mut().expect("publishing requires a mapper");
                mapper.restart(mapper.generation());
                send_depth_status(tx, &self.symbol, mapper.generation(), DepthStatus::Stopped)
                    .await?;
            }
            return Ok(());
        }
        let Some(mapper) = self.mapper.as_mut() else {
            return self.report_missing_capability(capture, tx).await;
        };
        let wanted = base_generation.saturating_add(*generation_offset);
        if !self.publishing || mapper.generation() != wanted {
            *generation_offset = generation_offset.saturating_add(1);
            mapper.restart(base_generation.saturating_add(*generation_offset));
            self.publishing = true;
            self.waiting_since = Some(now);
            self.last_usable_at = None;
            self.offline = false;
            self.last_seq = None;
            send_depth_status(
                tx,
                &self.symbol,
                mapper.generation(),
                DepthStatus::Connecting,
            )
            .await?;
        }
        if self.offline {
            return Ok(());
        }
        let (since, limit, error_class) = match self.last_usable_at {
            Some(last) => (last, STALE_DEPTH_WAIT, "stale_depth"),
            None => (
                self.waiting_since.expect("capture opened"),
                INITIAL_DEPTH_WAIT,
                "no_depth_signal",
            ),
        };
        if now.saturating_duration_since(since) < limit {
            return Ok(());
        }
        self.offline = true;
        self.last_seq = None;
        // A new snapshot must follow any silence; never bridge unobserved depth.
        *generation_offset = generation_offset.saturating_add(1);
        mapper.restart(base_generation.saturating_add(*generation_offset));
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BOOK_OFFLINE",
            symbol = %self.symbol,
            error_class,
            wait_s = limit.as_secs(),
            action = "wait_for_usable_image",
            "no fresh usable DOM image; book offline while trades and history stay connected"
        );
        send_depth_status(
            tx,
            &self.symbol,
            mapper.generation(),
            DepthStatus::Disconnected { error_class },
        )
        .await
    }

    /// Handle one image. `Err(())` means the consumer is gone.
    pub(super) async fn observe(
        &mut self,
        image: protocol::Book,
        capture: &BookCaptureSwitch,
        generation_offset: &mut u64,
        tx: &mpsc::Sender<Mt5Event>,
        now: Instant,
    ) -> Result<(), ()> {
        self.poll(capture, generation_offset, tx, now).await?;
        if !self.publishing {
            return Ok(());
        }
        let lost_images = self.images_lost(image.seq);
        let mapper = self.mapper.as_mut().expect("publishing requires a mapper");
        if lost_images && !self.offline {
            send_depth_status(
                tx,
                &self.symbol,
                mapper.generation(),
                DepthStatus::Resyncing {
                    reason: DepthResyncReason::SourceRestarted {
                        cause: "book_images_lost",
                    },
                },
            )
            .await?;
            *generation_offset = generation_offset.saturating_add(1);
            let (_, base_generation) = capture.state();
            mapper.restart(base_generation.saturating_add(*generation_offset));
            send_depth_status(
                tx,
                &self.symbol,
                mapper.generation(),
                DepthStatus::Connecting,
            )
            .await?;
        }
        self.last_seq = Some(image.seq);
        let previous_unchanged = mapper.stats.unchanged;
        let event = mapper.map(&image);
        // An unchanged validated image confirms availability. Invalid and empty
        // images do not keep a previous ladder live indefinitely.
        if event.is_some() || mapper.stats.unchanged > previous_unchanged {
            self.last_usable_at = Some(now);
            self.offline = false;
        }
        let Some(event) = event else {
            return Ok(());
        };
        let synchronized =
            matches!(event, DepthEvent::Snapshot { .. }).then(|| mapper.synchronized_status());
        let generation = mapper.generation();
        tx.send(Mt5Event::Depth(event)).await.map_err(|_| ())?;
        if let Some(status) = synchronized {
            send_depth_status(tx, &self.symbol, generation, status).await?;
        }
        Ok(())
    }

    /// Whether images went missing (or the bridge restarted its counter)
    /// between the last one and `seq`.
    fn images_lost(&self, seq: u64) -> bool {
        match self.last_seq {
            Some(last) => seq != last.saturating_add(1),
            None => false,
        }
    }

    /// Tell a waiting consumer, once, that this bridge cannot supply depth.
    pub(super) async fn report_missing_capability(
        &mut self,
        capture: &BookCaptureSwitch,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        let (enabled, base_generation) = capture.state();
        if self.mapper.is_some()
            || self.missing_capability_reported == Some(base_generation)
            || !enabled
        {
            return Ok(());
        }
        self.missing_capability_reported = Some(base_generation);
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BOOK_UNSUPPORTED_BY_BRIDGE",
            symbol = %self.symbol,
            action = "report_disconnected",
            "depth capture is on but this bridge sends no Depth of Market"
        );
        // Tagged with the consumer's own base generation: a status below the
        // generation floor it is watching would be discarded as stale, and the
        // chart would keep waiting for a book that is never coming.
        send_depth_status(
            tx,
            &self.symbol,
            base_generation,
            DepthStatus::Disconnected {
                error_class: "bridge_without_depth",
            },
        )
        .await
    }

    /// End the generation when the bridge session ends.
    pub(super) async fn close(&mut self, tx: &mpsc::Sender<Mt5Event>) {
        if let Some(mapper) = self.mapper.as_ref() {
            mapper.stats.log_summary(&self.symbol);
            if self.publishing {
                let _ = send_depth_status(
                    tx,
                    &self.symbol,
                    mapper.generation(),
                    DepthStatus::Disconnected {
                        error_class: "bridge_lost",
                    },
                )
                .await;
            }
        }
    }
}

#[cfg(test)]
mod tests;
