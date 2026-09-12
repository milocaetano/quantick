//! The two block state machines one session runs.
//!
//! A historical candle block ([`RatesBlock`]) and the depth capture state
//! ([`DepthSession`]) are the only things in the message loop that outlive a
//! single line, so they are named here rather than left as loose variables.

use std::collections::BTreeMap;

use tokio::sync::mpsc;
use tracing::{info, warn};

use quantick_engine::Bar;
use quantick_orderbook::{DepthEvent, DepthResyncReason, DepthStatus};

use crate::depth::BookMapper;
use crate::protocol;
use crate::rates::RateMapper;

use super::events::Mt5Event;
use super::ports::BookCaptureSwitch;
use super::publish::send_depth_status;

/// Most candles one block may deliver.
///
/// The bridge caps what it sends (`--rates-max-bars`) and logs the shortfall,
/// but the bridge is the side this one cannot vouch for: a misconfigured or
/// hostile one could stream candles until the feed runs out of memory. Ninety
/// days of one-minute buckets is ~130 000, so this is comfortably above any
/// legitimate block while still being a bound.
pub(super) const MAX_BARS_PER_BLOCK: usize = 1_000_000;

/// The historical candle block being received, between `rates_start` and
/// `rates_end`.
///
/// Bars land in a [`BTreeMap`] keyed by `open_time` rather than a `Vec`: the
/// terminal can repeat a bucket across chunk boundaries, and a map both settles
/// that (last write wins — a repeat is a correction) and hands back one
/// ascending series without a sort whose tie-breaking would be an unstated
/// rule. Same block in, same series out, whatever order the chunks arrived in.
pub(super) struct RatesBlock {
    interval_ms: i64,
    mapper: RateMapper,
    bars: BTreeMap<i64, Bar>,
    /// Whether the cap has been hit, so it is reported once rather than per row.
    truncated: bool,
}

impl RatesBlock {
    pub(super) fn new(interval_ms: i64, server_utc_offset_s: i64) -> Self {
        Self {
            interval_ms,
            mapper: RateMapper::new(interval_ms, server_utc_offset_s),
            bars: BTreeMap::new(),
            truncated: false,
        }
    }

    /// Map and absorb one chunk. Unreadable rows are counted, not fatal: one
    /// corrupt candle in ninety days is a gap, not a reason to lose the block.
    pub(super) fn absorb(&mut self, chunk: &protocol::RateChunk) {
        for row in &chunk.bars {
            if self.bars.len() >= MAX_BARS_PER_BLOCK {
                if !self.truncated {
                    self.truncated = true;
                    warn!(
                        target: "quantick::feed",
                        schema_version = 1_u8,
                        event_code = "MT5_RATES_TRUNCATED",
                        max_bars = MAX_BARS_PER_BLOCK as u64,
                        action = "keep_oldest_stop_absorbing",
                        "the candle block exceeded the cap; ignoring the rest of it"
                    );
                }
                return;
            }
            if let Some(bar) = self.mapper.map(row) {
                self.bars.insert(bar.open_time, bar);
            }
        }
    }

    pub(super) fn len(&self) -> usize {
        self.bars.len()
    }

    /// Close the block: log what it cost, and hand back the ascending series.
    pub(super) fn finish(self, symbol: &str) -> (i64, Vec<Bar>, bool) {
        self.mapper.stats.log_summary(symbol, self.interval_ms);
        // Clipping here is the same kind of shortfall the bridge reports with
        // its own `partial`: bars that exist and were not delivered.
        let clipped = self.truncated;
        (self.interval_ms, self.bars.into_values().collect(), clipped)
    }
}

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
    missing_capability_reported: bool,
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
            missing_capability_reported: false,
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

    /// Handle one image. `Err(())` means the consumer is gone.
    pub(super) async fn observe(
        &mut self,
        image: protocol::Book,
        capture: &BookCaptureSwitch,
        generation_offset: &mut u64,
        tx: &mpsc::Sender<Mt5Event>,
    ) -> Result<(), ()> {
        if self.mapper.is_none() {
            // A bridge sending images it never declared is a version skew, not
            // data to trust silently.
            return Ok(());
        }
        let (enabled, base_generation) = capture.state();
        let lost_images = self.images_lost(image.seq);
        let mapper = self.mapper.as_mut().expect("checked above");
        if !enabled {
            if self.publishing {
                let generation = mapper.generation();
                self.publishing = false;
                self.last_seq = None;
                mapper.restart(generation); // next capture starts from a snapshot
                send_depth_status(tx, &self.symbol, generation, DepthStatus::Stopped).await?;
            }
            return Ok(());
        }

        // Open a generation when capture starts, when the consumer moves its
        // base, or when images were lost and the diff would silently bridge a
        // moment we never observed.
        let wanted = base_generation.saturating_add(*generation_offset);
        if !self.publishing || mapper.generation() != wanted || lost_images {
            if lost_images {
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
            }
            *generation_offset = generation_offset.saturating_add(1);
            let generation = base_generation.saturating_add(*generation_offset);
            mapper.restart(generation);
            self.publishing = true;
            send_depth_status(tx, &self.symbol, generation, DepthStatus::Connecting).await?;
        }
        self.last_seq = Some(image.seq);

        let Some(event) = mapper.map(&image) else {
            return Ok(());
        };
        let synchronized =
            matches!(event, DepthEvent::Snapshot { .. }).then(|| mapper.synchronized_status());
        let generation = mapper.generation();
        if tx.send(Mt5Event::Depth(event)).await.is_err() {
            return Err(());
        }
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
        if self.mapper.is_some() || self.missing_capability_reported || !enabled {
            return Ok(());
        }
        self.missing_capability_reported = true;
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
