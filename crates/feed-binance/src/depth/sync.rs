//! Deterministic Binance snapshot + diff synchronization.
//!
//! This module has no network, async or wall clock. A caller installs one REST
//! snapshot and then applies WebSocket updates. The first non-stale update must
//! bridge `snapshot.last_update_id + 1`; every later update must overlap the
//! next expected id. A gap is returned as typed data and logged with stable
//! fields so the transport can discard the book and resynchronize.

use quantick_orderbook::{ApplyOutcome, BookError, OrderBook};
use tracing::{debug, info, warn};

use super::wire::{DepthSnapshot, DepthUpdate};

/// Synchronizer lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncPhase {
    /// No snapshot has been installed.
    AwaitingSnapshot,
    /// Snapshot installed; waiting for an update that bridges its id.
    AwaitingBridge {
        /// Update id represented by the installed snapshot.
        snapshot_update_id: u64,
    },
    /// Snapshot and stream have been bridged.
    Synchronized {
        /// Final update id applied to the local book.
        current_update_id: u64,
    },
}

/// Result of installing a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootstrapOutcome {
    /// Snapshot update id.
    pub snapshot_update_id: u64,
    /// Installed bid level count.
    pub bid_levels: usize,
    /// Installed ask level count.
    pub ask_levels: usize,
}

/// Result of observing one diff update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncApplyOutcome {
    /// The update was older than or equal to the current local book.
    Stale {
        /// Event final id.
        final_update_id: u64,
        /// Current local-book id.
        current_update_id: u64,
    },
    /// The update was applied.
    Applied {
        /// Whether this was the first update bridging the snapshot.
        bridged_snapshot: bool,
        /// Event first id.
        first_update_id: u64,
        /// Event final id.
        final_update_id: u64,
        /// Number of price levels changed.
        changed_levels: usize,
        /// Current bid level count.
        bid_levels: usize,
        /// Current ask level count.
        ask_levels: usize,
    },
}

/// An update that cannot be safely applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthApplyError {
    /// No snapshot has been installed.
    SnapshotRequired,
    /// A message from a different symbol reached this raw stream.
    SymbolMismatch {
        /// Configured symbol.
        expected: String,
        /// Payload symbol.
        got: String,
    },
    /// At least one update id was missed.
    Gap {
        /// Current local-book update id.
        current_update_id: u64,
        /// Next id that should have been covered.
        expected_update_id: u64,
        /// First id in the received event.
        got_first_update_id: u64,
        /// Final id in the received event.
        got_final_update_id: u64,
    },
    /// The generic order-book rejected the snapshot or delta.
    Book(BookError),
}

impl std::fmt::Display for DepthApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SnapshotRequired => write!(f, "a depth snapshot is required"),
            Self::SymbolMismatch { expected, got } => {
                write!(f, "depth symbol mismatch: expected {expected}, got {got}")
            }
            Self::Gap {
                expected_update_id,
                got_first_update_id,
                got_final_update_id,
                ..
            } => write!(
                f,
                "depth sequence gap: expected {expected_update_id}, got [{got_first_update_id}, {got_final_update_id}]"
            ),
            Self::Book(error) => write!(f, "order-book error: {error}"),
        }
    }
}

impl std::error::Error for DepthApplyError {}

impl From<BookError> for DepthApplyError {
    fn from(error: BookError) -> Self {
        match error {
            BookError::SequenceGap {
                expected_update_id,
                first_update_id,
                final_update_id,
            } => Self::Gap {
                current_update_id: expected_update_id.saturating_sub(1),
                expected_update_id,
                got_first_update_id: first_update_id,
                got_final_update_id: final_update_id,
            },
            other => Self::Book(other),
        }
    }
}

/// Binance depth synchronizer backed by the reusable deterministic order book.
#[derive(Debug, Clone)]
pub struct DepthSynchronizer {
    symbol: String,
    generation: u64,
    phase: SyncPhase,
    book: OrderBook,
}

impl DepthSynchronizer {
    /// A fresh synchronizer for one symbol/session generation.
    #[must_use]
    pub fn new(symbol: impl Into<String>, generation: u64) -> Self {
        Self {
            symbol: symbol.into().to_uppercase(),
            generation,
            phase: SyncPhase::AwaitingSnapshot,
            book: OrderBook::new(),
        }
    }

    /// Discard all local state before retrieving a new snapshot.
    pub fn reset(&mut self) {
        self.phase = SyncPhase::AwaitingSnapshot;
        self.book = OrderBook::new();
    }

