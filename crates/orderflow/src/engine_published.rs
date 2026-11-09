//! What the engine publishes for the UI to read between frames: the capture
//! status the badge shows, the health counters the summary logs, the bounded
//! book ladder and the snapshot that carries them. Pure data; the engine
//! fills it in [`super::BookEngine::published`].

use std::sync::Arc;

use quantick_orderbook::BookLevel;
use rust_decimal::Decimal;

#[cfg(doc)]
use super::BookEngine;
use super::VisibleOrderflow;
use crate::HeatmapConfig;
#[cfg(doc)]
use crate::LiquidityHistory;
#[cfg(doc)]
use crate::constants::LADDER_LEVELS_PER_SIDE;

/// Health data consumed by the periodic AI-first application summary.
#[derive(Debug, Clone)]
pub struct OrderflowHealth {
    pub enabled: bool,
    pub status: &'static str,
    pub generation: Option<u64>,
    pub last_update_id: Option<u64>,
    pub last_event_ms: Option<i64>,
    /// Source-to-UI delay observed for the newest accepted timestamped event.
    pub arrival_latency_ms: Option<i64>,
    /// How old the newest mark on the tape is, or that there is none — the
    /// gap the trader reads as bubbles trailing the tape's right edge.
    /// See [`LiquidityHistory::tape_age`].
    pub tape_age: Option<crate::TapeAge>,
    pub bid_levels: usize,
    pub ask_levels: usize,
    pub active_levels: usize,
    pub archived_runs: usize,
    pub aggression_count: usize,
    pub history_bytes: usize,
    pub projection_cells: usize,
    pub projection_aggressions: usize,
    pub projection_liquidity_events: usize,
    pub dropped_cells: usize,
    pub folded_aggressions: usize,
    /// Exact quantity the trader's own display floor kept off the canvas.
    pub floored_quantity: Decimal,
    pub dropped_liquidity_events: usize,
    pub effective_grouping: Decimal,
    pub effective_grouping_multiple: u32,
    pub projection_ms: f32,
    /// Cost of the half rebuilt for every frame.
    pub live_ms: f32,
    pub projection_builds: u64,
    pub projection_cache_hits: u64,
    pub config_revision: u64,
    pub last_snapshot_observed_ms: Option<i64>,
    pub depth_updates: u64,
    pub depth_updates_since_summary: u64,
    pub snapshots: u64,
    pub gaps: u64,
    /// Aggressions the retention window or a capacity cap removed from the
    /// history, cumulative: the bubbles that left the canvas by design.
    pub aggressions_evicted: u64,
    /// Finalized liquidity runs removed the same way, cumulative.
    pub runs_evicted: u64,
}

impl OrderflowHealth {
    pub(super) fn empty() -> Self {
        Self {
            enabled: false,
            status: "disabled",
            generation: None,
            last_update_id: None,
            last_event_ms: None,
            arrival_latency_ms: None,
            tape_age: None,
            bid_levels: 0,
            ask_levels: 0,
            active_levels: 0,
            archived_runs: 0,
            aggression_count: 0,
            history_bytes: 0,
            projection_cells: 0,
            projection_aggressions: 0,
            projection_liquidity_events: 0,
            dropped_cells: 0,
            folded_aggressions: 0,
            floored_quantity: Decimal::ZERO,
            dropped_liquidity_events: 0,
            effective_grouping: Decimal::new(1, 2),
            effective_grouping_multiple: 1,
            projection_ms: 0.0,
            live_ms: 0.0,
            projection_builds: 0,
            projection_cache_hits: 0,
            config_revision: 0,
            last_snapshot_observed_ms: None,
            depth_updates: 0,
            depth_updates_since_summary: 0,
            snapshots: 0,
            gaps: 0,
            aggressions_evicted: 0,
            runs_evicted: 0,
        }
    }
}

/// Capture status shown by the UI badge. Pure data; colors live in the view.
#[derive(Debug, Clone)]
pub enum CaptureStatus {
    Disabled,
    Connecting,
    Buffering,
    SnapshotFetching,
    Live {
        generation: u64,
        last_update_id: u64,
    },
    Resyncing {
        reason: &'static str,
    },
    Disconnected {
        error_class: &'static str,
    },
    Error,
}

