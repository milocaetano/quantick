//! Thread-free order-book state machine behind the heatmap.
//!
//! Owns the authoritative [`LiquidityHistory`], synchronization status,
//! diagnostic counters and the projection cache. It has no egui, no channels
//! and no clock other than projection-cache aging, so tests drive it
//! synchronously; at runtime [`crate::orderflow_worker`] owns one instance on a
//! dedicated thread and the UI only reads published snapshots.

use std::sync::Arc;
use std::time::{Duration, Instant};

use quantick_engine::{Bar, Trade};
use quantick_feed_binance::depth::{DepthEvent, DepthResyncReason, DepthStatus};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use crate::orderflow::{
    BarTimeline, HeatmapConfig, HeatmapProjection, HistoryStatus, LiquidityHistory, PriceWindow,
    project,
};

pub(crate) const PROJECTION_INTERVAL: Duration = Duration::from_millis(220);

/// Quantize the price window before it keys the projection cache, so a
/// sub-pixel wiggle of the auto-fit range (which happens almost every frame on a
/// live book) does not force a full re-projection. The window is snapped to
/// ~1/500 of its own span; a cached frame is at most that far off vertically.
fn quantize_price(value: f64, span: f64) -> u64 {
    let step = (span / 500.0).max(f64::MIN_POSITIVE);
    ((value / step).round() * step).to_bits()
}

/// Reference price of a snapshot (best bid/ask mid), for sizing the capture
/// bucket. Any level works — only the magnitude matters.
fn snapshot_reference_price(snapshot: &quantick_orderbook::BookSnapshot) -> Option<f64> {
    let best_bid = snapshot.bids().iter().map(|level| level.price()).max();
    let best_ask = snapshot.asks().iter().map(|level| level.price()).min();
    let price = match (best_bid, best_ask) {
        (Some(bid), Some(ask)) => (bid + ask) / Decimal::from(2),
        (Some(price), None) | (None, Some(price)) => price,
        (None, None) => return None,
    };
    price.to_f64()
}

/// A capture price bucket proportional to the asset's price, snapped to a
/// 1 / 2 / 5 · 10^k step near `price / 65_000` — so BTC (~65k) lands near $1, an
/// index (~130k) near $2, a ~5k contract near $0.1, and FX stays sub-cent. A
/// dense book at the 0.01 default explodes into tens of thousands of RLE runs;
/// this keeps the count affordable without per-asset tuning. Falls back to the
/// fine default when the price is unusable.
fn adaptive_base(price: f64) -> Decimal {
    if !price.is_finite() || price <= 0.0 {
        return Decimal::new(1, 2);
    }
    let target = (price / 65_000.0).clamp(1e-9, 1e6);
    let pow = 10f64.powf(target.log10().floor());
    let mantissa = target / pow; // in [1, 10)
    let nice = if mantissa < 1.5 {
        1.0
    } else if mantissa < 3.5 {
        2.0
    } else if mantissa < 7.5 {
        5.0
    } else {
        10.0
    };
    Decimal::from_f64(nice * pow).unwrap_or(Decimal::new(1, 2))
}

/// One visible, already-projected order-flow frame.
#[derive(Clone)]
pub struct VisibleOrderflow {
    pub(crate) projection: Arc<HeatmapProjection>,
    pub(crate) first_bar_index: usize,
    pub(crate) slot_count: usize,
}

/// Identity of one projection request; two equal layouts may share a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectionLayout {
    first_bar_index: usize,
    slot_count: usize,
    extend_live_end: bool,
    // Cell y-positions are normalized against the price window at build time.
    // The window is compared after `quantize_price` (~1/500 of its own span):
    // real pans/zooms rebuild, while the sub-pixel auto-fit wiggle of a live
    // book reuses the cached frame, which is then at most ~1/500 of the span
    // off vertically. A deliberate, bounded relaxation of the exact
    // bit-compare this key used before the projection went off-thread.
    price_low_bits: u64,
    price_high_bits: u64,
}

impl ProjectionLayout {
    #[must_use]
    pub(crate) fn new(
        first_bar_index: usize,
        slot_count: usize,
        extend_live_end: bool,
        price_range: (f64, f64),
    ) -> Self {
        let price_span = price_range.1 - price_range.0;
        Self {
            first_bar_index,
            slot_count,
            extend_live_end,
            price_low_bits: quantize_price(price_range.0, price_span),
            price_high_bits: quantize_price(price_range.1, price_span),
        }
    }
}

