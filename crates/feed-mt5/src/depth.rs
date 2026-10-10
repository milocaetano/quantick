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

/// Shortest spacing, in book event time, between two published confirmations
/// of an unchanged image.
///
/// The bridges resend an unchanged DOM every 50 ms of their own monotonic
/// clock (`BOOK_CONFIRM_INTERVAL_MS`), but stamp each image with the
/// terminal's time, so the spacing seen here jitters around 50 ms (48, 52,
/// ...). A floor equal to the cadence would drop every confirmation that lands
/// a millisecond early. Well under the cadence passes all of them and still
/// bounds an image that differs only in rows the differ drops to 50
/// confirmations a second instead of one per terminal event.
const MIN_CONFIRM_SPACING_MS: i64 = 20;

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
    /// Images identical to the previous one, too soon after the last
    /// published event to confirm it; nothing was published.
    pub unchanged: u64,
    /// Images identical to the previous one, published as an empty delta
    /// that confirms the book at the image's instant.
    pub confirmed: u64,
    /// Images rejected as crossed or locked (auction, stale terminal state).
    pub crossed: u64,
    /// Images rejected because a level was unparseable or negative.
    pub malformed: u64,
    /// Images without any resting liquidity; not evidence of a live DOM.
    pub empty: u64,
    /// Level rows skipped for having no usable price (MT5 market orders).
    pub market_rows: u64,
    /// Images whose timestamp went backwards and was held at the last value.
    pub clamped_timestamps: u64,
}

impl BookStats {
    /// Total images published to the book stream: snapshots, deltas and
    /// confirmations. With [`skipped`](Self::skipped) it accounts for every
    /// image: `images == published() + skipped()`.
    #[must_use]
    pub fn published(&self) -> u64 {
        self.snapshots + self.deltas + self.confirmed
    }