impl CaptureStatus {
    pub fn label(&self) -> String {
        match self {
            Self::Disabled => "book off".to_owned(),
            Self::Connecting => "book connecting".to_owned(),
            Self::Buffering => "book buffering".to_owned(),
            Self::SnapshotFetching => "book syncing".to_owned(),
            Self::Live {
                generation,
                last_update_id,
            } => format!("book live · gen {generation} · #{last_update_id}"),
            Self::Resyncing { reason } => format!("book resync · {reason}"),
            Self::Disconnected { error_class } => {
                format!("book offline · {error_class}").replace('_', " ")
            }
            Self::Error => "book error".to_owned(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Connecting => "connecting",
            Self::Buffering => "buffering",
            Self::SnapshotFetching => "snapshot_fetching",
            Self::Live { .. } => "live",
            Self::Resyncing { .. } => "resyncing",
            Self::Disconnected { .. } => "disconnected",
            Self::Error => "error",
        }
    }

    /// Whether this status is a failure the trader has to be told about: the
    /// book stopped and the depth on screen is no longer the book.
    ///
    /// The canvas badge is the only place that says so in real time — the
    /// loading overlay covers the *waiting* states only, and the dock's strip
    /// carries no status at all. So this is the one thing hiding the badge may
    /// not silence: data honesty outranks a clean canvas, and stale depth that
    /// reads as live is exactly the lie the rule exists to prevent.
    /// Exhaustive, so a new status has to decide whether it is a failure.
    pub fn is_failure(&self) -> bool {
        match self {
            Self::Disconnected { .. } | Self::Error => true,
            Self::Disabled
            | Self::Connecting
            | Self::Buffering
            | Self::SnapshotFetching
            | Self::Live { .. }
            | Self::Resyncing { .. } => false,
        }
    }

    /// Whether capture is working towards a live book — the states the app's
    /// loading overlay shows as a wait. Exhaustive so a new status must decide
    /// whether it reads as "loading" instead of silently defaulting to "no".
    pub fn is_syncing(&self) -> bool {
        match self {
            Self::Connecting
            | Self::Buffering
            | Self::SnapshotFetching
            | Self::Resyncing { .. } => true,
            Self::Disabled | Self::Live { .. } | Self::Disconnected { .. } | Self::Error => false,
        }
    }
}

/// Bounded copy of the current order book, cut best-first around the spread.
/// Raw exchange levels, not heatmap buckets: each consumer (price axis, live
/// strip) aggregates them at whatever grouping its own view uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookLadder {
    /// Highest bid of the whole book, even when it sits outside the window
    /// the per-side lists were clipped to.
    pub best_bid: Option<BookLevel>,
    /// Lowest ask of the whole book, same contract as `best_bid`.
    pub best_ask: Option<BookLevel>,
    /// Bid levels best-first (descending price), at most
    /// [`LADDER_LEVELS_PER_SIDE`], clipped to the last known view window.
    pub bids: Vec<BookLevel>,
    /// Ask levels best-first (ascending price), same bounds as `bids`.
    pub asks: Vec<BookLevel>,
}

/// One immutable snapshot of everything the UI reads between frames.
#[derive(Clone)]
pub struct BookPublished {
    pub status: CaptureStatus,
    pub health: OrderflowHealth,
    pub live_end_ms: Option<i64>,
    /// The engine's current capture bucket. The auto-base logic can change it
    /// without a user action, so the UI mirrors it from here.
    pub base_price_grouping: Decimal,
    pub frame: Option<Arc<VisibleOrderflow>>,
    /// The native tape held at a past instant ([`BookEngine::set_tape_end`]).
    pub past_tape: Option<Arc<crate::projection::PastTape>>,
    /// First instant the retained tape is complete from.
    pub tape_retained_from_ms: Option<i64>,
    /// Current book around the spread; `None` while capture is off or the
    /// book has no snapshot yet. Shared through `Arc` so the per-frame clone
    /// of this snapshot stays cheap.
    pub ladder: Option<Arc<BookLadder>>,
}

impl BookPublished {
    #[must_use]
    pub fn initial() -> Self {
        Self {
            status: CaptureStatus::Disabled,
            health: OrderflowHealth::empty(),
            live_end_ms: None,
            base_price_grouping: HeatmapConfig::default().price_grouping,
            frame: None,
            past_tape: None,
            tape_retained_from_ms: None,
            ladder: None,
        }
    }
}