/// Everything the projection needs from one chart frame, owned so it can cross
/// the worker channel.
#[derive(Debug, Clone)]
pub(crate) struct ProjectionRequest {
    pub first_bar_index: usize,
    pub closed: Vec<Bar>,
    pub partial: Option<Bar>,
    pub extend_live_end: bool,
    pub price_range: (f64, f64),
}

impl ProjectionRequest {
    #[must_use]
    pub(crate) fn layout(&self) -> ProjectionLayout {
        ProjectionLayout::new(
            self.first_bar_index,
            self.closed.len() + usize::from(self.partial.is_some()),
            self.extend_live_end,
            self.price_range,
        )
    }
}

struct ProjectionCache {
    built_at: Instant,
    layout: ProjectionLayout,
    frame: Arc<VisibleOrderflow>,
}

/// Health data consumed by the periodic AI-first application summary.
#[derive(Debug, Clone)]
pub struct OrderflowHealth {
    pub enabled: bool,
    pub status: &'static str,
    pub generation: Option<u64>,
    pub last_update_id: Option<u64>,
    pub last_event_ms: Option<i64>,
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
    pub dropped_aggressions: usize,
    pub dropped_liquidity_events: usize,
    pub effective_grouping: Decimal,
    pub effective_grouping_multiple: u32,
    pub projection_ms: f32,
    pub projection_builds: u64,
    pub projection_cache_hits: u64,
    pub config_revision: u64,
    pub last_snapshot_observed_ms: Option<i64>,
    pub depth_updates: u64,
    pub depth_updates_since_summary: u64,
    pub snapshots: u64,
    pub gaps: u64,
}

impl OrderflowHealth {
    fn empty() -> Self {
        Self {
            enabled: false,
            status: "disabled",
            generation: None,
            last_update_id: None,
            last_event_ms: None,
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
            dropped_aggressions: 0,
            dropped_liquidity_events: 0,
            effective_grouping: Decimal::new(1, 2),
            effective_grouping_multiple: 1,
            projection_ms: 0.0,
            projection_builds: 0,
            projection_cache_hits: 0,
            config_revision: 0,
            last_snapshot_observed_ms: None,
            depth_updates: 0,
            depth_updates_since_summary: 0,
            snapshots: 0,
            gaps: 0,
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
            Self::Disconnected { error_class } => format!("book down · {error_class}"),
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
        }
    }
}

/// Authoritative book/heatmap state machine. See the module docs.
pub struct BookEngine {
    symbol: String,
    config: HeatmapConfig,
    history: LiquidityHistory,
    /// Size the capture bucket from the asset price on the first snapshot.
    /// Cleared once the user sets the base resolution by hand.
    auto_base: bool,
    status: CaptureStatus,
    generation_floor: u64,
    latest_generation: Option<u64>,
    last_snapshot_observed_ms: Option<i64>,
    depth_updates: u64,
    depth_updates_since_summary: u64,
    config_revision: u64,
    last_projection_cells: usize,
    last_projection_aggressions: usize,
    last_projection_liquidity_events: usize,
    last_dropped_cells: usize,
    last_dropped_aggressions: usize,
    last_dropped_liquidity_events: usize,
    last_effective_grouping: Decimal,
    last_effective_grouping_multiple: u32,
    last_projection_ms: f32,
    projection_builds: u64,
    projection_cache_hits: u64,
    projection_cache: Option<ProjectionCache>,
    /// Last successfully built frame. Survives soft cache invalidation so the
    /// UI never flashes to an empty heatmap between rebuilds; cleared by hard
    /// resets (symbol change, grouping reset, capture off).
    last_frame: Option<Arc<VisibleOrderflow>>,
}

