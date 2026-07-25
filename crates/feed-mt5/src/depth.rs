//! MetaTrader Depth of Market → the neutral depth-stream contract.
//!
//! MT5's book is the opposite shape from an exchange diff stream: no update
//! ids, no incremental messages, no sequencing at all. `OnBookEvent` fires and
//! `MarketBookGet` hands back the entire visible DOM. The bridge forwards those
//! images verbatim (see [`crate::protocol::Book`]) and this module turns them
//! into the same [`DepthEvent`] stream Binance produces, so everything
//! downstream — order book, run-length liquidity history, heatmap projection —
//! is shared rather than forked per venue.
//!
//! The conversion is [`SnapshotDiffer`]: first image becomes a snapshot,
//! every later one becomes a delta of only the levels that moved. B3 republishes
//! ~20 levels many times a second while typically one or two of them changed,
//! so diffing here is what keeps the whole downstream cost proportional to real
//! change rather than to event rate.
//!
//! # What MT5 does not provide, and what this module does about it
//!
//! - **No update ids** → synthetic, monotonic, generation-scoped (the differ's).
//! - **Server wall time** → converted to UTC with the bridge's declared offset,
//!   exactly like ticks, so book and trades share one timeline.
//! - **Truncated depth** → coverage is always
//!   [`BookCoverage::Limited`], never `Full`. Liquidity leaving the visible
//!   window is reported as removed because that is all the terminal knows;
//!   labelling the coverage is what keeps that honest.
//! - **Market-order rows** (`BOOK_TYPE_*_MARKET`, no meaningful price) → skipped
//!   and counted. They are not resting liquidity.
//! - **Crossed images** (B3 publishes them during the pre-open auction) →
//!   rejected and counted; the last uncrossed image stands.

use std::str::FromStr as _;

use rust_decimal::Decimal;
use tracing::{info, warn};

use quantick_orderbook::{
    BookCoverage, BookLevel, DepthEvent, DepthStatus, ImageOutcome, SnapshotDiffer,
};

use crate::protocol::{Book, WireLevel};

/// Depth levels assumed per side when the bridge declares no limit.
///
/// Only used to label coverage; nothing is truncated to it. B3 terminals
/// typically expose a few tens of levels.
const ASSUMED_BOOK_LEVELS: usize = 32;

/// The honest ledger of everything the book mapper did with one session's
/// images. All fields public on purpose: they are data, not behaviour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BookStats {
    /// Images received from the bridge.
    pub images: u64,
    /// Images that opened a generation (one per generation).
    pub snapshots: u64,
    /// Images that produced an absolute delta.
    pub deltas: u64,
    /// Images identical to the previous one; nothing was published.
    pub unchanged: u64,
    /// Images rejected as crossed or locked (auction, stale terminal state).
    pub crossed: u64,
    /// Images rejected because a level was unparseable or negative.
    pub malformed: u64,
    /// Level rows skipped for having no usable price (MT5 market orders).
    pub market_rows: u64,
    /// Images whose timestamp went backwards and was held at the last value.
    pub clamped_timestamps: u64,
}

impl BookStats {
    /// Total images that changed the published book.
    #[must_use]
    pub fn published(&self) -> u64 {
        self.snapshots + self.deltas
    }

    /// Total images that published nothing.
    #[must_use]
    pub fn skipped(&self) -> u64 {
        self.unchanged + self.crossed + self.malformed
    }

    /// Emit the whole ledger as one structured log line (AI-first: a log
    /// excerpt alone answers "why is the heatmap empty / stale?").
    pub fn log_summary(&self, symbol: &str) {
        info!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code = "MT5_BOOK_SUMMARY",
            symbol,
            images = self.images,
            published = self.published(),
            snapshots = self.snapshots,
            deltas = self.deltas,
            skipped = self.skipped(),
            unchanged = self.unchanged,
            crossed = self.crossed,
            malformed = self.malformed,
            market_rows = self.market_rows,
            clamped_timestamps = self.clamped_timestamps,
            "mt5 book mapping summary"
        );
    }
}