    /// Total images that published nothing.
    #[must_use]
    pub fn skipped(&self) -> u64 {
        self.unchanged + self.crossed + self.malformed + self.empty
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
            confirmed = self.confirmed,
            crossed = self.crossed,
            malformed = self.malformed,
            empty = self.empty,
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
    /// Event time of the newest published snapshot, delta or confirmation.
    last_published_ms: Option<i64>,
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
            last_published_ms: None,
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
        self.last_published_ms = None;
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

        if self
            .bids
            .iter()
            .chain(&self.asks)
            .all(|level| level.quantity().is_zero())
        {
            self.stats.empty += 1;
            return None;
        }

        let event_time_ms = self.timeline_ms(image.time_ms);

        match self.differ.observe(&self.bids, &self.asks) {
            ImageOutcome::Snapshot(snapshot) => {
                self.stats.snapshots += 1;
                self.last_published_ms = Some(event_time_ms);
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
                self.last_published_ms = Some(event_time_ms);
                Some(DepthEvent::Update {
                    symbol: self.symbol.clone(),
                    generation: self.generation,
                    event_time_ms,
                    delta,
                })
            }
            ImageOutcome::Unchanged => {
                // A fresh read of a book that has not changed is still an
                // observation: published, it moves the book clock to this
                // instant, so the resting book is drawn to where it was seen
                // and no further.
                let due = self.last_published_ms.is_none_or(|last| {
                    event_time_ms.saturating_sub(last) >= MIN_CONFIRM_SPACING_MS
                });
                let Some(delta) = due.then(|| self.differ.confirm()).flatten() else {
                    self.stats.unchanged += 1;
                    return None;
                };
                self.stats.confirmed += 1;
                self.last_published_ms = Some(event_time_ms);
                Some(DepthEvent::Update {
                    symbol: self.symbol.clone(),
                    generation: self.generation,
                    event_time_ms,
                    delta,
                })
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
    fn an_identical_image_soon_after_the_last_event_publishes_nothing() {
        let mut mapper = mapper();
        let picture = image(1, 1_000, &[("177795", "3")], &[("177800", "5")]);
        assert!(mapper.map(&picture).is_some());
        assert!(
            mapper
                .map(&image(2, 1_010, &[("177795", "3")], &[("177800", "5")]))
                .is_none()
        );
        assert_eq!(mapper.stats.unchanged, 1);
        assert_eq!(mapper.stats.deltas, 0);
    }

    /// The bridge re-reads an unchanged DOM and sends it again: that is an
    /// observation that the book still stood at that instant. It is published
    /// as an empty delta so the book clock reaches it — without it, a book
    /// that changes three times a second stops short of every newer print.
    #[test]
    fn an_identical_image_confirms_the_book_at_its_own_instant() {
        let mut mapper = mapper();
        let mut book = quantick_orderbook::OrderBook::new();
        let mut apply = |event: Option<DepthEvent>| match event {
            Some(DepthEvent::Snapshot { snapshot, .. }) => {
                book.install_snapshot(snapshot).unwrap();
                None
            }
            Some(DepthEvent::Update {
                event_time_ms,
                delta,
                ..
            }) => {
                let outcome = book.apply_delta(&delta).unwrap();
                Some((event_time_ms, delta, outcome))
            }
            other => panic!("unexpected event {other:?}"),
        };

        apply(mapper.map(&image(1, 1_000, &[("177795", "3")], &[("177800", "5")])));
        let (confirmed_ms, confirmation, outcome) =
            apply(mapper.map(&image(2, 1_100, &[("177795", "3")], &[("177800", "5")])))
                .expect("an identical image a confirmation interval later is published");
        assert_eq!(confirmed_ms, 1_100 + 10_800_000);
        assert!(confirmation.bids().is_empty() && confirmation.asks().is_empty());
        assert_eq!(
            outcome,
            quantick_orderbook::ApplyOutcome::Applied {
                first_update_id: 2,
                final_update_id: 2,
                changed_levels: 0,
            },
            "a confirmation is a real, sequenced event that changes no level"
        );

        // The next real change continues the sequence: no gap, no stale id.
        let (_, delta, _) =
            apply(mapper.map(&image(3, 1_120, &[("177795", "4")], &[("177800", "5")])))
                .expect("a changed image publishes a delta");
        assert_eq!(delta.first_update_id(), 3);
        assert_eq!(mapper.stats.deltas, 1);
        assert_eq!(mapper.stats.confirmed, 1);
        assert_eq!(mapper.stats.unchanged, 0);
    }

    /// The Python bridge resends an unchanged DOM every 50 ms of its own
    /// monotonic clock but stamps each image with the terminal's time, so
    /// the spacing the mapper sees jitters around the cadence. Every
    /// confirmation must still reach the book, and the ledger must balance.
    #[test]
    fn every_confirmation_at_the_bridge_cadence_is_published_despite_jitter() {
        let mut mapper = mapper();
        let mut book = quantick_orderbook::OrderBook::new();
        let bids = [("177795", "3")];
        let asks = [("177800", "5")];
        let mut stamp = 1_000;
        let Some(DepthEvent::Snapshot { snapshot, .. }) =
            mapper.map(&image(1, stamp, &bids, &asks))
        else {
            panic!("the first image is a snapshot");
        };
        book.install_snapshot(snapshot).unwrap();

        let mut book_ms = None;
        for (seq, spacing) in (2..).zip([49, 51, 50, 48, 52, 50, 49, 51, 50]) {
            stamp += spacing;
            match mapper.map(&image(seq, stamp, &bids, &asks)) {
                Some(DepthEvent::Update {
                    event_time_ms,
                    delta,
                    ..
                }) => {
                    assert!(delta.bids().is_empty() && delta.asks().is_empty());
                    book.apply_delta(&delta)
                        .expect("a confirmation stays in sequence");
                    book_ms = Some(event_time_ms);
                }
                other => panic!("the confirmation at {stamp} was not published: {other:?}"),
            }
        }

        assert_eq!(
            book_ms,
            Some(stamp - OFFSET_S * 1000),
            "the book reaches the newest stamp"
        );
        assert_eq!(mapper.stats.confirmed, 9);
        assert_eq!(mapper.stats.unchanged, 0);
        assert_eq!(
            mapper.stats.published(),
            10,
            "a snapshot and nine confirmations"
        );
        assert_eq!(
            mapper.stats.images,
            mapper.stats.published() + mapper.stats.skipped(),
            "every image is published or skipped"
        );
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
