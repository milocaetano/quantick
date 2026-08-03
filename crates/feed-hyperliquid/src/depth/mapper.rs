//! Complete Hyperliquid book images to snapshot-plus-delta events.

use std::str::FromStr as _;

use quantick_orderbook::{
    BookCoverage, BookLevel, DepthEvent, DepthStatus, ImageOutcome, SnapshotDiffer,
};
use rust_decimal::Decimal;

use super::wire::{BookImage, RawLevel};

/// Public API coverage documented by Hyperliquid.
pub const HYPERLIQUID_LEVELS_PER_SIDE: usize = 20;

/// Honest ledger for one capture task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BookStats {
    /// Complete images received.
    pub images: u64,
    /// First accepted images of generations.
    pub snapshots: u64,
    /// Changed images published as deltas.
    pub deltas: u64,
    /// Images identical to the previous accepted one.
    pub unchanged: u64,
    /// Crossed/locked images rejected whole.
    pub crossed: u64,
    /// Images rejected due to symbol, time, price, or size.
    pub malformed: u64,
    /// Images rejected because venue time moved behind the last published event.
    pub timestamp_regressions: u64,
}

/// A complete image that cannot be published honestly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookMapError {
    /// Image belongs to another coin.
    Symbol { expected: String, got: String },
    /// Epoch timestamp is negative.
    Timestamp(i64),
    /// Venue time moved behind the last event published by this capture task.
    TimestampRegression {
        /// Last factual timestamp published by the mapper.
        last_published_ms: i64,
        /// Regressive timestamp supplied by the venue.
        got_ms: i64,
    },
    /// Level price is not positive decimal data.
    Price {
        /// `bid` or `ask`.
        side: &'static str,
        /// Invalid token.
        value: String,
    },
    /// Level size is not positive decimal data.
    Size {
        /// `bid` or `ask`.
        side: &'static str,
        /// Invalid token.
        value: String,
    },
    /// Image exceeded the documented public-book coverage.
    LevelCount {
        /// `bid` or `ask`.
        side: &'static str,
        /// Levels carried by the image.
        got: usize,
        /// Maximum levels Hyperliquid documents per side.
        max: usize,
    },
}

impl std::fmt::Display for BookMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Symbol { expected, got } => {
                write!(f, "expected {expected} book, got {got}")
            }
            Self::Timestamp(value) => write!(f, "invalid book timestamp {value}"),
            Self::TimestampRegression {
                last_published_ms,
                got_ms,
            } => write!(
                f,
                "book timestamp regressed to {got_ms} after {last_published_ms}"
            ),
            Self::Price { side, value } => write!(f, "invalid {side} price {value:?}"),
            Self::Size { side, value } => write!(f, "invalid {side} size {value:?}"),
            Self::LevelCount { side, got, max } => {
                write!(f, "{side} image has {got} levels; maximum is {max}")
            }
        }
    }
}

impl std::error::Error for BookMapError {}

/// Stateful full-image differ for one selected coin.
#[derive(Debug)]
pub struct BookMapper {
    symbol: String,
    generation: u64,
    differ: SnapshotDiffer,
    last_event_ms: Option<i64>,
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    /// Mapping and rejection counters.
    pub stats: BookStats,
}

impl BookMapper {
    /// Create a mapper whose next valid image opens `generation`.
    #[must_use]
    pub fn new(symbol: impl Into<String>, generation: u64) -> Self {
        Self {
            symbol: symbol.into().to_uppercase(),
            generation,
            differ: SnapshotDiffer::new(BookCoverage::Limited {
                levels_per_side: HYPERLIQUID_LEVELS_PER_SIDE,
            }),
            last_event_ms: None,
            bids: Vec::with_capacity(HYPERLIQUID_LEVELS_PER_SIDE),
            asks: Vec::with_capacity(HYPERLIQUID_LEVELS_PER_SIDE),
            stats: BookStats::default(),
        }
    }

    /// Start a new consumer generation after continuity ends.
    pub fn restart(&mut self, generation: u64) {
        self.generation = generation;
        self.differ.reset();
    }

    /// Whether this generation has accepted its starting image.
    #[must_use]
    pub fn is_synchronized(&self) -> bool {
        self.differ.is_initialized()
    }

    /// Status matching the current normalized image.
    #[must_use]
    pub fn synchronized_status(&self) -> DepthStatus {
        let (bid_levels, ask_levels) = self.differ.level_counts();
        DepthStatus::Synchronized {
            last_update_id: self.differ.last_update_id(),
            bid_levels,
            ask_levels,
        }
    }