/// Stateful DOM image → [`DepthEvent`] mapper for one capture generation.
///
/// Deterministic: the same images with the same offset always produce the same
/// events. Reusable scratch buffers keep the steady state free of per-image
/// allocation apart from the emitted delta's own levels.
#[derive(Debug)]
pub struct BookMapper {
    symbol: String,
    generation: u64,
    price_step: Option<Decimal>,
    /// `server_time - utc`, in milliseconds (from hello, refreshed by
    /// heartbeats) — the same conversion ticks use.
    offset_ms: i64,
    differ: SnapshotDiffer,
    last_utc_ms: Option<i64>,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    /// The honest ledger of everything mapped, skipped and why.
    pub stats: BookStats,
}

impl BookMapper {
    /// A mapper for `generation`, publishing events tagged with `symbol`.
    ///
    /// `book_levels` and `tick_size` come from the bridge's hello: the first
    /// labels how much of the exchange book the terminal can see, the second
    /// lets the consumer render on the instrument's real price grid.
    #[must_use]
    pub fn new(
        symbol: impl Into<String>,
        generation: u64,
        book_levels: Option<u32>,
        tick_size: Option<&str>,
        server_utc_offset_s: i64,
    ) -> Self {
        let levels_per_side = book_levels
            .and_then(|levels| usize::try_from(levels).ok())
            .filter(|levels| *levels > 0)
            .unwrap_or(ASSUMED_BOOK_LEVELS);
        Self {
            symbol: symbol.into(),
            generation,
            price_step: tick_size
                .and_then(|size| Decimal::from_str(size).ok())
                .filter(|size| *size > Decimal::ZERO),
            // Saturating: the offset is declared by whatever dialed our port,
            // so an absurd value must not panic the feed task.
            offset_ms: server_utc_offset_s.saturating_mul(1000),
            differ: SnapshotDiffer::new(BookCoverage::Limited { levels_per_side }),
            last_utc_ms: None,
            bids: Vec::new(),
            asks: Vec::new(),
            stats: BookStats::default(),
        }
    }

    /// The generation this mapper currently publishes.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether an image has been published since the last restart.
    #[must_use]
    pub fn is_synchronized(&self) -> bool {
        self.differ.is_initialized()
    }

    /// Status describing the currently published book, for a consumer badge.
    #[must_use]
    pub fn synchronized_status(&self) -> DepthStatus {
        let (bid_levels, ask_levels) = self.differ.level_counts();
        DepthStatus::Synchronized {
            last_update_id: self.differ.last_update_id(),
            bid_levels,
            ask_levels,
        }
    }

    /// Refresh the server-time offset (heartbeats may recompute it, e.g.
    /// across a DST change on brokers that observe one).
    pub fn set_server_utc_offset_s(&mut self, offset_s: i64) {
        self.offset_ms = offset_s.saturating_mul(1000);
    }

    /// Start publishing `generation` from a fresh snapshot.
    ///
    /// Continuity across a restart is never implied: the consumer sees a new
    /// generation and can render the discontinuity instead of connecting
    /// liquidity across it.
    pub fn restart(&mut self, generation: u64) {
        self.generation = generation;
        self.differ.reset();
        self.last_utc_ms = None;
    }