impl BookEngine {
    #[must_use]
    pub fn new(symbol: impl Into<String>) -> Self {
        let config = HeatmapConfig::default();
        Self {
            symbol: symbol.into(),
            history: LiquidityHistory::new(config.clone()),
            config,
            auto_base: true,
            status: CaptureStatus::Disabled,
            generation_floor: 0,
            latest_generation: None,
            last_snapshot_observed_ms: None,
            depth_updates: 0,
            depth_updates_since_summary: 0,
            config_revision: 0,
            last_projection_cells: 0,
            last_projection_aggressions: 0,
            last_projection_liquidity_events: 0,
            last_dropped_cells: 0,
            last_dropped_aggressions: 0,
            last_dropped_liquidity_events: 0,
            last_effective_grouping: HeatmapConfig::default().price_grouping,
            last_effective_grouping_multiple: 1,
            last_projection_ms: 0.0,
            projection_builds: 0,
            projection_cache_hits: 0,
            projection_cache: None,
            last_frame: None,
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    #[cfg(test)]
    pub(crate) fn base_capture_grouping(&self) -> Decimal {
        self.config.price_grouping
    }

    /// Reset market-specific history while preserving visual/retention
    /// settings. Capture deliberately returns to off; the app starts a fresh
    /// provider task only after the new feed handle is installed.
    pub fn reset_for_symbol(&mut self, symbol: impl Into<String>) {
        self.symbol = symbol.into();
        self.config.enabled = false;
        // A new market has a new price scale; re-derive the capture bucket.
        self.auto_base = true;
        self.history = LiquidityHistory::new(self.config.clone());
        self.status = CaptureStatus::Disabled;
        self.generation_floor = 0;
        self.latest_generation = None;
        self.last_snapshot_observed_ms = None;
        self.depth_updates = 0;
        self.depth_updates_since_summary = 0;
        self.last_projection_cells = 0;
        self.last_projection_aggressions = 0;
        self.last_projection_liquidity_events = 0;
        self.last_dropped_cells = 0;
        self.last_dropped_aggressions = 0;
        self.last_dropped_liquidity_events = 0;
        self.last_effective_grouping = self.config.price_grouping;
        self.last_effective_grouping_multiple = 1;
        self.last_projection_ms = 0.0;
        self.projection_builds = 0;
        self.projection_cache_hits = 0;
        self.projection_cache = None;
        self.last_frame = None;
    }

    /// Commit a capture toggle only after its feed command was accepted.
    pub fn set_enabled(&mut self, enabled: bool, generation_floor: u64) {
        if self.config.enabled == enabled {
            return;
        }
        if !enabled {
            self.mark_gap_at_latest("capture_disabled");
        }
        self.config.enabled = enabled;
        self.generation_floor = generation_floor;
        self.invalidate_projection();
        if !enabled {
            self.last_frame = None;
        }
        self.status = if enabled {
            CaptureStatus::Connecting
        } else {
            CaptureStatus::Disabled
        };
        self.apply_non_grouping_config();
        self.config_revision = self.config_revision.saturating_add(1);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_CAPTURE_CHANGED",
            symbol = self.symbol.as_str(),
            enabled,
            generation_floor,
            config_revision = self.config_revision,
            action = if enabled { "start_capture" } else { "stop_capture" },
            "book heatmap capture changed"
        );
    }

    /// Mark the existing generation discontinuous before the feed is restarted.
    pub fn prepare_restart(&mut self, generation_floor: u64, reason: &'static str) {
        self.mark_gap_at_latest(reason);
        self.generation_floor = generation_floor;
        self.status = CaptureStatus::Connecting;
        self.invalidate_projection();
    }

    /// Apply every setting except the capture bucket and the enabled flag.
    ///
    /// The engine owns `price_grouping` (the auto-base logic can rewrite it
    /// from live data), so a visual-config update from the UI must never carry
    /// a stale bucket back in. Grouping changes use
    /// [`apply_grouping_now`](Self::apply_grouping_now) or
    /// [`accept_grouping_restart`](Self::accept_grouping_restart).
    pub fn apply_visual_config(&mut self, mut config: HeatmapConfig) {
        config.price_grouping = self.config.price_grouping;
        config.enabled = self.config.enabled;
        config.sanitize();
        if config == self.config {
            return;
        }
        self.config = config;
        self.invalidate_projection();
        self.apply_non_grouping_config();
        self.log_config_changed(false, "reproject");
    }

    /// Change the capture bucket immediately (capture disabled or empty
    /// history): retained runs cannot be reinterpreted, so history resets.
    pub fn apply_grouping_now(&mut self, grouping: Decimal) {
        if grouping == self.config.price_grouping {
            return;
        }
        // The bucket was picked by hand; stop auto-sizing it.
        self.auto_base = false;
        match self.history.reset_price_grouping(grouping) {
            Ok(reset) => {
                self.config.price_grouping = grouping;
                self.invalidate_projection();
                self.last_frame = None;
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_GROUPING_RESET",
                    symbol = self.symbol.as_str(),
                    previous_grouping = %reset.previous,
                    current_grouping = %reset.current,
                    dropped_runs = reset.dropped_runs,
                    dropped_aggressions = reset.dropped_aggressions,
                    action = "apply_while_capture_disabled",
                    "base price grouping changed; historical runs were reset honestly"
                );
                self.apply_non_grouping_config();
                self.log_config_changed(true, "reproject");
            }
            Err(error) => {
                self.log_history_error(
                    "grouping_reset",
                    self.latest_generation.unwrap_or(0),
                    &error,
                );
            }
        }
    }