    /// Observe one complete image. `None` means unchanged or crossed.
    ///
    /// # Errors
    ///
    /// Rejects the complete image if any field is malformed or its timestamp
    /// regresses. A partial book or invented replacement time is never
    /// substituted for an authoritative image.
    pub fn map(&mut self, image: &BookImage) -> Result<Option<DepthEvent>, BookMapError> {
        self.stats.images = self.stats.images.saturating_add(1);
        let result = self.map_inner(image);
        if let Err(error) = &result {
            self.stats.malformed = self.stats.malformed.saturating_add(1);
            if matches!(error, BookMapError::TimestampRegression { .. }) {
                self.stats.timestamp_regressions =
                    self.stats.timestamp_regressions.saturating_add(1);
            }
        }
        result
    }

    fn map_inner(&mut self, image: &BookImage) -> Result<Option<DepthEvent>, BookMapError> {
        if !image.coin.eq_ignore_ascii_case(&self.symbol) {
            return Err(BookMapError::Symbol {
                expected: self.symbol.clone(),
                got: image.coin.clone(),
            });
        }
        if image.time < 0 {
            return Err(BookMapError::Timestamp(image.time));
        }
        if let Some(last_published_ms) = self.last_event_ms
            && image.time < last_published_ms
        {
            return Err(BookMapError::TimestampRegression {
                last_published_ms,
                got_ms: image.time,
            });
        }
        validate_level_count(&image.levels[0], "bid")?;
        validate_level_count(&image.levels[1], "ask")?;
        read_levels(&image.levels[0], "bid", &mut self.bids)?;
        read_levels(&image.levels[1], "ask", &mut self.asks)?;

        let event_time_ms = image.time;

        let event = match self.differ.observe(&self.bids, &self.asks) {
            ImageOutcome::Snapshot(snapshot) => {
                self.stats.snapshots = self.stats.snapshots.saturating_add(1);
                Some(DepthEvent::Snapshot {
                    symbol: self.symbol.clone(),
                    generation: self.generation,
                    observed_at_ms: event_time_ms,
                    effective_at_ms: event_time_ms,
                    // Hyperliquid enforces significant-figure price rules, not
                    // one immutable tick size across every price magnitude.
                    price_step: None,
                    snapshot,
                })
            }
            ImageOutcome::Delta(delta) => {
                self.stats.deltas = self.stats.deltas.saturating_add(1);
                Some(DepthEvent::Update {
                    symbol: self.symbol.clone(),
                    generation: self.generation,
                    event_time_ms,
                    delta,
                })
            }
            ImageOutcome::Unchanged => {
                self.stats.unchanged = self.stats.unchanged.saturating_add(1);
                None
            }
            ImageOutcome::Crossed { .. } => {
                self.stats.crossed = self.stats.crossed.saturating_add(1);
                None
            }
        };
        if event.is_some() {
            self.last_event_ms = Some(event_time_ms);
        }
        Ok(event)
    }
}

fn validate_level_count(raw: &[RawLevel], side: &'static str) -> Result<(), BookMapError> {
    if raw.len() > HYPERLIQUID_LEVELS_PER_SIDE {
        return Err(BookMapError::LevelCount {
            side,
            got: raw.len(),
            max: HYPERLIQUID_LEVELS_PER_SIDE,
        });
    }
    Ok(())
}