    /// Map one DOM image. Returns the event to publish, if any.
    pub fn map(&mut self, image: &Book) -> Option<DepthEvent> {
        self.stats.images += 1;

        if read_levels(&image.bids, &mut self.bids, &mut self.stats).is_none()
            || read_levels(&image.asks, &mut self.asks, &mut self.stats).is_none()
        {
            self.stats.malformed += 1;
            warn_once(
                self.stats.malformed,
                "MT5_BOOK_MALFORMED",
                &self.symbol,
                image.seq,
                "book image had an unparseable or negative level; keeping the previous image",
            );
            return None;
        }

        let event_time_ms = self.timeline_ms(image.time_ms);

        match self.differ.observe(&self.bids, &self.asks) {
            ImageOutcome::Snapshot(snapshot) => {
                self.stats.snapshots += 1;
                info!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BOOK_SYNCHRONIZED",
                    symbol = %self.symbol,
                    generation = self.generation,
                    seq = image.seq,
                    bid_levels = snapshot.bids().len(),
                    ask_levels = snapshot.asks().len(),
                    coverage = ?snapshot.coverage(),
                    "first DOM image accepted; book capture is live"
                );
                Some(DepthEvent::Snapshot {
                    symbol: self.symbol.clone(),
                    generation: self.generation,
                    // The terminal timestamps the image itself, so observation
                    // and effect are the same instant — there is no separate
                    // local fetch to distinguish, unlike a REST snapshot.
                    observed_at_ms: event_time_ms,
                    effective_at_ms: event_time_ms,
                    price_step: self.price_step,
                    snapshot,
                })
            }
            ImageOutcome::Delta(delta) => {
                self.stats.deltas += 1;
                Some(DepthEvent::Update {
                    symbol: self.symbol.clone(),
                    generation: self.generation,
                    event_time_ms,
                    delta,
                })
            }
            ImageOutcome::Unchanged => {
                self.stats.unchanged += 1;
                None
            }
            ImageOutcome::Crossed { best_bid, best_ask } => {
                self.stats.crossed += 1;
                warn_once(
                    self.stats.crossed,
                    "MT5_BOOK_CROSSED",
                    &self.symbol,
                    image.seq,
                    "DOM image was crossed (auction?); keeping the last uncrossed image",
                );
                tracing::debug!(
                    target: "quantick::feed",
                    schema_version = 1_u8,
                    event_code = "MT5_BOOK_CROSSED",
                    symbol = %self.symbol,
                    seq = image.seq,
                    best_bid = %best_bid,
                    best_ask = %best_ask,
                    total_crossed = self.stats.crossed,
                    "crossed DOM image rejected"
                );
                None
            }
        }
    }

    /// Convert server time to UTC and keep the published timeline monotonic.
    ///
    /// A book timestamp that goes backwards would be refused downstream and
    /// cost the whole generation. Holding it at the last value keeps the
    /// capture alive; the counter and log say how often it happened, so a
    /// terminal with a jumping clock is diagnosable rather than invisible.
    fn timeline_ms(&mut self, server_time_ms: i64) -> i64 {
        let utc_ms = server_time_ms.saturating_sub(self.offset_ms);
        let published = match self.last_utc_ms {
            Some(last) if utc_ms < last => {
                self.stats.clamped_timestamps += 1;
                warn_once(
                    self.stats.clamped_timestamps,
                    "MT5_BOOK_TIME_BACKWARDS",
                    &self.symbol,
                    0,
                    "DOM image timestamp went backwards; holding the previous instant",
                );
                last
            }
            _ => utc_ms,
        };
        self.last_utc_ms = Some(published);
        published
    }
}

/// Parse one side into `dst`. `None` means the image must be rejected whole:
/// a book image is one atomic observation, and publishing it minus the rows we
/// failed to read would invent liquidity that is not there.
fn read_levels(src: &[WireLevel], dst: &mut Vec<BookLevel>, stats: &mut BookStats) -> Option<()> {
    dst.clear();
    dst.reserve(src.len());
    for WireLevel(price, quantity) in src {
        let (Ok(price), Ok(quantity)) = (Decimal::from_str(price), Decimal::from_str(quantity))
        else {
            return None;
        };
        if quantity < Decimal::ZERO {
            return None;
        }
        // MT5 reports market-order rows with no usable price. They are orders
        // waiting to cross, not resting liquidity at a level.
        if price <= Decimal::ZERO {
            stats.market_rows += 1;
            continue;
        }
        match BookLevel::new(price, quantity) {
            Ok(level) => dst.push(level),
            Err(_) => return None,
        }
    }
    Some(())
}