    /// Commit a staged base-grouping change after the feed accepted its
    /// restart command. Until this point the old history remained fully usable.
    pub fn accept_grouping_restart(&mut self, grouping: Decimal, generation_floor: u64) {
        self.auto_base = false;
        match self.history.reset_price_grouping(grouping) {
            Ok(reset) => {
                self.config.price_grouping = grouping;
                self.last_frame = None;
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_GROUPING_RESET",
                    symbol = self.symbol.as_str(),
                    previous_grouping = %reset.previous,
                    current_grouping = %reset.current,
                    dropped_runs = reset.dropped_runs,
                    dropped_aggressions = reset.dropped_aggressions,
                    action = "restart_capture",
                    "accepted capture restart; base grouping and retained L2 history were reset"
                );
                self.log_config_changed(true, "request_capture_restart");
            }
            Err(error) => {
                self.log_history_error(
                    "grouping_reset",
                    self.latest_generation.unwrap_or(0),
                    &error,
                );
            }
        }
        self.apply_non_grouping_config();
        self.prepare_restart(generation_floor, "configuration_restart");
    }

    /// Record a factual aggregate trade for the aggression overlay.
    pub fn record_trade(&mut self, trade: &Trade) {
        if self.config.enabled {
            self.history.record_aggression(trade);
        }
    }

    /// Apply one already-synchronized feed event.
    pub fn handle_depth_event(&mut self, event: DepthEvent) {
        let (symbol, generation) = match &event {
            DepthEvent::Status {
                symbol, generation, ..
            }
            | DepthEvent::Snapshot {
                symbol, generation, ..
            }
            | DepthEvent::Update {
                symbol, generation, ..
            } => (symbol, *generation),
        };
        if !self.config.enabled {
            return;
        }
        if !symbol.eq_ignore_ascii_case(&self.symbol) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_SYMBOL_MISMATCH",
                expected_symbol = self.symbol.as_str(),
                got_symbol = symbol.as_str(),
                generation,
                action = "ignore_event",
                "ignored order-book event for another symbol"
            );
            return;
        }
        let minimum_generation = self
            .latest_generation
            .unwrap_or(self.generation_floor)
            .max(self.generation_floor);
        if generation < minimum_generation {
            tracing::debug!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_STALE_GENERATION",
                symbol = self.symbol.as_str(),
                generation,
                generation_floor = self.generation_floor,
                latest_generation = self.latest_generation,
                minimum_generation,
                action = "ignore_event",
                "ignored queued event from a stopped depth task"
            );
            return;
        }
        self.latest_generation = Some(generation);

        match event {
            DepthEvent::Status { status, .. } => self.handle_status(generation, status),
            DepthEvent::Snapshot {
                observed_at_ms,
                effective_at_ms,
                snapshot,
                ..
            } => {
                self.invalidate_projection();
                self.last_snapshot_observed_ms = Some(observed_at_ms);
                // Size the capture bucket from the asset price before the first
                // snapshot of a capture, so a dense book (BTC etc.) doesn't
                // explode into runs. Skipped once the user picks a base by hand.
                if self.auto_base
                    && matches!(self.history.status(), HistoryStatus::Empty)
                    && let Some(price) = snapshot_reference_price(&snapshot)
                {
                    self.apply_auto_base(adaptive_base(price));
                }
                if let Err(error) =
                    self.history
                        .install_snapshot(effective_at_ms, generation, snapshot)
                {
                    self.log_history_error("snapshot", generation, &error);
                    self.mark_gap_at_latest("snapshot_rejected");
                    self.status = CaptureStatus::Error;
                } else {
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "HEATMAP_SNAPSHOT_APPLIED",
                        symbol = self.symbol.as_str(),
                        generation,
                        observed_at_ms,
                        effective_at_ms,
                        bid_levels = self.history.book().bid_count(),
                        ask_levels = self.history.book().ask_count(),
                        coverage = ?self.history.book().coverage(),
                        action = "open_coverage",
                        "book snapshot installed in heatmap history"
                    );
                }
            }
            DepthEvent::Update {
                event_time_ms,
                delta,
                ..
            } => match self.history.apply_delta(event_time_ms, &delta) {
                Ok(_) => {
                    self.depth_updates = self.depth_updates.saturating_add(1);
                    self.depth_updates_since_summary =
                        self.depth_updates_since_summary.saturating_add(1);
                }
                Err(error) => {
                    self.log_history_error("delta", generation, &error);
                    self.mark_gap_at_latest("delta_rejected");
                    self.status = CaptureStatus::Error;
                }
            },
        }
    }

    fn handle_status(&mut self, generation: u64, status: DepthStatus) {
        self.status = match status {
            DepthStatus::Connecting => CaptureStatus::Connecting,
            DepthStatus::Buffering { .. } => CaptureStatus::Buffering,
            DepthStatus::SnapshotFetching { .. } => CaptureStatus::SnapshotFetching,
            DepthStatus::Synchronized { last_update_id, .. } => CaptureStatus::Live {
                generation,
                last_update_id,
            },
            DepthStatus::Resyncing { reason } => {
                let reason = resync_reason_code(&reason);
                self.mark_gap_at_latest(reason);
                CaptureStatus::Resyncing { reason }
            }
            DepthStatus::Disconnected { error_class } => {
                self.mark_gap_at_latest(error_class);
                CaptureStatus::Disconnected { error_class }
            }
            DepthStatus::Stopped => CaptureStatus::Disabled,
        };
    }

    fn mark_gap_at_latest(&mut self, reason: &'static str) {
        let Some(timestamp_ms) = self.history.latest_book_ms() else {
            return;
        };
        if matches!(self.history.status(), HistoryStatus::Gap { .. }) {
            return;
        }
        if let Err(error) = self.history.mark_gap(timestamp_ms, reason) {
            self.log_history_error("gap", self.latest_generation.unwrap_or(0), &error);
        } else {
            self.invalidate_projection();
        }
    }

    fn log_history_error(
        &self,
        operation: &'static str,
        generation: u64,
        error: &dyn std::fmt::Display,
    ) {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_HISTORY_REJECTED",
            symbol = self.symbol.as_str(),
            generation,
            operation,
            error = %error,
            action = "wait_for_fresh_snapshot",
            "order-flow history rejected an event"
        );
    }

    fn log_config_changed(&mut self, capture_grouping_changed: bool, action: &'static str) {
        self.config_revision = self.config_revision.saturating_add(1);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "HEATMAP_CONFIG_CHANGED",
            symbol = self.symbol.as_str(),
            config_revision = self.config_revision,
            enabled = self.config.enabled,
            retention_ms = self.config.retention_ms,
            price_grouping = %self.config.price_grouping,
            display_grouping = ?self.config.display_grouping,
            opacity = self.config.opacity,
            gamma = self.config.gamma,
            show_aggressions = self.config.show_aggressions,
            bubble_cluster_ms = self.config.bubble_cluster_ms,
            show_liquidity_events = self.config.show_liquidity_events,
            liquidity_correlation_ms = self.config.liquidity_correlation_ms,
            theme = ?self.config.theme,
            capture_grouping_changed,
            action,
            "heatmap configuration changed"
        );
    }

    /// Build (or reuse) renderer-independent primitives for one request.
    pub(crate) fn project(&mut self, request: &ProjectionRequest) -> Option<Arc<VisibleOrderflow>> {
        if !self.config.enabled {
            return None;
        }
        let layout = request.layout();
        let now = Instant::now();
        if let Some(cache) = &self.projection_cache
            && cache.layout == layout
            && now.saturating_duration_since(cache.built_at) < PROJECTION_INTERVAL
        {
            self.projection_cache_hits = self.projection_cache_hits.saturating_add(1);
            return Some(Arc::clone(&cache.frame));
        }

        let low = Decimal::from_f64(request.price_range.0)?;
        let high = Decimal::from_f64(request.price_range.1)?;
        let prices = PriceWindow::new(low, high)?;
        let timeline = BarTimeline::from_bars(
            request.first_bar_index,
            &request.closed,
            request.partial.as_ref(),
            if request.extend_live_end {
                self.history.latest_book_ms()
            } else {
                None
            },
        );
        if timeline.is_empty() {
            return None;
        }
        let started = now;
        let projection = project(&self.history, &timeline, prices);
        self.last_projection_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.last_projection_cells = projection.cells.len();
        self.last_projection_aggressions = projection.aggressions.len();
        self.last_projection_liquidity_events = projection.liquidity_events.len();
        self.last_dropped_cells = projection.dropped_cells;
        self.last_dropped_aggressions = projection.dropped_aggressions;
        self.last_dropped_liquidity_events = projection.dropped_liquidity_events;
        self.last_effective_grouping = projection.effective_grouping.bucket_width;
        self.last_effective_grouping_multiple = projection.effective_grouping.multiple;
        self.projection_builds = self.projection_builds.saturating_add(1);
        let frame = Arc::new(VisibleOrderflow {
            projection: Arc::new(projection),
            first_bar_index: request.first_bar_index,
            slot_count: timeline.len(),
        });
        self.projection_cache = Some(ProjectionCache {
            built_at: now,
            layout,
            frame: Arc::clone(&frame),
        });
        self.last_frame = Some(Arc::clone(&frame));
        Some(frame)
    }

    fn invalidate_projection(&mut self) {
        self.projection_cache = None;
    }

    fn apply_non_grouping_config(&mut self) {
        if let Err(error) = self.history.update_config(self.config.clone()) {
            self.log_history_error("config_update", self.latest_generation.unwrap_or(0), &error);
        }
    }

    /// Adopt a price-derived capture bucket while history is still empty (so no
    /// data is discarded), keeping config and history in sync.
    fn apply_auto_base(&mut self, base: Decimal) {
        if base <= Decimal::ZERO || base == self.config.price_grouping {
            return;
        }
        match self.history.reset_price_grouping(base) {
            Ok(_) => {
                self.config.price_grouping = base;
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "HEATMAP_AUTO_BASE",
                    symbol = self.symbol.as_str(),
                    base = %base,
                    action = "size_capture_bucket_from_price",
                    "auto-sized L2 capture bucket from asset price"
                );
            }
            Err(error) => {
                self.log_history_error("auto_base", self.latest_generation.unwrap_or(0), &error);
            }
        }
    }

    pub fn reset_summary_counters(&mut self) {
        self.depth_updates_since_summary = 0;
    }

    #[must_use]
    pub fn health(&self) -> OrderflowHealth {
        let (generation, last_update_id, last_event_ms) = match self.history.status() {
            HistoryStatus::Synced {
                generation,
                last_update_id,
                last_event_ms,
            } => (Some(generation), last_update_id, Some(last_event_ms)),
            HistoryStatus::Gap {
                from_generation, ..
            } => (
                from_generation,
                self.history.book().last_update_id(),
                self.history.latest_book_ms(),
            ),
            HistoryStatus::Empty => (None, None, None),
        };
        let counters = self.history.counters();
        OrderflowHealth {
            enabled: self.config.enabled,
            status: self.status.code(),
            generation,
            last_update_id,
            last_event_ms,
            bid_levels: self.history.book().bid_count(),
            ask_levels: self.history.book().ask_count(),
            active_levels: self.history.active_level_count(),
            archived_runs: self.history.archived_run_count(),
            aggression_count: self.history.aggressions().count(),
            history_bytes: self.history.approximate_history_bytes(),
            projection_cells: self.last_projection_cells,
            projection_aggressions: self.last_projection_aggressions,
            projection_liquidity_events: self.last_projection_liquidity_events,
            dropped_cells: self.last_dropped_cells,
            dropped_aggressions: self.last_dropped_aggressions,
            dropped_liquidity_events: self.last_dropped_liquidity_events,
            effective_grouping: self.last_effective_grouping,
            effective_grouping_multiple: self.last_effective_grouping_multiple,
            projection_ms: self.last_projection_ms,
            projection_builds: self.projection_builds,
            projection_cache_hits: self.projection_cache_hits,
            config_revision: self.config_revision,
            last_snapshot_observed_ms: self.last_snapshot_observed_ms,
            depth_updates: self.depth_updates,
            depth_updates_since_summary: self.depth_updates_since_summary,
            snapshots: counters.snapshots,
            gaps: counters.gaps,
        }
    }

    /// Snapshot everything the UI needs, cheap enough to run per event batch.
    #[must_use]
    pub fn published(&self) -> BookPublished {
        BookPublished {
            status: self.status.clone(),
            health: self.health(),
            live_end_ms: if self.config.enabled {
                self.history.latest_book_ms()
            } else {
                None
            },
            base_price_grouping: self.config.price_grouping,
            frame: self.last_frame.clone(),
        }
    }
}