fn read_levels(
    raw: &[RawLevel],
    side: &'static str,
    target: &mut Vec<BookLevel>,
) -> Result<(), BookMapError> {
    target.clear();
    target.reserve(raw.len());
    for level in raw {
        let price = Decimal::from_str(&level.price).map_err(|_| BookMapError::Price {
            side,
            value: level.price.clone(),
        })?;
        if price <= Decimal::ZERO {
            return Err(BookMapError::Price {
                side,
                value: level.price.clone(),
            });
        }
        let size = Decimal::from_str(&level.size).map_err(|_| BookMapError::Size {
            side,
            value: level.size.clone(),
        })?;
        if size <= Decimal::ZERO {
            return Err(BookMapError::Size {
                side,
                value: level.size.clone(),
            });
        }
        target.push(BookLevel::new(price, size).expect("positive exact level was prevalidated"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(price: &str, size: &str) -> RawLevel {
        RawLevel {
            price: price.to_owned(),
            size: size.to_owned(),
            n: 1,
        }
    }

    fn image(time: i64, bid_size: &str) -> BookImage {
        BookImage {
            coin: "BTC".to_owned(),
            time,
            levels: [
                vec![level("100", bid_size), level("99", "3")],
                vec![level("101", "4"), level("102", "5")],
            ],
        }
    }

    #[test]
    fn first_image_is_limited_snapshot_then_changes_are_deltas() {
        let mut mapper = BookMapper::new("btc", 7);
        let Some(DepthEvent::Snapshot {
            generation,
            snapshot,
            price_step,
            ..
        }) = mapper.map(&image(1_000, "2")).unwrap()
        else {
            panic!("expected snapshot");
        };
        assert_eq!(generation, 7);
        assert_eq!(price_step, None);
        assert_eq!(
            snapshot.coverage(),
            BookCoverage::Limited {
                levels_per_side: HYPERLIQUID_LEVELS_PER_SIDE
            }
        );
        let Some(DepthEvent::Update { delta, .. }) = mapper.map(&image(1_500, "8")).unwrap() else {
            panic!("expected delta");
        };
        assert_eq!(delta.bids().len(), 1);
        assert_eq!(mapper.stats.snapshots, 1);
        assert_eq!(mapper.stats.deltas, 1);
    }

    #[test]
    fn reconnect_starts_a_new_generation_without_reusing_update_id() {
        let mut mapper = BookMapper::new("BTC", 1);
        let first = mapper.map(&image(1_000, "2")).unwrap().unwrap();
        let first_id = match first {
            DepthEvent::Snapshot { snapshot, .. } => snapshot.last_update_id(),
            _ => unreachable!(),
        };
        mapper.restart(2);
        let second = mapper.map(&image(2_000, "2")).unwrap().unwrap();
        let DepthEvent::Snapshot {
            generation,
            snapshot,
            ..
        } = second
        else {
            panic!("expected replacement snapshot");
        };
        assert_eq!(generation, 2);
        assert!(snapshot.last_update_id() > first_id);
    }

    #[test]
    fn malformed_level_rejects_the_entire_image() {
        let mut mapper = BookMapper::new("BTC", 1);
        let mut malformed = image(1_000, "2");
        malformed.levels[1][0].price = "oops".to_owned();
        assert!(matches!(
            mapper.map(&malformed),
            Err(BookMapError::Price { side: "ask", .. })
        ));
        assert!(!mapper.is_synchronized());
        assert_eq!(mapper.stats.malformed, 1);
    }

    #[test]
    fn backwards_time_rejects_the_entire_image_and_preserves_state() {
        let mut mapper = BookMapper::new("BTC", 1);
        let initial = mapper.map(&image(2_000, "2")).unwrap().unwrap();
        let DepthEvent::Snapshot { snapshot, .. } = initial else {
            panic!("expected initial snapshot");
        };
        let initial_update_id = snapshot.last_update_id();
        let status_before = mapper.synchronized_status();

        assert_eq!(
            mapper.map(&image(1_999, "3")),
            Err(BookMapError::TimestampRegression {
                last_published_ms: 2_000,
                got_ms: 1_999,
            })
        );
        assert_eq!(mapper.synchronized_status(), status_before);
        assert_eq!(mapper.stats.snapshots, 1);
        assert_eq!(mapper.stats.deltas, 0);
        assert_eq!(mapper.stats.malformed, 1);
        assert_eq!(mapper.stats.timestamp_regressions, 1);

        let event = mapper.map(&image(2_001, "3")).unwrap().unwrap();
        let DepthEvent::Update {
            event_time_ms,
            delta,
            ..
        } = event
        else {
            panic!("expected delta after rejected image");
        };
        assert_eq!(event_time_ms, 2_001);
        assert_eq!(delta.first_update_id(), initial_update_id + 1);
        assert_eq!(delta.final_update_id(), initial_update_id + 1);
        assert_eq!(delta.bids().len(), 1);
    }

    #[test]
    fn an_oversized_side_is_rejected_before_any_book_is_published() {
        let mut mapper = BookMapper::new("BTC", 1);
        let mut oversized = image(1_000, "2");
        oversized.levels[0] = (0..=HYPERLIQUID_LEVELS_PER_SIDE)
            .map(|offset| level(&(100_u64.saturating_sub(offset as u64)).to_string(), "1"))
            .collect();

        assert_eq!(
            mapper.map(&oversized),
            Err(BookMapError::LevelCount {
                side: "bid",
                got: HYPERLIQUID_LEVELS_PER_SIDE + 1,
                max: HYPERLIQUID_LEVELS_PER_SIDE,
            })
        );
        assert!(!mapper.is_synchronized());
        assert_eq!(mapper.stats.malformed, 1);
    }

    #[test]
    fn an_empty_visible_book_is_published_without_inventing_levels() {
        let mut mapper = BookMapper::new("BTC", 1);
        let empty = BookImage {
            coin: "BTC".to_owned(),
            time: 1_000,
            levels: [Vec::new(), Vec::new()],
        };

        let Some(DepthEvent::Snapshot { snapshot, .. }) = mapper.map(&empty).unwrap() else {
            panic!("expected an empty factual snapshot");
        };
        assert!(snapshot.bids().is_empty());
        assert!(snapshot.asks().is_empty());
        assert!(mapper.is_synchronized());
        assert_eq!(mapper.stats.snapshots, 1);
    }
}
