//! Provider-neutral vocabulary for a synchronized depth stream.
//!
//! A consumer of live market depth needs three things regardless of where the
//! data comes from: a lifecycle it can render honestly ([`DepthStatus`]), one
//! authoritative starting image per generation, and absolute updates on top of
//! it ([`DepthEvent`]). Those are pure domain concepts, so they live here next
//! to [`OrderBook`](crate::OrderBook) rather than inside any one feed crate —
//! otherwise every new venue would either invent its own dialect or make the
//! app depend on an unrelated exchange's crate.
//!
//! The vocabulary deliberately says nothing about *how* a source stays
//! synchronized. Binance publishes a REST snapshot plus incremental diffs with
//! exchange update ids; MetaTrader's DOM publishes a complete image on every
//! book event and has no ids at all. Both reach this contract — the second one
//! through [`SnapshotDiffer`](crate::SnapshotDiffer), which turns full images
//! into the same snapshot-then-delta shape.
//!
//! Update ids are therefore *the local stream's* ids: exchange-assigned where
//! the venue has them, synthetic and monotonic where it does not. They order
//! and gap-check one generation; they mean nothing across generations.

use rust_decimal::Decimal;

use crate::{BookDelta, BookSnapshot};

/// Why a synchronized stream must be discarded and rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthResyncReason {
    /// The starting image was older than the earliest eligible buffered update.
    SnapshotTooOld {
        /// Snapshot update id.
        snapshot_update_id: u64,
        /// First id in the event that could not bridge it.
        first_update_id: u64,
        /// Final id in that event.
        final_update_id: u64,
    },
    /// At least one update id was missed.
    SequenceGap {
        /// Next required update id.
        expected_update_id: u64,
        /// First received id.
        first_update_id: u64,
        /// Final received id.
        final_update_id: u64,
    },
    /// Bootstrap retried too many times on one connection.
    SnapshotAttemptsExhausted {
        /// Number of attempted snapshots.
        attempts: u32,
    },
    /// The source restarted its book without a sequence break we could bridge
    /// (a bridge session ended, a terminal resubscribed, a venue reset).
    ///
    /// Sources whose images carry no ids use this instead of
    /// [`SequenceGap`](Self::SequenceGap): there is no id to point at, only the
    /// fact that continuity ended.
    SourceRestarted {
        /// Stable machine-readable cause.
        cause: &'static str,
    },
}

/// Consumer-visible depth lifecycle.
///
/// Not every source reaches every state: a venue with no bootstrap buffering
/// goes straight from [`Connecting`](Self::Connecting) to
/// [`Synchronized`](Self::Synchronized).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthStatus {
    /// The transport is being established.
    Connecting,
    /// The transport is open and updates are being buffered until the starting
    /// image arrives.
    Buffering {
        /// Configured hard buffer limit.
        capacity: usize,
    },
    /// A starting image is being fetched while the transport stays live.
    SnapshotFetching {
        /// Attempt number within this connection generation.
        attempt: u32,
        /// Updates already buffered.
        buffered_updates: usize,
    },
    /// Image and stream are continuous and consumer data is authoritative
    /// within the declared snapshot coverage.
    Synchronized {
        /// Current final update id.
        last_update_id: u64,
        /// Current stored bid level count.
        bid_levels: usize,
        /// Current stored ask level count.
        ask_levels: usize,
    },
    /// Current state is being discarded.
    Resyncing {
        /// Typed reason for the resync.
        reason: DepthResyncReason,
    },
    /// A connection generation ended.
    Disconnected {
        /// Stable machine-readable error class.
        error_class: &'static str,
    },
    /// Consumer cancellation stopped the reconnect loop.
    Stopped,
}

/// Typed output consumed by a chart or another order-book client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepthEvent {
    /// Lifecycle/status transition.
    Status {
        /// Symbol as the consumer configured it.
        symbol: String,
        /// Monotonic capture/session generation.
        generation: u64,
        /// New status.
        status: DepthStatus,
    },
    /// Starting image, emitted exactly once per successful generation before
    /// its first delta.
    Snapshot {
        /// Symbol as the consumer configured it.
        symbol: String,
        /// Monotonic capture/session generation.
        generation: u64,
        /// Local epoch milliseconds when the image was observed.
        ///
        /// Sources whose starting image carries no exchange timestamp report
        /// local observation time here, and it is labelled as such.
        observed_at_ms: i64,
        /// Logical start time for the image in the consumer timeline.
        ///
        /// An update can be buffered before the image arrives, so its exchange
        /// event time may precede `observed_at_ms`. This value is at most one
        /// millisecond before the earliest bridged event and never after the
        /// local observation, preventing a false backwards-time delta while
        /// keeping the observation timestamp available for diagnostics.
        effective_at_ms: i64,
        /// Smallest price increment of the instrument, when the source knows
        /// it. `None` means "not declared" — consumers must not invent one;
        /// they may fall back to a heuristic and should label it as such.
        ///
        /// Rendering liquidity on buckets finer than the real increment leaves
        /// permanently empty rows between real ones, so a source that knows its
        /// tick size saves the consumer from guessing.
        price_step: Option<Decimal>,
        /// Validated snapshot.
        snapshot: BookSnapshot,
    },
    /// One absolute update.
    Update {
        /// Symbol as the consumer configured it.
        symbol: String,
        /// Monotonic capture/session generation.
        generation: u64,
        /// Source event time in epoch milliseconds.
        event_time_ms: i64,
        /// Validated absolute delta.
        delta: BookDelta,
    },
}

impl DepthEvent {
    /// The symbol every variant carries.
    #[must_use]
    pub fn symbol(&self) -> &str {
        match self {
            Self::Status { symbol, .. }
            | Self::Snapshot { symbol, .. }
            | Self::Update { symbol, .. } => symbol,
        }
    }

    /// The capture generation every variant carries.
    #[must_use]
    pub fn generation(&self) -> u64 {
        match self {
            Self::Status { generation, .. }
            | Self::Snapshot { generation, .. }
            | Self::Update { generation, .. } => *generation,
        }
    }
}