fn resync_reason_code(reason: &DepthResyncReason) -> &'static str {
    match reason {
        DepthResyncReason::SnapshotTooOld { .. } => "snapshot_too_old",
        DepthResyncReason::SequenceGap { .. } => "sequence_gap",
        DepthResyncReason::SnapshotAttemptsExhausted { .. } => "snapshot_attempts_exhausted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

    #[test]
    fn adaptive_base_scales_with_price_and_stays_round() {
        // BTC ~ $1, an index ~ $2, a ~5k contract ~ $0.1, FX stays sub-cent.
        assert_eq!(adaptive_base(65_000.0), Decimal::from(1));
        assert_eq!(adaptive_base(118_000.0), Decimal::from(2));
        assert_eq!(adaptive_base(5_000.0), Decimal::new(1, 1)); // 0.1
        assert!(adaptive_base(1.1) < Decimal::new(1, 2)); // < 0.01
        // Every result is a positive 1/2/5 * 10^k step.
        for price in [0.5, 12.0, 300.0, 4_200.0, 65_000.0, 250_000.0] {
            let base = adaptive_base(price);
            assert!(base > Decimal::ZERO, "price {price} -> {base}");
        }
        // Degenerate prices fall back to the fine default.
        assert_eq!(adaptive_base(0.0), Decimal::new(1, 2));
        assert_eq!(adaptive_base(f64::NAN), Decimal::new(1, 2));
    }

    #[test]
    fn auto_base_sizes_the_capture_bucket_from_the_first_snapshot() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 1);
        // A ~65k book: the 0.01 default should auto-size up to ~$1.
        engine.handle_depth_event(DepthEvent::Snapshot {
            symbol: "BTCUSDT".to_owned(),
            generation: 1,
            observed_at_ms: 1_100,
            effective_at_ms: 1_000,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(64_999), Decimal::from(2)).unwrap()],
                vec![BookLevel::new(Decimal::from(65_001), Decimal::from(3)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
        assert_eq!(engine.base_capture_grouping(), Decimal::from(1));
        assert!(engine.history.book().is_initialized());
    }

    fn snapshot_event(generation: u64) -> DepthEvent {
        DepthEvent::Snapshot {
            symbol: "BTCUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1000,
                },
            ),
        }
    }

    fn bar(open_time: i64, close_time: i64) -> Bar {
        Bar {
            open_time,
            close_time,
            open: Decimal::from(100),
            high: Decimal::from(102),
            low: Decimal::from(98),
            close: Decimal::from(101),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    fn request(bars: &[Bar], price_range: (f64, f64)) -> ProjectionRequest {
        ProjectionRequest {
            first_bar_index: 0,
            closed: bars.to_vec(),
            partial: None,
            extend_live_end: true,
            price_range,
        }
    }

    #[test]
    fn disabled_engine_ignores_book_and_trade_events() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.handle_depth_event(snapshot_event(1));
        engine.record_trade(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: Decimal::from(100),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        assert!(!engine.history.book().is_initialized());
        assert_eq!(engine.history.aggressions().count(), 0);
    }

    #[test]
    fn effective_snapshot_time_allows_earlier_buffered_observation() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 100);
        engine.handle_depth_event(snapshot_event(100));
        assert_eq!(engine.history.latest_book_ms(), Some(999));
        assert_eq!(engine.last_snapshot_observed_ms, Some(1_100));
    }

    #[test]
    fn projection_is_cached_at_depth_cadence_and_gap_invalidates_it() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 10);
        engine.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];

        let first = engine.project(&request(&bars, (98.0, 102.0))).unwrap();
        let second = engine.project(&request(&bars, (98.0, 102.0))).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(engine.projection_builds, 1);
        assert_eq!(engine.projection_cache_hits, 1);

        // A price-window change (pan/zoom, auto-fit shift) must rebuild:
        // cached cell positions are normalized against the old window.
        let panned = engine.project(&request(&bars, (97.0, 103.0))).unwrap();
        assert!(!Arc::ptr_eq(&first, &panned));
        assert_eq!(engine.projection_builds, 2);

        engine.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            status: DepthStatus::Resyncing {
                reason: DepthResyncReason::SequenceGap {
                    expected_update_id: 11,
                    first_update_id: 12,
                    final_update_id: 13,
                },
            },
        });
        let after_gap = engine.project(&request(&bars, (98.0, 102.0))).unwrap();
        assert!(!Arc::ptr_eq(&first, &after_gap));
        assert_eq!(engine.projection_builds, 3);
    }

    #[test]
    fn sub_quantum_price_wiggle_reuses_the_cached_frame() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 10);
        engine.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];

        let first = engine.project(&request(&bars, (98.0, 102.0))).unwrap();
        // The auto-fit range wiggles by far less than 1/500 of the span almost
        // every frame on a live book; the quantized cache key must absorb it
        // (a cached frame is at most ~1/500 of the span off vertically).
        let wiggled = engine
            .project(&request(&bars, (98.000_5, 102.000_5)))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &wiggled), "sub-quantum shift is a hit");
        assert_eq!(engine.projection_builds, 1);

        // A shift past the quantum is a real pan and must rebuild.
        let panned = engine.project(&request(&bars, (98.05, 102.05))).unwrap();
        assert!(!Arc::ptr_eq(&first, &panned));
        assert_eq!(engine.projection_builds, 2);
    }

    #[test]
    fn soft_invalidation_keeps_the_last_frame_published() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 10);
        engine.handle_depth_event(snapshot_event(10));
        let bars = [bar(900, 1_100)];
        let frame = engine.project(&request(&bars, (98.0, 102.0))).unwrap();

        // A gap invalidates the cache but the published frame survives, so the
        // UI never flashes to an empty heatmap while the rebuild is queued.
        engine.prepare_restart(11, "test_gap");
        let published = engine.published();
        assert!(published.frame.is_some());
        assert!(Arc::ptr_eq(&published.frame.unwrap(), &frame));

        // Disabling capture is a hard reset: the frame must vanish.
        engine.set_enabled(false, 12);
        assert!(engine.published().frame.is_none());
    }

    #[test]
    fn events_below_capture_generation_are_ignored() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 500);
        engine.handle_depth_event(snapshot_event(499));
        assert!(!engine.history.book().is_initialized());
    }

    #[test]
    fn delayed_event_from_an_older_reconnect_generation_is_ignored() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 10);
        engine.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 12,
            status: DepthStatus::Connecting,
        });
        engine.handle_depth_event(snapshot_event(11));
        assert!(!engine.history.book().is_initialized());
        assert_eq!(engine.latest_generation, Some(12));
    }

    #[test]
    fn rejected_delta_closes_coverage_immediately() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 10);
        engine.handle_depth_event(snapshot_event(10));
        engine.handle_depth_event(DepthEvent::Update {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            event_time_ms: 998,
            delta: BookDelta::new(11, 11, Vec::new(), Vec::new()),
        });

        assert!(matches!(engine.history.status(), HistoryStatus::Gap { .. }));
        assert!(!engine.history.book().is_initialized());
        assert_eq!(engine.history.coverage_gaps().count(), 1);
    }

    #[test]
    fn resync_status_opens_an_explicit_gap() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 10);
        engine.handle_depth_event(snapshot_event(10));
        engine.handle_depth_event(DepthEvent::Status {
            symbol: "BTCUSDT".to_owned(),
            generation: 10,
            status: DepthStatus::Resyncing {
                reason: DepthResyncReason::SequenceGap {
                    expected_update_id: 11,
                    first_update_id: 12,
                    final_update_id: 13,
                },
            },
        });
        assert!(matches!(engine.history.status(), HistoryStatus::Gap { .. }));
        assert_eq!(engine.history.coverage_gaps().count(), 1);
    }

    #[test]
    fn visual_config_update_never_carries_a_stale_bucket_back_in() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.set_enabled(true, 1);
        engine.handle_depth_event(snapshot_event(1));
        let base_after_auto = engine.base_capture_grouping();
        assert!(engine.history.book().is_initialized());

        // A visual update built from a stale UI mirror (default bucket) must
        // not reset the engine's auto-sized bucket or its history.
        let mut stale = HeatmapConfig {
            gamma: 1.2,
            ..HeatmapConfig::default()
        };
        stale.enabled = false; // also stale; engine keeps its own flag
        engine.apply_visual_config(stale);
        assert_eq!(engine.base_capture_grouping(), base_after_auto);
        assert!(engine.enabled());
        assert!(engine.history.book().is_initialized());
        assert_eq!(engine.history.config().gamma, 1.2);
    }

    #[test]
    fn grouping_now_resets_history_and_disables_auto_base() {
        let mut engine = BookEngine::new("BTCUSDT");
        engine.apply_grouping_now(Decimal::new(5, 1));
        assert_eq!(engine.base_capture_grouping(), Decimal::new(5, 1));

        // The next snapshot must keep the hand-picked bucket.
        engine.set_enabled(true, 1);
        engine.handle_depth_event(snapshot_event(1));
        assert_eq!(engine.base_capture_grouping(), Decimal::new(5, 1));
    }
}