/// Log the first occurrence of a recurring condition at warn level. A crossed
/// auction book repeats for minutes; one warning plus a counter in the session
/// summary tells the story without flooding the log.
fn warn_once(total: u64, event_code: &'static str, symbol: &str, seq: u64, message: &str) {
    if total == 1 {
        warn!(
            target: "quantick::feed",
            schema_version = 1_u8,
            event_code,
            symbol,
            seq,
            action = "keep_previous_image",
            "{message}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B3's real offset: server time is UTC−3.
    const OFFSET_S: i64 = -10_800;

    fn wire(levels: &[(&str, &str)]) -> Vec<WireLevel> {
        levels
            .iter()
            .map(|(price, quantity)| WireLevel((*price).to_string(), (*quantity).to_string()))
            .collect()
    }

    fn image(seq: u64, time_ms: i64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> Book {
        Book {
            seq,
            time_ms,
            bids: wire(bids),
            asks: wire(asks),
        }
    }

    fn mapper() -> BookMapper {
        BookMapper::new("WIN$N", 7, Some(20), Some("5"), OFFSET_S)
    }

    #[test]
    fn the_first_image_is_a_snapshot_in_utc_with_the_declared_tick_size() {
        let mut mapper = mapper();
        let event = mapper
            .map(&image(
                1,
                1_000,
                &[("177795", "3"), ("177790", "12")],
                &[("177800", "5")],
            ))
            .expect("first image publishes a snapshot");
        let DepthEvent::Snapshot {
            symbol,
            generation,
            observed_at_ms,
            effective_at_ms,
            price_step,
            snapshot,
        } = event
        else {
            panic!("expected a snapshot");
        };
        assert_eq!(symbol, "WIN$N");
        assert_eq!(generation, 7);
        // Server time minus the declared offset: 1000 - (-10_800_000).
        assert_eq!(observed_at_ms, 1_000 + 10_800_000);
        assert_eq!(effective_at_ms, observed_at_ms);
        assert_eq!(price_step, Some(Decimal::from(5)));
        assert_eq!(snapshot.bids().len(), 2);
        assert_eq!(
            snapshot.coverage(),
            BookCoverage::Limited {
                levels_per_side: 20
            }
        );
        assert!(mapper.is_synchronized());
    }

    #[test]
    fn a_republished_identical_image_publishes_nothing() {
        let mut mapper = mapper();
        let picture = image(1, 1_000, &[("177795", "3")], &[("177800", "5")]);
        assert!(mapper.map(&picture).is_some());
        assert!(
            mapper
                .map(&image(2, 1_100, &[("177795", "3")], &[("177800", "5")]))
                .is_none()
        );
        assert_eq!(mapper.stats.unchanged, 1);
        assert_eq!(mapper.stats.deltas, 0);
    }

    #[test]
    fn a_changed_image_publishes_only_the_levels_that_moved() {
        let mut mapper = mapper();
        mapper.map(&image(
            1,
            1_000,
            &[("177795", "3"), ("177790", "12")],
            &[("177800", "5")],
        ));
        let event = mapper
            .map(&image(
                2,
                1_500,
                &[("177795", "1"), ("177790", "12")],
                &[("177800", "5")],
            ))
            .expect("a changed image publishes a delta");
        let DepthEvent::Update {
            event_time_ms,
            delta,
            ..
        } = event
        else {
            panic!("expected an update");
        };
        assert_eq!(event_time_ms, 1_500 + 10_800_000);
        assert_eq!(delta.bids().len(), 1);
        assert_eq!(delta.bids()[0].quantity(), Decimal::ONE);
        assert!(delta.asks().is_empty());
    }

    #[test]
    fn market_order_rows_are_skipped_and_counted() {
        let mut mapper = mapper();
        let event = mapper
            .map(&image(
                1,
                1_000,
                &[("0", "40"), ("177795", "3")],
                &[("177800", "5")],
            ))
            .expect("a snapshot");
        let DepthEvent::Snapshot { snapshot, .. } = event else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.bids().len(), 1);
        assert_eq!(mapper.stats.market_rows, 1);
    }

    #[test]
    fn a_malformed_image_is_rejected_whole_and_the_previous_one_stands() {
        let mut mapper = mapper();
        mapper.map(&image(1, 1_000, &[("177795", "3")], &[("177800", "5")]));
        assert!(
            mapper
                .map(&image(
                    2,
                    1_100,
                    &[("not-a-price", "3")],
                    &[("177800", "5")]
                ))
                .is_none()
        );
        assert!(
            mapper
                .map(&image(3, 1_200, &[("177795", "-3")], &[("177800", "5")]))
                .is_none()
        );
        assert_eq!(mapper.stats.malformed, 2);
        // The surviving image still diffs normally.
        let event = mapper.map(&image(4, 1_300, &[("177795", "9")], &[("177800", "5")]));
        assert!(matches!(event, Some(DepthEvent::Update { .. })));
    }

    #[test]
    fn a_crossed_auction_image_is_rejected_and_counted() {
        let mut mapper = mapper();
        mapper.map(&image(1, 1_000, &[("177795", "3")], &[("177800", "5")]));
        assert!(
            mapper
                .map(&image(2, 1_100, &[("177805", "3")], &[("177800", "5")]))
                .is_none()
        );
        assert_eq!(mapper.stats.crossed, 1);
        assert_eq!(mapper.stats.published(), 1);
    }

    #[test]
    fn a_backwards_timestamp_is_held_not_published_backwards() {
        let mut mapper = mapper();
        mapper.map(&image(1, 5_000, &[("177795", "3")], &[("177800", "5")]));
        let event = mapper
            .map(&image(2, 4_000, &[("177795", "9")], &[("177800", "5")]))
            .expect("a delta");
        let DepthEvent::Update { event_time_ms, .. } = event else {
            panic!("expected an update");
        };
        assert_eq!(event_time_ms, 5_000 + 10_800_000);
        assert_eq!(mapper.stats.clamped_timestamps, 1);
    }

    #[test]
    fn a_restart_opens_a_new_generation_with_a_fresh_snapshot() {
        let mut mapper = mapper();
        mapper.map(&image(1, 1_000, &[("177795", "3")], &[("177800", "5")]));
        mapper.restart(8);
        let event = mapper
            .map(&image(2, 1_100, &[("177795", "3")], &[("177800", "5")]))
            .expect("a fresh snapshot after the restart");
        let DepthEvent::Snapshot {
            generation,
            snapshot,
            ..
        } = event
        else {
            panic!("expected a snapshot");
        };
        assert_eq!(generation, 8);
        // Update ids never restart, so a late event from the old generation
        // can never look current.
        assert_eq!(snapshot.last_update_id(), 2);
    }

    #[test]
    fn an_undeclared_tick_size_stays_undeclared() {
        // Never invent a price grid: the consumer falls back to its own
        // heuristic and labels it, rather than trusting a made-up step.
        let mut mapper = BookMapper::new("WIN$N", 1, None, None, OFFSET_S);
        let event = mapper
            .map(&image(1, 1_000, &[("177795", "3")], &[("177800", "5")]))
            .expect("a snapshot");
        let DepthEvent::Snapshot {
            price_step,
            snapshot,
            ..
        } = event
        else {
            panic!("expected a snapshot");
        };
        assert_eq!(price_step, None);
        assert_eq!(
            snapshot.coverage(),
            BookCoverage::Limited {
                levels_per_side: ASSUMED_BOOK_LEVELS
            }
        );
    }
}