    /// Install a REST snapshot, but do not call the session synchronized until
    /// the first eligible WebSocket event bridges it.
    ///
    /// `requested_levels` records the REST coverage honestly: Binance returns
    /// at most that many levels on each side, so levels outside that window are
    /// not known until they change.
    ///
    /// # Errors
    ///
    /// Returns [`DepthApplyError::Book`] if the generic book rejects a level or
    /// detects a crossed snapshot.
    pub fn install_snapshot(
        &mut self,
        snapshot: &DepthSnapshot,
        requested_levels: usize,
    ) -> Result<BootstrapOutcome, DepthApplyError> {
        let book_snapshot = snapshot.to_book_snapshot(requested_levels)?;
        self.book.install_snapshot(book_snapshot)?;
        self.phase = SyncPhase::AwaitingBridge {
            snapshot_update_id: snapshot.last_update_id,
        };
        let outcome = BootstrapOutcome {
            snapshot_update_id: snapshot.last_update_id,
            bid_levels: self.book.bid_count(),
            ask_levels: self.book.ask_count(),
        };
        info!(
            target: "quantick::depth",
            schema_version = 1_u8,
            event_code = "depth_snapshot_installed",
            symbol = self.symbol.as_str(),
            generation = self.generation,
            snapshot_update_id = snapshot.last_update_id,
            bid_levels = outcome.bid_levels,
            ask_levels = outcome.ask_levels,
            action = "await_bridge",
            "depth snapshot installed; awaiting stream bridge"
        );
        Ok(outcome)
    }

    /// Apply one absolute diff-depth event.
    ///
    /// # Errors
    ///
    /// Returns a typed symbol mismatch, missing snapshot, sequence gap or
    /// generic book invariant error. A gap never mutates the local book.
    pub fn apply(&mut self, update: &DepthUpdate) -> Result<SyncApplyOutcome, DepthApplyError> {
        if !update.symbol.eq_ignore_ascii_case(&self.symbol) {
            return Err(DepthApplyError::SymbolMismatch {
                expected: self.symbol.clone(),
                got: update.symbol.clone(),
            });
        }
        let Some(current_update_id) = self.book.last_update_id() else {
            return Err(DepthApplyError::SnapshotRequired);
        };
        if update.final_update_id <= current_update_id {
            debug!(
                target: "quantick::depth",
                schema_version = 1_u8,
                event_code = "depth_update_stale",
                symbol = self.symbol.as_str(),
                generation = self.generation,
                current_update_id,
                first_update_id = update.first_update_id,
                final_update_id = update.final_update_id,
                action = "ignore",
                "ignoring stale depth update"
            );
            return Ok(SyncApplyOutcome::Stale {
                final_update_id: update.final_update_id,
                current_update_id,
            });
        }

        let expected_update_id = current_update_id.saturating_add(1);
        if update.first_update_id > expected_update_id {
            warn!(
                target: "quantick::depth",
                schema_version = 1_u8,
                event_code = "depth_sequence_gap",
                symbol = self.symbol.as_str(),
                generation = self.generation,
                current_update_id,
                expected_update_id,
                first_update_id = update.first_update_id,
                final_update_id = update.final_update_id,
                action = "resync",
                "depth update gap detected"
            );
            return Err(DepthApplyError::Gap {
                current_update_id,
                expected_update_id,
                got_first_update_id: update.first_update_id,
                got_final_update_id: update.final_update_id,
            });
        }

        let delta = update.to_book_delta()?;
        let was_awaiting_bridge = matches!(self.phase, SyncPhase::AwaitingBridge { .. });
        match self.book.apply_delta(&delta)? {
            ApplyOutcome::Stale {
                final_update_id,
                current_update_id,
            } => Ok(SyncApplyOutcome::Stale {
                final_update_id,
                current_update_id,
            }),
            ApplyOutcome::Applied {
                first_update_id,
                final_update_id,
                changed_levels,
            } => {
                self.phase = SyncPhase::Synchronized {
                    current_update_id: final_update_id,
                };
                if was_awaiting_bridge {
                    info!(
                        target: "quantick::depth",
                        schema_version = 1_u8,
                        event_code = "depth_stream_bridged",
                        symbol = self.symbol.as_str(),
                        generation = self.generation,
                        first_update_id,
                        final_update_id,
                        bid_levels = self.book.bid_count(),
                        ask_levels = self.book.ask_count(),
                        action = "publish",
                        "depth stream bridged to snapshot"
                    );
                }
                Ok(SyncApplyOutcome::Applied {
                    bridged_snapshot: was_awaiting_bridge,
                    first_update_id,
                    final_update_id,
                    changed_levels,
                    bid_levels: self.book.bid_count(),
                    ask_levels: self.book.ask_count(),
                })
            }
        }
    }

    /// Current lifecycle phase.
    #[must_use]
    pub fn phase(&self) -> SyncPhase {
        self.phase
    }

    /// Read-only access to the synchronized generic order book.
    #[must_use]
    pub fn book(&self) -> &OrderBook {
        &self.book
    }

    /// Configured upper-case symbol.
    #[must_use]
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Connection/synchronization generation.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::depth::DepthLevel;

    fn level(price: i64, quantity: i64) -> DepthLevel {
        DepthLevel {
            price: Decimal::from(price),
            quantity: Decimal::from(quantity),
        }
    }

    fn snapshot(last_update_id: u64) -> DepthSnapshot {
        DepthSnapshot {
            last_update_id,
            bids: vec![level(99, 5)],
            asks: vec![level(101, 7)],
        }
    }

    fn update(first: u64, final_id: u64, bids: Vec<DepthLevel>) -> DepthUpdate {
        DepthUpdate {
            event_time_ms: 1_000,
            symbol: "BTCUSDT".to_string(),
            first_update_id: first,
            final_update_id: final_id,
            bids,
            asks: Vec::new(),
        }
    }

    #[test]
    fn snapshot_waits_for_a_bridging_update() {
        let mut sync = DepthSynchronizer::new("btcusdt", 7);
        let installed = sync.install_snapshot(&snapshot(100), 5_000).unwrap();
        assert_eq!(installed.snapshot_update_id, 100);
        assert_eq!(
            sync.phase(),
            SyncPhase::AwaitingBridge {
                snapshot_update_id: 100
            }
        );

        let outcome = sync.apply(&update(99, 101, vec![level(99, 4)])).unwrap();
        assert!(matches!(
            outcome,
            SyncApplyOutcome::Applied {
                bridged_snapshot: true,
                final_update_id: 101,
                ..
            }
        ));
        assert_eq!(
            sync.phase(),
            SyncPhase::Synchronized {
                current_update_id: 101
            }
        );
    }

    #[test]
    fn stale_event_is_ignored_without_mutation() {
        let mut sync = DepthSynchronizer::new("BTCUSDT", 1);
        sync.install_snapshot(&snapshot(100), 100).unwrap();
        let outcome = sync.apply(&update(90, 100, vec![level(99, 999)])).unwrap();
        assert_eq!(
            outcome,
            SyncApplyOutcome::Stale {
                final_update_id: 100,
                current_update_id: 100
            }
        );
        assert_eq!(sync.book().best_bid().unwrap().quantity(), Decimal::from(5));
    }

    #[test]
    fn gap_is_typed_and_does_not_advance_the_book() {
        let mut sync = DepthSynchronizer::new("BTCUSDT", 1);
        sync.install_snapshot(&snapshot(100), 100).unwrap();
        let error = sync
            .apply(&update(102, 103, vec![level(99, 1)]))
            .unwrap_err();
        assert_eq!(
            error,
            DepthApplyError::Gap {
                current_update_id: 100,
                expected_update_id: 101,
                got_first_update_id: 102,
                got_final_update_id: 103
            }
        );
        assert_eq!(sync.book().last_update_id(), Some(100));
        assert_eq!(sync.book().best_bid().unwrap().quantity(), Decimal::from(5));
    }

    #[test]
    fn zero_quantity_removes_a_level() {
        let mut sync = DepthSynchronizer::new("BTCUSDT", 1);
        sync.install_snapshot(&snapshot(100), 100).unwrap();
        sync.apply(&update(101, 101, vec![level(99, 0)])).unwrap();
        assert_eq!(sync.book().bid_count(), 0);
    }

    #[test]
    fn different_symbol_is_rejected() {
        let mut sync = DepthSynchronizer::new("BTCUSDT", 1);
        sync.install_snapshot(&snapshot(100), 100).unwrap();
        let mut wrong = update(101, 101, Vec::new());
        wrong.symbol = "ETHUSDT".to_string();
        assert!(matches!(
            sync.apply(&wrong),
            Err(DepthApplyError::SymbolMismatch { .. })
        ));
    }
}
